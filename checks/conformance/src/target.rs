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
}
impl CatalogTarget {
    pub const fn new(version: String) -> Self {
        Self {
            version,
            observation: RefCell::new(None),
            token: RefCell::new(None),
            sequence: Cell::new(0),
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
fn token_bound<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<u64>, D::Error> {
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

fn unavailable(error: impl std::fmt::Display) -> TargetError {
    TargetError::unavailable("catalog observation", error.to_string())
}
fn unsupported(operation: &str) -> TargetError {
    TargetError::unsupported(
        operation,
        "pure catalog evaluation exposes no such operation",
    )
}

impl ConformanceTarget for CatalogTarget {
    fn identity(&self) -> Result<ImplementationIdentity, TargetError> {
        Ok(ImplementationIdentity::new(
            "llm-catalog-libraries",
            &self.version,
        ))
    }
    fn begin_scenario(&self, _: &ScenarioContext) -> Result<(), TargetError> {
        self.observation.replace(None);
        self.token.replace(None);
        Ok(())
    }
    fn end_scenario(&self, _: &ScenarioContext) -> Result<(), TargetError> {
        self.observation.replace(None);
        self.token.replace(None);
        Ok(())
    }
    fn execute_command(
        &self,
        request: SemanticCommandRequest,
    ) -> Result<SemanticCommandResult, TargetError> {
        if request.command.to_string() != "llm.routing.Evaluate" {
            return Err(unsupported(&request.command.to_string()));
        }
        let input: EvaluationRequest =
            serde_json::from_value(serde_json::to_value(request.input).map_err(unavailable)?)
                .map_err(unavailable)?;
        let facts = observe(&input);
        let valid = facts["catalog_valid"]
            .as_bool()
            .ok_or_else(|| unavailable("missing validity fact"))?;
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
            ObservedEvent::new("llm.routing.Evaluated".parse().map_err(unavailable)?)
                .with("catalog_valid", Node::Bool(valid)),
        );
        Ok(result)
    }
    fn query_view(&self, request: SemanticViewRequest) -> Result<SemanticViewResult, TargetError> {
        if request.view.to_string() != "llm.routing.LastEvaluation" || !request.params.is_empty() {
            return Err(unsupported(&request.view.to_string()));
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
