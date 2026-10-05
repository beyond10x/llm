//! Ordered-fallback conformance observations.
//!
//! This module drives the real `Catalog::run_turn` over in-process scripted `Model`s, a real pause
//! port and a bounded sink, and reports what the library returned. The scripted models only replay
//! the authored failure or success; every attempt, wait, warning, halt, refusal and evidence line
//! below comes back out of the library. It reads no suite and branches on no scenario name.

use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use ess_conformance::target::TargetError;
use llm_core::{
    BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item, Model, Provenance,
    StopReason, StreamEvent, StreamSink, TurnDocument, TurnObservation, TurnOutcome, TurnRequest,
    Usage, VecSink,
};
use llm_routing::{
    AttemptResult, Catalog, FallbackPolicy, FallbackRun, Models, Ports, RetryPolicy, Selection,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::target::{Observed, token_bound};

/// Every view name this domain answers through `query_view`.
pub const VIEWS: &[&str] = &["llm.routing.LastFallback"];

const MAX_PROGRAM_BYTES: usize = 64 * 1024;
const MAX_EMITS: usize = 64;
const MAX_SINK_EVENTS: usize = 256;
const MAX_SINK_BYTES: usize = 64 * 1024;
/// Ends a run whose pause port was told to hang and whose cancellation was ignored.
const BACKSTOP: Duration = Duration::from_secs(30);

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
    /// A deadline this many milliseconds after the run starts.
    #[serde(default)]
    deadline_in_ms: Option<u64>,
    #[serde(default)]
    retry: RetryInput,
    #[serde(default)]
    cancel_on_pause: bool,
    #[serde(default)]
    within_ms: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Script {
    #[serde(default)]
    emits: usize,
    #[serde(default)]
    fail: Option<Failure>,
    #[serde(default)]
    fail_times: Option<usize>,
    #[serde(default)]
    input_tokens: Option<u64>,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(deny_unknown_fields)]
struct Failure {
    code: ErrorCode,
    dispatch: Dispatch,
    #[serde(default)]
    retriable: bool,
    #[serde(default)]
    retry_after_ms: Option<u64>,
}

/// The caller's retry policy; every absent field is the library default.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct RetryInput {
    max_attempts: Option<u32>,
    backoff_base_ms: Option<u64>,
    max_doublings: Option<u32>,
    max_server_delay_ms: Option<u64>,
}

impl RetryInput {
    fn policy(&self) -> RetryPolicy {
        let default = RetryPolicy::DEFAULT;
        RetryPolicy {
            max_attempts: self.max_attempts.unwrap_or(default.max_attempts),
            backoff_base: self
                .backoff_base_ms
                .map_or(default.backoff_base, Duration::from_millis),
            max_doublings: self.max_doublings.unwrap_or(default.max_doublings),
            max_server_delay: self
                .max_server_delay_ms
                .map_or(default.max_server_delay, Duration::from_millis),
        }
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Replays one authored script. It chooses nothing about fallback or retry.
struct Scripted {
    provenance: Provenance,
    capabilities: Capabilities,
    emits: usize,
    fail: Option<Failure>,
    /// Fails only the first n attempts, then succeeds; every attempt when absent.
    fail_times: Option<usize>,
    input_tokens: Option<u64>,
    calls: AtomicUsize,
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
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            let fail = self
                .fail
                .filter(|_| self.fail_times.is_none_or(|times| call < times));
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
            match fail {
                None => {
                    observation.final_usage = true;
                    Ok(TurnOutcome {
                        stop_reason: StopReason::EndTurn,
                        items: vec![Item::assistant("done")],
                        observation,
                    })
                }
                Some(failure) => {
                    let mut error = Error::new(failure.code, "scripted failure")
                        .with_dispatch(failure.dispatch)
                        .with_retriable(failure.retriable);
                    error.retry_after_ms = failure.retry_after_ms;
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
        "delivered_text": null, "retry_policy": null, "warnings": [], "paused_ms": [],
        "retriable": null, "error_message": null, "ended_within": null
    });
    if let Err(error) = execute(&request, &mut facts) {
        failure_facts(&error, &mut facts);
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

/// One scripted model per authored serving model, each bound to its declared binding.
fn scripted_fleet(
    models: BTreeMap<String, Script>,
    catalog: &Catalog,
) -> Result<BTreeMap<Id, Scripted>, Error> {
    let mut fleet = BTreeMap::new();
    for (serving, script) in models {
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
                fail_times: script.fail_times,
                input_tokens: script.input_tokens,
                calls: AtomicUsize::new(0),
            },
        );
    }
    Ok(fleet)
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
    let fleet = scripted_fleet(program.models, &catalog)?;
    let retry = program.retry.policy();
    facts["retry_policy"] = json!({
        "max_attempts": retry.max_attempts,
        "backoff_base_ms": millis(retry.backoff_base),
        "max_doublings": retry.max_doublings,
        "max_server_delay_ms": millis(retry.max_server_delay),
    });
    let policy = FallbackPolicy {
        max_attempts: program
            .max_attempts
            .unwrap_or(FallbackPolicy::default().max_attempts),
        deadline: program.deadline_passed.then(Instant::now).or_else(|| {
            program
                .deadline_in_ms
                .map(|ms| Instant::now() + Duration::from_millis(ms))
        }),
        retry,
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
    let paused = Mutex::new(Vec::new());
    let cancel_on_pause = program.cancel_on_pause;
    // A real wait. With `cancel_on_pause` the port cancels the run and then never ends by itself,
    // so only the library's own race against cancellation can end the run before the backstop.
    let pause = |wait: Duration| -> BoxFuture<'static, ()> {
        if let Ok(mut paused) = paused.lock() {
            paused.push(millis(wait));
        }
        if cancel_on_pause {
            cancel.cancel();
            Box::pin(std::future::pending())
        } else {
            Box::pin(tokio::time::sleep(wait))
        }
    };
    let started = Instant::now();
    let run = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .map_err(|_| Error::new(ErrorCode::Unavailable, "no local runtime"))?
        // The backstop's timer is created inside the runtime, not while building its argument.
        .block_on(async {
            tokio::time::timeout(
                BACKSTOP,
                catalog.run_turn(
                    &document.request,
                    input.input_tokens,
                    policy,
                    Ports {
                        models: &models,
                        admit: &mut admit,
                        sink: &mut sink,
                        cancel: &cancel,
                        pause: &pause,
                    },
                ),
            )
            .await
        });
    let elapsed = started.elapsed();
    if let Some(within) = program.within_ms {
        facts["ended_within"] = json!(elapsed <= Duration::from_millis(within));
    }
    facts["paused_ms"] = json!(
        paused
            .lock()
            .map(|paused| paused.clone())
            .unwrap_or_default()
    );
    let Ok(run) = run else {
        facts["error_code"] = json!("fixture:backstop");
        return Ok(());
    };
    let run = run?;
    record(&run, facts);
    facts["delivered_text"] = json!(sink.text());
    facts["warnings"] = json!(
        sink.events()
            .iter()
            .filter_map(|event| match event {
                StreamEvent::Warning { code, message } => Some(format!("{code}: {message}")),
                _ => None,
            })
            .collect::<Vec<_>>()
    );
    Ok(())
}

fn failure_facts(error: &Error, facts: &mut Value) {
    facts["error_code"] = json!(error.code);
    facts["retriable"] = json!(error.retriable);
    facts["error_message"] = json!(error.message);
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
        failure_facts(error, facts);
    }
}
