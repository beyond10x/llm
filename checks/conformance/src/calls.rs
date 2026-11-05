//! Call record conformance observations.
//!
//! This module drives the real `llm_calls::record_turn` over the real `Catalog::run_turn`, with
//! in-process scripted `Model`s, a scripted clock, the caller's price book and an in-memory
//! record sink, and reports every record the sink received. The scripted models only replay the
//! authored answer or failure and move the clock as the program says; every record field comes
//! back out of the library. It reads no suite and branches on no scenario name.

use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, AtomicUsize, Ordering},
    time::Duration,
};

use ess_conformance::target::TargetError;
use llm_calls::{CallIdentity, CallLog, Clock, Recording, record_turn};
use llm_core::{
    BoxFuture, CallId, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item, Model,
    Provenance, StopReason, StreamEvent, StreamSink, ToolCall, TurnDocument, TurnObservation,
    TurnOutcome, TurnRequest, Usage, VecSink,
};
use llm_cost::PriceBook;
use llm_routing::{Catalog, FallbackPolicy, Models, Ports, Selection};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::target::Observed;

/// Every view name this domain answers through `query_view`.
pub const VIEWS: &[&str] = &["llm.calls.LastCall"];

const MAX_PROGRAM_BYTES: usize = 64 * 1024;
const MAX_SINK_EVENTS: usize = 256;
const MAX_SINK_BYTES: usize = 64 * 1024;

/// Observe one command of this domain, or `None` when the command belongs to another.
pub fn observe(command: &str, input: &Value) -> Option<Result<Observed, TargetError>> {
    match command {
        "llm.calls.Call" => Some(exercise(input)),
        "llm.calls.Replay" => Some(replay(input)),
        _ => None,
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[expect(
    dead_code,
    reason = "read only to refuse an input that is not the command's"
)]
struct ReplayInput {
    catalog_toml: String,
    corpus_json: String,
    prices_toml: String,
    run_json: String,
    program_json: String,
}

/// No library runs a replay yet: the command is answered, accepts no program and says so.
fn replay(input: &Value) -> Result<Observed, TargetError> {
    let _: ReplayInput = serde_json::from_value(input.clone()).map_err(unavailable)?;
    Ok(Observed {
        facts: json!({
            "valid_program": false, "error_code": json!(ErrorCode::Unsupported),
            "delivered_text": null, "records": [], "shadow_results": [], "case_results": [],
            "spent_today": null
        }),
        view: "llm.calls.LastCall",
        event: "llm.calls.Observed",
        field: "valid_program",
    })
}

fn unavailable(error: impl std::fmt::Display) -> TargetError {
    TargetError::unavailable("call record observation", error.to_string())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CallInput {
    catalog_toml: String,
    turn_json: String,
    #[serde(default)]
    prices_toml: Option<String>,
    #[serde(default)]
    shadows_json: Option<String>,
    program_json: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Program {
    record_id: String,
    /// The caller's input-token estimate, which selection admits each target against.
    #[serde(default)]
    input_tokens: Option<u64>,
    /// The wall clock when the call starts; the monotonic clock starts at zero.
    #[serde(default)]
    start_unix_ms: u64,
    models: BTreeMap<String, Script>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Script {
    /// Text shown as one delta and returned as the assistant's message.
    #[serde(default)]
    answer: Option<String>,
    /// Arguments, as JSON text, of a call to the turn's first published tool, shown and returned.
    #[serde(default)]
    tool_arguments: Option<String>,
    /// Milliseconds that pass on the scripted clock before the attempt shows anything.
    #[serde(default)]
    first_event_after_ms: u64,
    /// Milliseconds that pass after it showed everything, before it returns.
    #[serde(default)]
    ends_after_ms: u64,
    #[serde(default)]
    fail: Option<Failure>,
    /// Fails only the first n attempts, then answers; every attempt when absent.
    #[serde(default)]
    fail_times: Option<usize>,
    /// The counters the attempt reports; absent reports none.
    #[serde(default)]
    usage: Option<Usage>,
    #[serde(default)]
    upstream_model: Option<String>,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct Failure {
    code: ErrorCode,
    dispatch: Dispatch,
    #[serde(default)]
    retriable: bool,
    /// The failure's message, which the caller receives and no record may hold.
    #[serde(default)]
    message: Option<String>,
}

/// A clock that moves only when a scripted model says time passed.
struct ScriptedClock {
    start_unix_ms: u64,
    now_ms: AtomicU64,
}

impl ScriptedClock {
    fn advance(&self, ms: u64) {
        self.now_ms.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for ScriptedClock {
    fn unix_ms(&self) -> u64 {
        self.start_unix_ms
            .saturating_add(self.now_ms.load(Ordering::SeqCst))
    }
    fn monotonic(&self) -> Duration {
        Duration::from_millis(self.now_ms.load(Ordering::SeqCst))
    }
}

/// Replays one authored script. It chooses nothing about fallback, retry or the record.
struct Scripted<'c> {
    provenance: Provenance,
    capabilities: Capabilities,
    clock: &'c ScriptedClock,
    events: Vec<StreamEvent>,
    items: Vec<Item>,
    stop_reason: StopReason,
    first_event_after_ms: u64,
    ends_after_ms: u64,
    fail: Option<Failure>,
    fail_times: Option<usize>,
    usage: Option<Usage>,
    upstream_model: Option<Id>,
    calls: AtomicUsize,
}

impl Model for Scripted<'_> {
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
                .clone()
                .filter(|_| self.fail_times.is_none_or(|times| call < times));
            self.clock.advance(self.first_event_after_ms);
            if fail.is_none() {
                for event in &self.events {
                    sink.emit(event.clone()).await?;
                }
            }
            self.clock.advance(self.ends_after_ms);
            let mut observation = TurnObservation::new(self.provenance.clone());
            observation.usage.clone_from(&self.usage);
            observation.upstream_model.clone_from(&self.upstream_model);
            match fail {
                None => {
                    observation.final_usage = true;
                    Ok(TurnOutcome {
                        stop_reason: self.stop_reason.clone(),
                        items: self.items.clone(),
                        observation,
                    })
                }
                Some(failure) => {
                    let error = Error::new(
                        failure.code,
                        failure
                            .message
                            .unwrap_or_else(|| "scripted failure".to_owned()),
                    )
                    .with_dispatch(failure.dispatch)
                    .with_retriable(failure.retriable);
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

struct Fleet<'c>(BTreeMap<Id, Scripted<'c>>);

impl Models for Fleet<'_> {
    fn model(&self, serving_model_id: &Id) -> Option<&dyn Model> {
        self.0
            .get(serving_model_id)
            .map(|model| model as &dyn Model)
    }
}

fn exercise(input: &Value) -> Result<Observed, TargetError> {
    let request: CallInput = serde_json::from_value(input.clone()).map_err(unavailable)?;
    let mut facts = json!({
        "valid_program": false, "error_code": null, "delivered_text": null, "records": [],
        "shadow_results": [], "case_results": [], "spent_today": null
    });
    if let Err(error) = execute(&request, &mut facts) {
        facts["error_code"] = json!(error.code);
    }
    Ok(Observed {
        facts,
        view: "llm.calls.LastCall",
        event: "llm.calls.Observed",
        field: "valid_program",
    })
}

/// What one scripted attempt shows and returns when it answers.
fn answer(
    script: &Script,
    request: &TurnRequest,
) -> Result<(Vec<StreamEvent>, Vec<Item>, StopReason), Error> {
    let text = script.answer.clone().unwrap_or_else(|| "done".to_owned());
    let mut events = vec![StreamEvent::TextDelta { text: text.clone() }];
    let mut items = vec![Item::assistant(text)];
    let Some(arguments) = &script.tool_arguments else {
        return Ok((events, items, StopReason::EndTurn));
    };
    let tool = request
        .tools
        .first()
        .ok_or_else(|| Error::invalid("a scripted tool call needs a published tool"))?;
    let call = ToolCall {
        call_id: CallId::new("call-1").map_err(|_| Error::invalid("invalid call id"))?,
        name: tool.name.clone(),
        arguments: serde_json::from_str(arguments)
            .map_err(|_| Error::invalid("scripted tool arguments are not JSON"))?,
    };
    events.push(StreamEvent::ToolCallStarted {
        call_id: call.call_id.clone(),
        name: call.name.clone(),
    });
    events.push(StreamEvent::ToolArgumentsDelta {
        call_id: call.call_id.clone(),
        delta: arguments.clone(),
    });
    items.push(Item::ToolCall(call));
    Ok((events, items, StopReason::ToolCalls))
}

/// One scripted model per authored serving model, each bound to its declared binding.
fn scripted_fleet<'c>(
    models: BTreeMap<String, Script>,
    catalog: &Catalog,
    request: &TurnRequest,
    clock: &'c ScriptedClock,
) -> Result<Fleet<'c>, Error> {
    let mut fleet = BTreeMap::new();
    for (serving, script) in models {
        let serving = Id::new(&serving).map_err(|_| Error::invalid("invalid serving model"))?;
        let binding = catalog
            .binding(&serving)
            .ok_or_else(|| Error::invalid("scripted serving model is not declared"))?;
        let (events, items, stop_reason) = answer(&script, request)?;
        let upstream_model = script
            .upstream_model
            .map(Id::new)
            .transpose()
            .map_err(|_| Error::invalid("invalid upstream model"))?;
        fleet.insert(
            serving,
            Scripted {
                provenance: binding.provenance().clone(),
                capabilities: binding.capabilities().clone(),
                clock,
                events,
                items,
                stop_reason,
                first_event_after_ms: script.first_event_after_ms,
                ends_after_ms: script.ends_after_ms,
                fail: script.fail,
                fail_times: script.fail_times,
                usage: script.usage,
                upstream_model,
                calls: AtomicUsize::new(0),
            },
        );
    }
    Ok(Fleet(fleet))
}

fn execute(input: &CallInput, facts: &mut Value) -> Result<(), Error> {
    if input.program_json.len() > MAX_PROGRAM_BYTES {
        return Err(Error::too_large("call program exceeds its bound"));
    }
    let program: Program = serde_json::from_str(&input.program_json)
        .map_err(|_| Error::invalid("invalid call program"))?;
    facts["valid_program"] = json!(true);
    if input.shadows_json.is_some() {
        return Err(Error::unsupported("shadow routes are not implemented"));
    }
    let catalog = Catalog::parse(&input.catalog_toml)?;
    let document: TurnDocument = serde_json::from_str(&input.turn_json)
        .map_err(|_| Error::invalid("invalid turn envelope"))?;
    let prices = input
        .prices_toml
        .as_deref()
        .map(PriceBook::parse_toml)
        .transpose()
        .map_err(|_| Error::invalid("invalid price book"))?;
    let record_id = Id::new(&program.record_id).map_err(|_| Error::invalid("invalid record id"))?;
    let clock = ScriptedClock {
        start_unix_ms: program.start_unix_ms,
        now_ms: AtomicU64::new(0),
    };
    let models = scripted_fleet(program.models, &catalog, &document.request, &clock)?;
    let log = CallLog::new();
    let mut sink = VecSink::new(MAX_SINK_EVENTS, MAX_SINK_BYTES);
    let cancel = Cancel::new();
    let mut admit = |_: &Selection<'_>| Ok(());
    // Retry waits are not what this domain observes: the scripted clock is what moves.
    let pause = |_: Duration| -> BoxFuture<'static, ()> { Box::pin(async {}) };
    let run = tokio::runtime::Builder::new_current_thread()
        .build()
        .map_err(|_| Error::new(ErrorCode::Unavailable, "no local runtime"))?
        .block_on(record_turn(
            &catalog,
            &document.request,
            program.input_tokens,
            FallbackPolicy::default(),
            Ports {
                models: &models,
                admit: &mut admit,
                sink: &mut sink,
                cancel: &cancel,
                pause: &pause,
            },
            CallIdentity::primary(record_id),
            Recording {
                sink: &log,
                clock: &clock,
                prices: prices.as_ref(),
            },
        ));
    facts["records"] = json!(
        log.records()
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| Error::new(ErrorCode::Protocol, "a record did not serialize"))?
    );
    let run = run?;
    facts["delivered_text"] = json!(sink.text());
    if let Err(error) = &run.result {
        facts["error_code"] = json!(error.code);
    }
    Ok(())
}
