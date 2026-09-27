use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
};

use ess_conformance::target::{
    ConformanceTarget, EventObservationRequest, ExternalOutcomeControl, ImplementationIdentity,
    ObservedEvent, RedeliveryRequest, ScenarioContext, SemanticCommandRequest,
    SemanticCommandResult, SemanticViewRequest, SemanticViewResult, TargetError,
};
use ess_primitives::{consistency::ConsistencyToken, node::Node};
use llm_core::{Error, TurnDocument};
use llm_routing::Catalog;
use serde::Deserialize;
use serde_json::{Value, json};

/// Only returned production facts are retained. This target never reads the suite,
/// its expected assertions, or a scenario name to determine an answer.
pub struct CatalogTarget {
    version: String,
    observation: RefCell<Option<BTreeMap<String, Node>>>,
    token: RefCell<Option<ConsistencyToken>>,
    sequence: Cell<u64>,
    observed_view: RefCell<Option<String>>,
}
impl CatalogTarget {
    pub const fn new(version: String) -> Self {
        Self {
            version,
            observation: RefCell::new(None),
            token: RefCell::new(None),
            sequence: Cell::new(0),
            observed_view: RefCell::new(None),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvaluationRequest {
    catalog_toml: String,
    turn_json: String,
    #[serde(default, deserialize_with = "token_bound")]
    input_tokens: Option<u64>,
}

// ESS serializes exactly integral witnesses as `1.0`. Use its exact integer
// accessor, never a lossy float cast, when crossing into the Rust u64 API.
pub(crate) fn token_bound<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u64>, D::Error> {
    let node = Option::<Node>::deserialize(deserializer)?;
    match node {
        None => Ok(None),
        Some(Node::Number(number)) => number
            .as_i64()
            .and_then(|v| u64::try_from(v).ok())
            .map(Some)
            .ok_or_else(|| {
                serde::de::Error::custom("token bound must be a nonnegative exact integer")
            }),
        _ => Err(serde::de::Error::custom("token bound must be an integer")),
    }
}

fn observe(input: &EvaluationRequest) -> Value {
    let mut facts = json!({
        "catalog_valid": false, "error_code": null, "selected_target": null,
        "model": null, "url": null, "protocol": null, "auth_kind": null,
        "billing_kind": null, "config_digest": null, "input_tokens": input.input_tokens,
        "rejections": [], "request_preserved": false
    });
    let result = evaluate(input, &mut facts);
    if let Err(error) = result {
        facts["error_code"] = json!(error.code);
    }
    facts
}

fn evaluate(input: &EvaluationRequest, facts: &mut Value) -> Result<(), Error> {
    let catalog = Catalog::parse(&input.catalog_toml)?;
    facts["catalog_valid"] = json!(true);
    facts["config_digest"] = json!(catalog.digest());
    let document: TurnDocument = serde_json::from_str(&input.turn_json)
        .map_err(|_| Error::invalid("invalid turn envelope"))?;
    let request = document.request;
    let explanation = catalog.explain(&request, input.input_tokens)?;
    facts["selected_target"] = json!(explanation.selected_target_id);
    facts["input_tokens"] = json!(explanation.input_tokens);
    facts["rejections"] = json!(
        explanation
            .targets
            .iter()
            .flat_map(|target| {
                target
                    .rejections
                    .iter()
                    .map(|reason| format!("{}:{}", target.target_id, reason.label()))
            })
            .collect::<Vec<_>>()
    );
    let selection = catalog.resolve(&request, input.input_tokens)?;
    facts["model"] = json!(selection.request.model);
    facts["url"] = json!(selection.binding.request_url());
    facts["protocol"] = json!(selection.binding.provenance().protocol);
    facts["auth_kind"] = json!(selection.binding.declaration().account.auth_kind);
    facts["billing_kind"] = json!(selection.binding.declaration().account.billing_kind);
    let mut restored = selection.request;
    restored.model.clone_from(&request.model);
    facts["request_preserved"] = json!(restored == request);
    Ok(())
}

/// What a domain module observed: the facts its public library returned, the view that
/// answers for them, the event the command emits, and the fact whose truth that event carries.
///
/// A domain module owns its own file and returns this; `execute_command` below is shared and
/// stays out of every unit's assignment.
pub struct Observed {
    pub facts: Value,
    pub view: &'static str,
    pub event: &'static str,
    pub field: &'static str,
}

fn unavailable(error: impl std::fmt::Display) -> TargetError {
    TargetError::unavailable("catalog observation", error.to_string())
}
fn unsupported(operation: &str) -> TargetError {
    TargetError::unsupported(
        operation,
        "library observation adapter exposes no such operation",
    )
}

impl ConformanceTarget for CatalogTarget {
    fn identity(&self) -> Result<ImplementationIdentity, TargetError> {
        Ok(ImplementationIdentity::new(
            "llm-foundation-libraries",
            &self.version,
        ))
    }
    fn begin_scenario(&self, _: &ScenarioContext) -> Result<(), TargetError> {
        self.observation.replace(None);
        self.token.replace(None);
        self.observed_view.replace(None);
        Ok(())
    }
    fn end_scenario(&self, _: &ScenarioContext) -> Result<(), TargetError> {
        self.observation.replace(None);
        self.token.replace(None);
        self.observed_view.replace(None);
        Ok(())
    }
    fn execute_command(
        &self,
        request: SemanticCommandRequest,
    ) -> Result<SemanticCommandResult, TargetError> {
        let input = serde_json::to_value(request.input).map_err(unavailable)?;
        let command = request.command.to_string();
        // Each domain module answers only its own commands and returns `None` otherwise,
        // so a new domain is a new file rather than an edit to this shared one.
        let owned = crate::messages::observe(&command, &input)
            .or_else(|| crate::responses::observe(&command, &input))
            .or_else(|| crate::chat::observe(&command, &input))
            .or_else(|| crate::hosting::observe(&command, &input))
            .or_else(|| crate::fallback::observe(&command, &input))
            .or_else(|| crate::runpod::observe(&command, &input))
            .or_else(|| crate::gateway::observe(&command, &input))
            .or_else(|| crate::providers::observe(&command, &input))
            .or_else(|| crate::transport::observe(&command, &input));
        let (facts, view, event, field) = if let Some(observed) = owned {
            let observed = observed?;
            (
                observed.facts,
                observed.view,
                observed.event,
                observed.field,
            )
        } else {
            match command.as_str() {
                "llm.inference.InspectResult" => (
                    crate::inference::observe(&serde_json::from_value(input).map_err(unavailable)?),
                    "llm.inference.LastResult",
                    "llm.inference.Inspected",
                    "accepted",
                ),
                "llm.routing.Evaluate" => (
                    observe(&serde_json::from_value(input).map_err(unavailable)?),
                    "llm.routing.LastEvaluation",
                    "llm.routing.Evaluated",
                    "catalog_valid",
                ),
                "llm.secrets.ProbeFile" => (
                    crate::secrets::file(input).map_err(unavailable)?,
                    "llm.secrets.LastProbe",
                    "llm.secrets.Probed",
                    "diagnostics_safe",
                ),
                "llm.secrets.ProbeKeychain" => (
                    crate::secrets::keychain(input).map_err(unavailable)?,
                    "llm.secrets.LastProbe",
                    "llm.secrets.Probed",
                    "diagnostics_safe",
                ),
                "llm.accounting.Quote" => (
                    crate::pricing::observe(&serde_json::from_value(input).map_err(unavailable)?),
                    "llm.accounting.LastQuote",
                    "llm.accounting.Quoted",
                    "accepted",
                ),
                "llm.budget.Exercise" => (
                    crate::budgets::observe(&serde_json::from_value(input).map_err(unavailable)?)
                        .map_err(unavailable)?,
                    "llm.budget.LastExecution",
                    "llm.budget.Exercised",
                    "valid_program",
                ),
                _ => return Err(unsupported(&command)),
            }
        };
        let notification = facts[field]
            .as_bool()
            .ok_or_else(|| unavailable("missing notification fact"))?;
        self.observed_view.replace(Some(view.to_owned()));
        self.observation
            .replace(Some(serde_json::from_value(facts).map_err(unavailable)?));
        // This local adapter notification says observation finished. The actual
        // route/refusal is asserted independently through LastEvaluation.
        let mut result = SemanticCommandResult::took(ess_conformance::scenario::OutcomeRef::new(
            request.command,
            "observed".parse().map_err(unavailable)?,
        ));
        let sequence = self
            .sequence
            .get()
            .checked_add(1)
            .ok_or_else(|| unavailable("sequence exhausted"))?;
        self.sequence.set(sequence);
        let token = ConsistencyToken::new(format!("{}:{sequence}", request.correlation))
            .map_err(unavailable)?;
        self.token.replace(Some(token.clone()));
        result.consistency = Some(token);
        result.direct_events.push(
            ObservedEvent::new(event.parse().map_err(unavailable)?)
                .with(field, Node::Bool(notification)),
        );
        Ok(result)
    }
    fn query_view(&self, request: SemanticViewRequest) -> Result<SemanticViewResult, TargetError> {
        let view = request.view.to_string();
        let known = matches!(
            view.as_str(),
            "llm.routing.LastEvaluation"
                | "llm.secrets.LastProbe"
                | "llm.accounting.LastQuote"
                | "llm.budget.LastExecution"
                | "llm.inference.LastResult"
        ) || [
            crate::messages::VIEWS,
            crate::responses::VIEWS,
            crate::chat::VIEWS,
            crate::hosting::VIEWS,
            crate::fallback::VIEWS,
            crate::runpod::VIEWS,
            crate::gateway::VIEWS,
            crate::providers::VIEWS,
            crate::transport::VIEWS,
        ]
        .iter()
        .any(|views| views.contains(&view.as_str()));
        if !known || !request.params.is_empty() {
            return Err(unsupported(&view));
        }
        if self.observed_view.borrow().as_ref() != Some(&view) {
            return Ok(SemanticViewResult::of(std::iter::empty()));
        }
        if request
            .consistency
            .token()
            .is_some_and(|token| Some(token) != self.token.borrow().as_ref())
        {
            return Err(unavailable(
                "requested observation is not retained in this scenario",
            ));
        }
        Ok(SemanticViewResult::of(
            self.observation.borrow().iter().cloned(),
        ))
    }
    fn observe_events(
        &self,
        _: EventObservationRequest,
    ) -> Result<Vec<ObservedEvent>, TargetError> {
        Err(unsupported("asynchronous events"))
    }
    fn configure_external_outcome(&self, _: ExternalOutcomeControl) -> Result<(), TargetError> {
        Err(unsupported("forcing an outcome"))
    }
    fn redeliver_event(&self, _: RedeliveryRequest) -> Result<(), TargetError> {
        Err(unsupported("redelivering an event"))
    }
}
