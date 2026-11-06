//! Closed-label classification conformance observations.
//!
//! This module drives the real `llm_decide::Chain` over the real routed, recorded call
//! (`llm_calls::record_turn` over `Catalog::run_turn`), with in-process scripted `Model`s, a
//! scripted clock that a pause moves, the caller's price book and an in-memory record sink, and
//! reports every attempt and every record. The scripted models only replay the authored answer,
//! failure, delay or hang; the scripted lexical classifiers only return the authored answer.
//! Every decision, abstention, cause and record comes back out of the library. It reads no suite
//! and branches on no scenario name.

use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use ess_conformance::target::TargetError;
use llm_calls::{CallLog, Clock, Recording};
use llm_core::{
    BoxFuture, CallId, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item, Model,
    Provenance, StopReason, StreamEvent, StreamSink, ToolCall, TurnObservation, TurnOutcome,
    TurnRequest, Usage,
};
use llm_cost::PriceBook;
use llm_decide::{
    Answer, Chain, Classification, Classifier, LabelSet, Lexical, LexicalClassifier,
    ModelClassifier, Path, Ports, SetupError,
};
use llm_routing::{Catalog, Models, Selection};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::target::Observed;

/// Every view name this domain answers through `query_view`.
pub const VIEWS: &[&str] = &[
    "llm.decisions.LastClassification",
    "llm.decisions.LastVerdict",
];

const MAX_PROGRAM_BYTES: usize = 64 * 1024;

/// Observe one command of this domain, or `None` when the command belongs to another.
pub fn observe(command: &str, input: &Value) -> Option<Result<Observed, TargetError>> {
    match command {
        "llm.decisions.Classify" => Some(classify(input)),
        "llm.decisions.Judge" => Some(judge(input)),
        _ => None,
    }
}

fn unavailable(error: impl std::fmt::Display) -> TargetError {
    TargetError::unavailable("decision observation", error.to_string())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[expect(
    dead_code,
    reason = "read only to refuse an input that is not the command's"
)]
struct JudgeInput {
    catalog_toml: String,
    rubric_json: String,
    subject_text: String,
    program_json: String,
}

/// No library judges against a rubric yet: the command is answered, accepts no program and says
/// so.
fn judge(input: &Value) -> Result<Observed, TargetError> {
    let _: JudgeInput = serde_json::from_value(input.clone()).map_err(unavailable)?;
    Ok(Observed {
        facts: json!({
            "valid_program": false, "error_code": json!(ErrorCode::Unsupported),
            "rubric_version": null, "rubric_digest": null, "upstream_model": null,
            "verdict_json": null, "request_digest": null
        }),
        view: "llm.decisions.LastVerdict",
        event: "llm.decisions.Judged",
        field: "valid_program",
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassifyInput {
    catalog_toml: String,
    label_set_json: String,
    chain_json: String,
    input_text: String,
    #[serde(default)]
    prices_toml: Option<String>,
    program_json: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LabelSetInput {
    id: String,
    version: String,
    labels: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChainInput {
    id: String,
    deadline_ms: u64,
    classifiers: Vec<ClassifierInput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassifierInput {
    id: String,
    path: Path,
    #[serde(default)]
    route_alias: Option<String>,
    #[serde(default)]
    min_confidence_bps: Option<u16>,
    #[serde(default)]
    instructions: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Program {
    request_id: String,
    /// The caller's input-token estimate, which routing admits each target against.
    #[serde(default)]
    input_tokens: Option<u64>,
    /// The wall clock when the chain starts; the monotonic clock starts at zero.
    #[serde(default)]
    start_unix_ms: u64,
    #[serde(default)]
    models: BTreeMap<String, Script>,
    /// Per lexical classifier id, its answer; absent or null abstains.
    #[serde(default)]
    lexical: BTreeMap<String, Option<Answer>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Script {
    /// Text shown as one delta and returned as the assistant's message.
    #[serde(default)]
    answer: Option<String>,
    /// Arguments, as JSON text, of a call to the turn's first published tool.
    #[serde(default)]
    tool_arguments: Option<String>,
    /// Milliseconds that pass on the scripted clock before the attempt returns.
    #[serde(default)]
    takes_ms: u64,
    /// Waits until the call is cancelled, then reports the cancellation.
    #[serde(default)]
    hang: bool,
    #[serde(default)]
    fail: Option<Failure>,
    /// Fails only the first n attempts, then answers; every attempt when absent.
    #[serde(default)]
    fail_times: Option<usize>,
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
}

/// A clock that moves only when a scripted model or a pause says time passed.
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

/// Replays one authored script. It chooses nothing about routing, the deadline or the record.
struct Scripted {
    provenance: Provenance,
    capabilities: Capabilities,
    clock: Arc<ScriptedClock>,
    script: Script,
    upstream_model: Option<Id>,
    calls: AtomicUsize,
}

impl Scripted {
    fn answer(&self, request: &TurnRequest) -> Result<(Vec<StreamEvent>, TurnOutcome), Error> {
        let mut observation = TurnObservation::new(self.provenance.clone());
        observation.usage.clone_from(&self.script.usage);
        observation.upstream_model.clone_from(&self.upstream_model);
        observation.final_usage = true;
        let Some(arguments) = &self.script.tool_arguments else {
            let text = self.script.answer.clone().unwrap_or_default();
            let events = vec![StreamEvent::TextDelta { text: text.clone() }];
            let outcome = TurnOutcome {
                stop_reason: StopReason::EndTurn,
                items: vec![Item::assistant(text)],
                observation,
            };
            return Ok((events, outcome));
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
        let events = vec![
            StreamEvent::ToolCallStarted {
                call_id: call.call_id.clone(),
                name: call.name.clone(),
            },
            StreamEvent::ToolArgumentsDelta {
                call_id: call.call_id.clone(),
                delta: arguments.clone(),
            },
        ];
        let outcome = TurnOutcome {
            stop_reason: StopReason::ToolCalls,
            items: vec![Item::ToolCall(call)],
            observation,
        };
        Ok((events, outcome))
    }
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
        request: &'a TurnRequest,
        sink: &'a mut dyn StreamSink,
        cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        Box::pin(async move {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            let fail = self
                .script
                .fail
                .clone()
                .filter(|_| self.script.fail_times.is_none_or(|times| call < times));
            if self.script.hang && fail.is_none() {
                cancel.cancelled().await;
                return Err(Error::cancelled().with_dispatch(Dispatch::Unknown));
            }
            self.clock.advance(self.script.takes_ms);
            if let Some(failure) = fail {
                let error = Error::new(failure.code, "scripted failure")
                    .with_dispatch(failure.dispatch)
                    .with_retriable(failure.retriable);
                if failure.dispatch == Dispatch::NotSent {
                    return Err(error);
                }
                let mut observation = TurnObservation::new(self.provenance.clone());
                observation.usage.clone_from(&self.script.usage);
                observation.upstream_model.clone_from(&self.upstream_model);
                return Err(error.with_observation(observation));
            }
            let (events, outcome) = self.answer(request)?;
            for event in events {
                sink.emit(event).await?;
            }
            Ok(outcome)
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

/// Returns the authored answer, or abstains when there is none.
struct ScriptedLexical(Option<Answer>);

impl Lexical for ScriptedLexical {
    fn classify(&self, _: &LabelSet, _: &str) -> Option<Answer> {
        self.0.clone()
    }
}

fn scripted_fleet(
    models: BTreeMap<String, Script>,
    catalog: &Catalog,
    clock: &Arc<ScriptedClock>,
) -> Result<Fleet, Error> {
    let mut fleet = BTreeMap::new();
    for (serving, script) in models {
        let serving = Id::new(&serving).map_err(|_| Error::invalid("invalid serving model"))?;
        let binding = catalog
            .binding(&serving)
            .ok_or_else(|| Error::invalid("scripted serving model is not declared"))?;
        let upstream_model = script
            .upstream_model
            .clone()
            .map(Id::new)
            .transpose()
            .map_err(|_| Error::invalid("invalid upstream model"))?;
        fleet.insert(
            serving,
            Scripted {
                provenance: binding.provenance().clone(),
                capabilities: binding.capabilities().clone(),
                clock: clock.clone(),
                script,
                upstream_model,
                calls: AtomicUsize::new(0),
            },
        );
    }
    Ok(Fleet(fleet))
}

/// Why the chain could not be set up: an llm error code, or a setup refusal's own code.
enum Refusal {
    Llm(Error),
    Setup(SetupError),
}

impl From<Error> for Refusal {
    fn from(error: Error) -> Self {
        Self::Llm(error)
    }
}

impl From<SetupError> for Refusal {
    fn from(error: SetupError) -> Self {
        Self::Setup(error)
    }
}

fn invalid(what: &'static str) -> impl Fn(serde_json::Error) -> Refusal {
    move |_| Refusal::Llm(Error::invalid(what))
}

fn identifier(value: &str) -> Result<Id, Refusal> {
    Id::new(value).map_err(|_| Refusal::Llm(Error::invalid("invalid identifier")))
}

fn classifier(
    input: ClassifierInput,
    lexical: &mut BTreeMap<String, Option<Answer>>,
) -> Result<Box<dyn Classifier>, Refusal> {
    let ClassifierInput {
        id: name,
        path,
        route_alias,
        min_confidence_bps,
        instructions,
    } = input;
    let id = identifier(&name)?;
    if path == Path::Lexical {
        let answer = lexical.remove(&name).flatten();
        let classifier = LexicalClassifier::new(id, ScriptedLexical(answer));
        return Ok(Box::new(match min_confidence_bps {
            Some(bps) => classifier.with_min_confidence_bps(bps)?,
            None => classifier,
        }));
    }
    let route = route_alias
        .ok_or_else(|| Refusal::Llm(Error::invalid("a model classifier names a route")))?;
    let classifier = if path == Path::ForcedTool {
        ModelClassifier::forced_tool(id, route)
    } else {
        ModelClassifier::json_schema(id, route)
    }
    .with_instructions(instructions.unwrap_or_default());
    Ok(Box::new(match min_confidence_bps {
        Some(bps) => classifier.with_min_confidence_bps(bps)?,
        None => classifier,
    }))
}

fn classify(input: &Value) -> Result<Observed, TargetError> {
    let request: ClassifyInput = serde_json::from_value(input.clone()).map_err(unavailable)?;
    let mut facts = json!({
        "valid_program": false, "error_code": null, "tried": [], "results": [], "label": null,
        "confidences": [], "final_cause": null, "call_record_ids": [], "records": []
    });
    if let Err(refusal) = execute(&request, &mut facts) {
        facts["error_code"] = match refusal {
            Refusal::Llm(error) => json!(error.code),
            Refusal::Setup(error) => json!(error.code()),
        };
    }
    Ok(Observed {
        facts,
        view: "llm.decisions.LastClassification",
        event: "llm.decisions.Classified",
        field: "valid_program",
    })
}

fn result(attempt: &Classification) -> String {
    match attempt {
        Classification::Decided(decision) => format!("{}:decided", decision.classifier_id),
        Classification::Abstained(abstention) => {
            let mut result = format!(
                "{}:abstained:{}",
                abstention.classifier_id,
                abstention.cause.name()
            );
            if let Some(code) = abstention.cause.error_code() {
                result.push(':');
                result.push_str(json!(code).as_str().unwrap_or_default());
            }
            result
        }
    }
}

fn execute(input: &ClassifyInput, facts: &mut Value) -> Result<(), Refusal> {
    if input.program_json.len() > MAX_PROGRAM_BYTES {
        return Err(Error::too_large("classification program exceeds its bound").into());
    }
    let mut program: Program =
        serde_json::from_str(&input.program_json).map_err(invalid("invalid program"))?;
    facts["valid_program"] = json!(true);
    let catalog = Catalog::parse(&input.catalog_toml)?;
    let prices = input
        .prices_toml
        .as_deref()
        .map(PriceBook::parse_toml)
        .transpose()
        .map_err(|_| Error::invalid("invalid price book"))?;
    let set: LabelSetInput =
        serde_json::from_str(&input.label_set_json).map_err(invalid("invalid label set"))?;
    let label_set = LabelSet::new(identifier(&set.id)?, set.version, set.labels)?;
    let chain: ChainInput =
        serde_json::from_str(&input.chain_json).map_err(invalid("invalid chain"))?;
    let classifiers = chain
        .classifiers
        .into_iter()
        .map(|each| classifier(each, &mut program.lexical))
        .collect::<Result<Vec<_>, _>>()?;
    let chain = Chain::new(
        identifier(&chain.id)?,
        label_set,
        Duration::from_millis(chain.deadline_ms),
        classifiers,
    )?;
    let request_id = identifier(&program.request_id)?;
    let clock = Arc::new(ScriptedClock {
        start_unix_ms: program.start_unix_ms,
        now_ms: AtomicU64::new(0),
    });
    let models = scripted_fleet(program.models, &catalog, &clock)?;
    let log = CallLog::new();
    let cancel = Cancel::new();
    let admit = |_: &Selection<'_>| Ok(());
    let moved_by_waits = clock.clone();
    // A wait, the chain's deadline timer or a retry wait, moves the scripted clock and returns.
    let pause = move |wait: Duration| -> BoxFuture<'static, ()> {
        let clock = moved_by_waits.clone();
        Box::pin(async move { clock.advance(u64::try_from(wait.as_millis()).unwrap_or(u64::MAX)) })
    };
    let outcome = tokio::runtime::Builder::new_current_thread()
        .build()
        .map_err(|_| Error::new(ErrorCode::Unavailable, "no local runtime"))?
        .block_on(chain.classify(
            &request_id,
            &input.input_text,
            program.input_tokens,
            Ports {
                catalog: &catalog,
                models: &models,
                admit: &admit,
                cancel: &cancel,
                pause: &pause,
                recording: Recording {
                    sink: &log,
                    clock: &*clock,
                    prices: prices.as_ref(),
                },
            },
        ));
    facts["tried"] = json!(
        outcome
            .attempts
            .iter()
            .map(|attempt| attempt.classifier_id().as_str())
            .collect::<Vec<_>>()
    );
    facts["results"] = json!(outcome.attempts.iter().map(result).collect::<Vec<_>>());
    facts["call_record_ids"] = json!(
        outcome
            .attempts
            .iter()
            .filter_map(|attempt| attempt.call_record_id().map(Id::as_str))
            .collect::<Vec<_>>()
    );
    match outcome.result() {
        Some(Classification::Decided(decision)) => {
            facts["label"] = json!(decision.label);
            facts["confidences"] = json!(decision.confidences);
        }
        Some(Classification::Abstained(abstention)) => {
            facts["final_cause"] = json!(abstention.cause.name());
            facts["error_code"] = json!(abstention.cause.error_code());
        }
        None => {}
    }
    facts["records"] = json!(
        log.records()
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| Error::new(ErrorCode::Protocol, "a record did not serialize"))?
    );
    Ok(())
}
