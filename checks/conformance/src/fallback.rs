//! Ordered-fallback conformance observations.
//!
//! This module drives the real `Catalog::run_turn` over in-process scripted `Model`s and reports
//! what the library returned. The scripted models only replay the authored failure or success;
//! every attempt, halt, refusal and evidence line below comes back out of the library. It reads no
//! suite and branches on no scenario name.

use std::{collections::BTreeMap, time::Instant};

use ess_conformance::target::TargetError;
use llm_core::{
    BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item, Model, Provenance,
    StopReason, StreamEvent, StreamSink, TurnDocument, TurnObservation, TurnOutcome, TurnRequest,
    Usage, VecSink,
};
use llm_routing::{AttemptResult, Catalog, FallbackPolicy, FallbackRun, Models, Ports, Selection};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::target::{Observed, token_bound};

/// Every view name this domain answers through `query_view`.
pub const VIEWS: &[&str] = &["llm.routing.LastFallback"];

const MAX_PROGRAM_BYTES: usize = 64 * 1024;
const MAX_EMITS: usize = 64;
const MAX_SINK_EVENTS: usize = 256;
const MAX_SINK_BYTES: usize = 64 * 1024;

/// Observe one command of this domain, or `None` when the command belongs to another.
pub fn observe(command: &str, input: &Value) -> Option<Result<Observed, TargetError>> {
    if command != "llm.routing.Fallback" {
        return None;
    }
    Some(exercise(input))
}

fn unavailable(error: impl std::fmt::Display) -> TargetError {
    TargetError::unavailable("fallback observation", error.to_string())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FallbackInput {
    catalog_toml: String,
    turn_json: String,
    #[serde(default, deserialize_with = "token_bound")]
    input_tokens: Option<u64>,
    program_json: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Program {
    models: BTreeMap<String, Script>,
    #[serde(default)]
    refuse_admission: Vec<String>,
    #[serde(default)]
    max_attempts: Option<usize>,
    #[serde(default)]
    deadline_passed: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Script {
    #[serde(default)]
    emits: usize,
    #[serde(default)]
    fail: Option<Failure>,
    #[serde(default)]
    input_tokens: Option<u64>,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(deny_unknown_fields)]
struct Failure {
    code: ErrorCode,
    dispatch: Dispatch,
}

/// Replays one authored script. It chooses nothing about fallback.
struct Scripted {
    provenance: Provenance,
    capabilities: Capabilities,
    emits: usize,
    fail: Option<Failure>,
    input_tokens: Option<u64>,
}

impl Model for Scripted {
    fn provenance(&self) -> &Provenance {
        &self.provenance
    }
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }
    fn turn<'a>(
        &'a self,
        _: &'a TurnRequest,
        sink: &'a mut dyn StreamSink,
        _: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        Box::pin(async move {
            for _ in 0..self.emits {
                sink.emit(StreamEvent::TextDelta {
                    text: format!("from {};", self.provenance.endpoint.as_str()),
                })
                .await?;
            }
            let mut observation = TurnObservation::new(self.provenance.clone());
            observation.usage = self.input_tokens.map(|input| Usage {
                input_tokens: Some(input),
                ..Usage::default()
            });
            match self.fail {
                None => {
                    observation.final_usage = true;
                    Ok(TurnOutcome {
                        stop_reason: StopReason::EndTurn,
                        items: vec![Item::assistant("done")],
                        observation,
                    })
                }
                Some(failure) => {
                    let error = Error::new(failure.code, "scripted failure")
                        .with_dispatch(failure.dispatch);
                    Err(if failure.dispatch == Dispatch::NotSent {
                        error
                    } else {
                        error.with_observation(observation)
                    })
                }
            }
        })
    }
}

struct Fleet(BTreeMap<Id, Scripted>);

impl Models for Fleet {
    fn model(&self, serving_model_id: &Id) -> Option<&dyn Model> {
        self.0
            .get(serving_model_id)
            .map(|model| model as &dyn Model)
    }
}

fn exercise(input: &Value) -> Result<Observed, TargetError> {
    let request: FallbackInput = serde_json::from_value(input.clone()).map_err(unavailable)?;
    let mut facts = json!({
        "valid_program": false, "error_code": null, "halt": null, "attempted": [],
        "attempt_results": [], "visible_events": [], "attempt_usage": [], "rejections": [],
        "delivered_text": null
    });
    if let Err(error) = execute(&request, &mut facts) {
        facts["error_code"] = json!(error.code);
    }
    Ok(Observed {
        facts,
        view: "llm.routing.LastFallback",
        event: "llm.routing.FallbackObserved",
        field: "valid_program",
    })
}

fn label(value: impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn execute(input: &FallbackInput, facts: &mut Value) -> Result<(), Error> {
    if input.program_json.len() > MAX_PROGRAM_BYTES {
        return Err(Error::too_large("fallback program exceeds its bound"));
    }
    let program: Program = serde_json::from_str(&input.program_json)
        .map_err(|_| Error::invalid("invalid fallback program"))?;
    facts["valid_program"] = json!(true);
    let catalog = Catalog::parse(&input.catalog_toml)?;
    let document: TurnDocument = serde_json::from_str(&input.turn_json)
        .map_err(|_| Error::invalid("invalid turn envelope"))?;
    let mut fleet = BTreeMap::new();
    for (serving, script) in program.models {
        if script.emits > MAX_EMITS {
            return Err(Error::too_large("scripted emission exceeds its bound"));
        }
        let serving = Id::new(&serving).map_err(|_| Error::invalid("invalid serving model"))?;
        let binding = catalog
            .binding(&serving)
            .ok_or_else(|| Error::invalid("scripted serving model is not declared"))?;
        fleet.insert(
            serving,
            Scripted {
                provenance: binding.provenance().clone(),
                capabilities: binding.capabilities().clone(),
                emits: script.emits,
                fail: script.fail,
                input_tokens: script.input_tokens,
            },
        );
    }
    let policy = FallbackPolicy {
        max_attempts: program
            .max_attempts
            .unwrap_or(FallbackPolicy::default().max_attempts),
        deadline: program.deadline_passed.then(Instant::now),
    };
    let refused = program.refuse_admission;
    let mut admit = |selection: &Selection<'_>| {
        if refused.iter().any(|id| id == selection.target.id.as_str()) {
            Err(Error::new(ErrorCode::Refused, "caller limit exhausted"))
        } else {
            Ok(())
        }
    };
    let models = Fleet(fleet);
    let mut sink = VecSink::new(MAX_SINK_EVENTS, MAX_SINK_BYTES);
    let cancel = Cancel::new();
    let run = tokio::runtime::Builder::new_current_thread()
        .build()
        .map_err(|_| Error::new(ErrorCode::Unavailable, "no local runtime"))?
        .block_on(catalog.run_turn(
            &document.request,
            input.input_tokens,
            policy,
            Ports {
                models: &models,
                admit: &mut admit,
                sink: &mut sink,
                cancel: &cancel,
            },
        ))?;
    record(&run, facts);
    facts["delivered_text"] = json!(sink.text());
    Ok(())
}

fn record(run: &FallbackRun, facts: &mut Value) {
    facts["halt"] = json!(run.halt.label());
    facts["attempted"] = json!(
        run.attempts
            .iter()
            .map(|attempt| attempt.target_id.as_str())
            .collect::<Vec<_>>()
    );
    facts["attempt_results"] = json!(
        run.attempts
            .iter()
            .map(|attempt| match &attempt.result {
                AttemptResult::Succeeded { .. } => format!("{}:succeeded", attempt.target_id),
                AttemptResult::Failed { code, dispatch, .. } => {
                    format!("{}:{}:{}", attempt.target_id, label(code), label(dispatch))
                }
            })
            .collect::<Vec<_>>()
    );
    facts["visible_events"] = json!(
        run.attempts
            .iter()
            .map(|attempt| format!("{}:{}", attempt.target_id, attempt.visible_events))
            .collect::<Vec<_>>()
    );
    facts["attempt_usage"] = json!(
        run.attempts
            .iter()
            .map(|attempt| {
                let observation = match &attempt.result {
                    AttemptResult::Succeeded { observation } => Some(observation),
                    AttemptResult::Failed { observation, .. } => observation.as_ref(),
                };
                let input = observation
                    .and_then(|observation| observation.usage.as_ref())
                    .and_then(|usage| usage.input_tokens)
                    .map_or_else(|| "unknown".to_owned(), |input| input.to_string());
                format!("{}:input={input}", attempt.target_id)
            })
            .collect::<Vec<_>>()
    );
    facts["rejections"] = json!(
        run.explanation
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
    if let Err(error) = &run.result {
        facts["error_code"] = json!(error.code);
    }
}
