//! Closed-label classification through scripted `llm_core::Model`s routed by the example
//! catalog, a scripted clock and an in-memory call log: no projection, no network.
//!
//! The example route `code` tries `local-small` (no tools) then `remote-large` (tools), so a
//! JSON-schema classifier reaches `local-small` first and a forced-tool one only `remote-large`.

use llm_calls::{CallLog, CallRecord, Clock, Kind, Outcome, Recording};
use llm_core::{
    BoxFuture, CallId, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item, Model,
    Provenance, StopReason, StreamEvent, StreamSink, ToolCall, ToolChoice, TurnObservation,
    TurnOutcome, TurnRequest, Usage,
};
use llm_cost::{Amount, Currency, ModelPrice, PriceBook, PriceDocument, Rate};
use llm_decide::{
    AbstentionCause, Answer, Chain, ChainOutcome, Classification, Classifier, FORMAT_NAME,
    LabelConfidence, LabelSet, Lexical, LexicalClassifier, ModelClassifier, Ports, SetupError,
    TOOL_NAME,
};
use llm_routing::{Catalog, CatalogDocument, Models, Selection};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
    time::Duration,
};

const EXAMPLE: &str = include_str!("../../../examples/catalog.toml");
const INPUT: &str = "the film was wonderful";

fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
    }
}

fn catalog() -> Catalog {
    let mut doc = CatalogDocument::parse(EXAMPLE).unwrap();
    doc.routes[0].fallback_enabled = true;
    doc.validate().unwrap()
}

fn labels() -> LabelSet {
    LabelSet::new(
        id("sentiment"),
        "v1",
        vec!["positive".into(), "negative".into(), "neutral".into()],
    )
    .unwrap()
}

/// A clock that moves only when a scripted model or a pause says time passed.
struct Scripted {
    unix_ms: u64,
    now_ms: AtomicU64,
}

impl Scripted {
    fn at(unix_ms: u64) -> Arc<Self> {
        Arc::new(Self {
            unix_ms,
            now_ms: AtomicU64::new(0),
        })
    }
    fn advance(&self, ms: u64) {
        self.now_ms.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for Scripted {
    fn unix_ms(&self) -> u64 {
        self.unix_ms + self.now_ms.load(Ordering::SeqCst)
    }
    fn monotonic(&self) -> Duration {
        Duration::from_millis(self.now_ms.load(Ordering::SeqCst))
    }
}

#[derive(Clone)]
enum Reply {
    Text(String),
    Tool(Value),
    Fail(Error),
    /// Waits until the call is cancelled, then reports the cancellation.
    Hang,
}

/// Replays one authored reply and keeps every request it was sent.
struct Fake {
    provenance: Provenance,
    capabilities: Capabilities,
    clock: Arc<Scripted>,
    reply: Reply,
    takes_ms: u64,
    usage: Option<Usage>,
    /// Fails the first attempt with this error, then replies.
    fail_first: Option<Error>,
    seen: Mutex<Vec<TurnRequest>>,
    calls: AtomicUsize,
    cancelled: AtomicBool,
}

impl Fake {
    fn on(catalog: &Catalog, serving: &str, clock: &Arc<Scripted>, reply: Reply) -> Self {
        let binding = catalog.binding(&id(serving)).unwrap();
        Self {
            provenance: binding.provenance().clone(),
            capabilities: binding.capabilities().clone(),
            clock: clock.clone(),
            reply,
            takes_ms: 0,
            usage: None,
            fail_first: None,
            seen: Mutex::new(Vec::new()),
            calls: AtomicUsize::new(0),
            cancelled: AtomicBool::new(false),
        }
    }
    fn seen(&self) -> Vec<TurnRequest> {
        self.seen.lock().unwrap().clone()
    }
}

impl Model for Fake {
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
            self.seen.lock().unwrap().push(request.clone());
            let attempt = self.calls.fetch_add(1, Ordering::SeqCst);
            let mut observation = TurnObservation::new(self.provenance.clone());
            observation.usage.clone_from(&self.usage);
            if attempt == 0
                && let Some(error) = &self.fail_first
            {
                return Err(error.clone().with_observation(observation));
            }
            self.clock.advance(self.takes_ms);
            match &self.reply {
                Reply::Hang => {
                    cancel.cancelled().await;
                    self.cancelled.store(true, Ordering::SeqCst);
                    Err(Error::cancelled().with_dispatch(Dispatch::Unknown))
                }
                Reply::Fail(error) if error.dispatch == Dispatch::NotSent => Err(error.clone()),
                Reply::Fail(error) => Err(error.clone().with_observation(observation)),
                Reply::Text(text) => {
                    sink.emit(StreamEvent::TextDelta { text: text.clone() })
                        .await?;
                    observation.final_usage = true;
                    Ok(TurnOutcome {
                        stop_reason: StopReason::EndTurn,
                        items: vec![Item::assistant(text.clone())],
                        observation,
                    })
                }
                Reply::Tool(arguments) => {
                    let call = ToolCall {
                        call_id: CallId::new("call-1").unwrap(),
                        name: request.tools[0].name.clone(),
                        arguments: arguments.clone(),
                    };
                    sink.emit(StreamEvent::ToolCallStarted {
                        call_id: call.call_id.clone(),
                        name: call.name.clone(),
                    })
                    .await?;
                    observation.final_usage = true;
                    Ok(TurnOutcome {
                        stop_reason: StopReason::ToolCalls,
                        items: vec![Item::ToolCall(call)],
                        observation,
                    })
                }
            }
        })
    }
}

struct Fleet(BTreeMap<Id, Fake>);

impl Models for Fleet {
    fn model(&self, serving_model_id: &Id) -> Option<&dyn Model> {
        self.0
            .get(serving_model_id)
            .map(|model| model as &dyn Model)
    }
}

impl Fleet {
    fn of(models: impl IntoIterator<Item = (&'static str, Fake)>) -> Self {
        Self(
            models
                .into_iter()
                .map(|(serving, model)| (id(serving), model))
                .collect(),
        )
    }
    fn get(&self, serving: &str) -> &Fake {
        &self.0[&id(serving)]
    }
}

/// Labels "neutral" any input containing "fine", and knows only that label's confidence.
struct Words;

impl Lexical for Words {
    fn classify(&self, _: &LabelSet, input: &str) -> Option<Answer> {
        input.contains("fine").then(|| Answer {
            label: "neutral".into(),
            confidences_bps: BTreeMap::from([("neutral".to_owned(), 6000)]),
        })
    }
}

struct Ran {
    outcome: ChainOutcome,
    records: Vec<CallRecord>,
}

fn run(
    chain: &Chain,
    catalog: &Catalog,
    fleet: &Fleet,
    clock: &Arc<Scripted>,
    input: &str,
    prices: Option<&PriceBook>,
) -> Ran {
    let log = CallLog::new();
    let cancel = Cancel::new();
    let admit = |_: &Selection<'_>| Ok(());
    let paused = clock.clone();
    // A pause moves the scripted clock by its wait and returns at once.
    let pause = move |wait: Duration| -> BoxFuture<'static, ()> {
        let clock = paused.clone();
        Box::pin(async move { clock.advance(u64::try_from(wait.as_millis()).unwrap()) })
    };
    let outcome = block_on(chain.classify(
        &id("req-1"),
        input,
        Ports {
            catalog,
            models: fleet,
            admit: &admit,
            cancel: &cancel,
            pause: &pause,
            recording: Recording {
                sink: &log,
                clock: &**clock,
                prices,
            },
        },
    ));
    Ran {
        outcome,
        records: log.records(),
    }
}

fn chain(classifiers: Vec<Box<dyn Classifier>>) -> Chain {
    Chain::new(id("chain"), labels(), Duration::from_secs(30), classifiers).unwrap()
}

fn answer(label: &str, positive: u64, negative: u64, neutral: u64) -> Value {
    json!({
        "label": label,
        "confidences": {"positive": positive, "negative": negative, "neutral": neutral}
    })
}

fn confidences(pairs: &[(&str, Option<u16>)]) -> Vec<LabelConfidence> {
    pairs
        .iter()
        .map(|(label, confidence_bps)| LabelConfidence {
            label: (*label).to_owned(),
            confidence_bps: *confidence_bps,
        })
        .collect()
}

fn cause(classification: &Classification) -> Option<AbstentionCause> {
    match classification {
        Classification::Decided(_) => None,
        Classification::Abstained(abstention) => Some(abstention.cause),
    }
}

fn the_schema() -> Value {
    let bps = json!({"type": "integer", "minimum": 0, "maximum": 10000});
    json!({
        "type": "object",
        "properties": {
            "label": {"type": "string", "enum": ["positive", "negative", "neutral"]},
            "confidences": {
                "type": "object",
                "properties": {"positive": bps, "negative": bps, "neutral": bps},
                "required": ["positive", "negative", "neutral"],
                "additionalProperties": false
            }
        },
        "required": ["label", "confidences"],
        "additionalProperties": false
    })
}

#[test]
fn a_json_schema_classifier_returns_one_label_with_a_confidence_per_label() {
    let catalog = catalog();
    let clock = Scripted::at(1_000);
    let fleet = Fleet::of([(
        "local-small",
        Fake::on(
            &catalog,
            "local-small",
            &clock,
            Reply::Text(answer("positive", 9100, 300, 600).to_string()),
        ),
    )]);
    let chain = chain(vec![Box::new(
        ModelClassifier::json_schema(id("schema"), "code").with_instructions("Classify."),
    )]);
    let ran = run(&chain, &catalog, &fleet, &clock, INPUT, None);

    let decision = ran.outcome.decision().expect("the classifier decides");
    assert_eq!(decision.label, "positive");
    assert_eq!(
        decision.confidences,
        confidences(&[
            ("positive", Some(9100)),
            ("negative", Some(300)),
            ("neutral", Some(600))
        ])
    );
    assert_eq!(decision.classifier_id, id("schema"));
    assert_eq!(decision.label_set_version, "v1");
    assert_eq!(decision.call_record_id, Some(id("req-1/schema")));
    assert_eq!(ran.outcome.attempts.len(), 1);

    let seen = fleet.get("local-small").seen();
    assert_eq!(seen.len(), 1, "one call: {seen:?}");
    let format = seen[0]
        .controls
        .output_format
        .as_ref()
        .expect("the turn asks for structured output");
    assert_eq!(format.schema, the_schema());
    assert_eq!(format.strict, Some(true));
    assert_eq!(format.name.as_deref(), Some(FORMAT_NAME));
    assert!(seen[0].tools.is_empty());
    assert_eq!(seen[0].tool_choice, ToolChoice::Auto);
    assert_eq!(seen[0].instructions, "Classify.");
    assert_eq!(seen[0].items, vec![Item::user(INPUT)]);

    assert_eq!(ran.records.len(), 1, "{:?}", ran.records);
    assert_eq!(ran.records[0].kind, Kind::Classification);
    assert_eq!(ran.records[0].id, id("req-1/schema"));
    assert_eq!(ran.records[0].outcome, Outcome::Succeeded);
}

#[test]
fn a_forced_tool_classifier_returns_one_label_with_a_confidence_per_label() {
    let catalog = catalog();
    let clock = Scripted::at(1_000);
    let fleet = Fleet::of([
        (
            "local-small",
            Fake::on(&catalog, "local-small", &clock, Reply::Hang),
        ),
        (
            "remote-large",
            Fake::on(
                &catalog,
                "remote-large",
                &clock,
                Reply::Tool(answer("negative", 200, 9700, 100)),
            ),
        ),
    ]);
    let chain = chain(vec![Box::new(ModelClassifier::forced_tool(
        id("tool"),
        "code",
    ))]);
    let ran = run(&chain, &catalog, &fleet, &clock, INPUT, None);

    let decision = ran.outcome.decision().expect("the classifier decides");
    assert_eq!(decision.label, "negative");
    assert_eq!(
        decision.confidences,
        confidences(&[
            ("positive", Some(200)),
            ("negative", Some(9700)),
            ("neutral", Some(100))
        ])
    );
    assert_eq!(decision.call_record_id, Some(id("req-1/tool")));
    assert!(
        fleet.get("local-small").seen().is_empty(),
        "a target without tools is never asked"
    );
    let seen = fleet.get("remote-large").seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].tools.len(), 1);
    assert_eq!(seen[0].tools[0].name.as_str(), TOOL_NAME);
    assert_eq!(seen[0].tools[0].input_schema, the_schema());
    assert_eq!(
        seen[0].tool_choice,
        ToolChoice::Named(seen[0].tools[0].name.clone())
    );
    assert!(seen[0].controls.output_format.is_none());
    assert_eq!(ran.records.len(), 1);
    assert_eq!(ran.records[0].kind, Kind::Classification);
}

#[test]
fn an_answer_naming_no_label_of_the_set_abstains_off_label() {
    let catalog = catalog();
    for (serving, classifier, reply) in [
        (
            "local-small",
            ModelClassifier::json_schema(id("m"), "code"),
            Reply::Text(answer("furious", 9000, 500, 500).to_string()),
        ),
        (
            "remote-large",
            ModelClassifier::forced_tool(id("m"), "code"),
            Reply::Tool(answer("furious", 9000, 500, 500)),
        ),
    ] {
        let clock = Scripted::at(1_000);
        let fleet = Fleet::of([(serving, Fake::on(&catalog, serving, &clock, reply))]);
        let chain = chain(vec![Box::new(classifier)]);
        let ran = run(&chain, &catalog, &fleet, &clock, INPUT, None);
        assert!(ran.outcome.decision().is_none());
        let Some(Classification::Abstained(abstention)) = ran.outcome.result() else {
            panic!("an off-label answer abstains: {:?}", ran.outcome);
        };
        assert_eq!(abstention.cause, AbstentionCause::OffLabel);
        assert!(abstention.is_final);
        assert_eq!(abstention.call_record_id, Some(id("req-1/m")));
        assert_eq!(ran.records.len(), 1);
    }
}

#[test]
fn an_answer_that_does_not_decode_as_the_schema_abstains_off_label() {
    let catalog = catalog();
    for text in [
        "positive".to_owned(),
        String::new(),
        json!({"label": "positive"}).to_string(),
        json!({"label": "positive", "confidences": {"positive": 9000, "negative": 500}})
            .to_string(),
        json!({"label": "positive", "confidences":
            {"positive": 9000, "negative": 500, "neutral": 500, "other": 0}})
        .to_string(),
        json!({"label": "positive", "confidences":
            {"positive": 10001, "negative": 0, "neutral": 0}})
        .to_string(),
        json!({"label": "positive", "confidences":
            {"positive": -1, "negative": 0, "neutral": 0}})
        .to_string(),
        json!({"label": "positive", "confidences":
            {"positive": 90.5, "negative": 0, "neutral": 0}})
        .to_string(),
        json!({"label": "positive", "confidences":
            {"positive": 9000, "negative": 500, "neutral": 500}, "why": "x"})
        .to_string(),
        json!({"label": 1, "confidences":
            {"positive": 9000, "negative": 500, "neutral": 500}})
        .to_string(),
        json!(["positive"]).to_string(),
    ] {
        let clock = Scripted::at(1_000);
        let fleet = Fleet::of([(
            "local-small",
            Fake::on(&catalog, "local-small", &clock, Reply::Text(text.clone())),
        )]);
        let chain = chain(vec![Box::new(ModelClassifier::json_schema(
            id("m"),
            "code",
        ))]);
        let ran = run(&chain, &catalog, &fleet, &clock, INPUT, None);
        assert_eq!(
            ran.outcome.result().and_then(cause),
            Some(AbstentionCause::OffLabel),
            "{text}: {:?}",
            ran.outcome
        );
    }
}

#[test]
fn a_classifier_past_the_chain_deadline_abstains_with_timeout() {
    let catalog = catalog();
    // A call still open at the deadline is cancelled and its record written; the next model
    // classifier starts past the deadline, so it abstains without a call.
    let clock = Scripted::at(1_000);
    let fleet = Fleet::of([
        (
            "local-small",
            Fake::on(&catalog, "local-small", &clock, Reply::Hang),
        ),
        (
            "remote-large",
            Fake::on(
                &catalog,
                "remote-large",
                &clock,
                Reply::Tool(answer("positive", 9000, 500, 500)),
            ),
        ),
    ]);
    let chain = Chain::new(
        id("chain"),
        labels(),
        Duration::from_millis(2_000),
        vec![
            Box::new(ModelClassifier::json_schema(id("schema"), "code")),
            Box::new(ModelClassifier::forced_tool(id("tool"), "code")),
        ],
    )
    .unwrap();
    let ran = run(&chain, &catalog, &fleet, &clock, INPUT, None);
    assert_eq!(ran.outcome.attempts.len(), 2, "{:?}", ran.outcome);
    let Classification::Abstained(first) = &ran.outcome.attempts[0] else {
        panic!("the open call times out: {:?}", ran.outcome);
    };
    assert_eq!(first.cause, AbstentionCause::Timeout);
    assert!(!first.is_final);
    assert_eq!(first.latency_ms, 2_000);
    assert_eq!(first.call_record_id, Some(id("req-1/schema")));
    assert!(
        fleet.get("local-small").cancelled.load(Ordering::SeqCst),
        "the call is cancelled"
    );
    let Classification::Abstained(second) = &ran.outcome.attempts[1] else {
        panic!("past the deadline nothing decides: {:?}", ran.outcome);
    };
    assert_eq!(second.cause, AbstentionCause::Timeout);
    assert!(second.is_final);
    assert_eq!(second.call_record_id, None);
    assert!(fleet.get("remote-large").seen().is_empty());
    assert_eq!(ran.records.len(), 1, "{:?}", ran.records);
    assert_eq!(ran.records[0].outcome, Outcome::Cancelled);

    // An answer that arrives at or after the deadline is not used.
    let clock = Scripted::at(1_000);
    let mut late = Fake::on(
        &catalog,
        "local-small",
        &clock,
        Reply::Text(answer("positive", 9000, 500, 500).to_string()),
    );
    late.takes_ms = 2_000;
    let fleet = Fleet::of([("local-small", late)]);
    let chain = Chain::new(
        id("chain"),
        labels(),
        Duration::from_millis(2_000),
        vec![Box::new(ModelClassifier::json_schema(id("schema"), "code"))],
    )
    .unwrap();
    let ran = run(&chain, &catalog, &fleet, &clock, INPUT, None);
    assert!(ran.outcome.decision().is_none(), "{:?}", ran.outcome);
    assert_eq!(
        ran.outcome.result().and_then(cause),
        Some(AbstentionCause::Timeout)
    );
    assert_eq!(ran.records.len(), 1);
}

#[test]
fn a_provider_failure_abstains_with_its_error_code() {
    let catalog = catalog();
    let clock = Scripted::at(1_000);
    let fleet = Fleet::of([(
        "local-small",
        Fake::on(
            &catalog,
            "local-small",
            &clock,
            Reply::Fail(
                Error::new(ErrorCode::Unauthorized, "scripted").with_dispatch(Dispatch::Rejected),
            ),
        ),
    )]);
    let chain = chain(vec![Box::new(ModelClassifier::json_schema(
        id("schema"),
        "code",
    ))]);
    let ran = run(&chain, &catalog, &fleet, &clock, INPUT, None);
    let Some(Classification::Abstained(abstention)) = ran.outcome.result() else {
        panic!("a failed call abstains: {:?}", ran.outcome);
    };
    assert_eq!(
        abstention.cause,
        AbstentionCause::ProviderError(ErrorCode::Unauthorized)
    );
    assert_eq!(abstention.cause.error_code(), Some(ErrorCode::Unauthorized));
    assert!(abstention.is_final);
    assert_eq!(abstention.call_record_id, Some(id("req-1/schema")));
    assert_eq!(ran.records.len(), 1);
    assert_eq!(ran.records[0].error_code, Some(ErrorCode::Unauthorized));
}

#[test]
fn a_confidence_below_the_classifier_floor_abstains_off_label() {
    let catalog = catalog();
    for (positive, decides) in [(7999, false), (8000, true)] {
        let clock = Scripted::at(1_000);
        let fleet = Fleet::of([(
            "local-small",
            Fake::on(
                &catalog,
                "local-small",
                &clock,
                Reply::Text(answer("positive", positive, 1000, 1001).to_string()),
            ),
        )]);
        let chain = chain(vec![Box::new(
            ModelClassifier::json_schema(id("schema"), "code")
                .with_min_confidence_bps(8000)
                .unwrap(),
        )]);
        let ran = run(&chain, &catalog, &fleet, &clock, INPUT, None);
        if decides {
            assert_eq!(ran.outcome.decision().map(|d| d.label.as_str()), Some("positive"));
        } else {
            assert_eq!(
                ran.outcome.result().and_then(cause),
                Some(AbstentionCause::OffLabel),
                "{:?}",
                ran.outcome
            );
        }
    }
    // A lexical answer that leaves the label's confidence unknown cannot meet a floor.
    let clock = Scripted::at(1_000);
    let chain = chain(vec![Box::new(
        LexicalClassifier::new(id("words"), WordsUnknown)
            .with_min_confidence_bps(1)
            .unwrap(),
    )]);
    let ran = run(&chain, &catalog, &Fleet::of([]), &clock, "fine", None);
    assert_eq!(
        ran.outcome.result().and_then(cause),
        Some(AbstentionCause::OffLabel)
    );
    assert_eq!(
        ModelClassifier::json_schema(id("schema"), "code")
            .with_min_confidence_bps(10_001)
            .err(),
        Some(SetupError::FloorAboveMax(10_001))
    );
}

/// Labels "neutral" and knows no confidence.
struct WordsUnknown;

impl Lexical for WordsUnknown {
    fn classify(&self, _: &LabelSet, _: &str) -> Option<Answer> {
        Some(Answer {
            label: "neutral".into(),
            confidences_bps: BTreeMap::new(),
        })
    }
}

#[test]
fn the_chain_falls_back_in_order_and_ends_in_the_caller_lexical_classifier() {
    let catalog = catalog();
    let clock = Scripted::at(1_000);
    let fleet = Fleet::of([
        (
            "local-small",
            Fake::on(
                &catalog,
                "local-small",
                &clock,
                Reply::Text(answer("furious", 9000, 500, 500).to_string()),
            ),
        ),
        (
            "remote-large",
            Fake::on(
                &catalog,
                "remote-large",
                &clock,
                Reply::Fail(
                    Error::new(ErrorCode::Unavailable, "scripted")
                        .with_dispatch(Dispatch::Rejected),
                ),
            ),
        ),
    ]);
    let chain = chain(vec![
        Box::new(ModelClassifier::json_schema(id("schema"), "code")),
        Box::new(ModelClassifier::forced_tool(id("tool"), "code")),
        Box::new(LexicalClassifier::new(id("words"), Words)),
    ]);
    let ran = run(&chain, &catalog, &fleet, &clock, "it was fine", None);
    let ids: Vec<&str> = ran
        .outcome
        .attempts
        .iter()
        .map(|attempt| attempt.classifier_id().as_str())
        .collect();
    assert_eq!(ids, ["schema", "tool", "words"]);
    assert_eq!(
        ran.outcome.attempts[0..2]
            .iter()
            .map(cause)
            .collect::<Vec<_>>(),
        [
            Some(AbstentionCause::OffLabel),
            Some(AbstentionCause::ProviderError(ErrorCode::Unavailable))
        ]
    );
    assert!(
        ran.outcome.attempts[0..2]
            .iter()
            .all(|attempt| matches!(attempt, Classification::Abstained(a) if !a.is_final))
    );
    let decision = ran.outcome.decision().expect("the lexical classifier decides");
    assert_eq!(decision.classifier_id, id("words"));
    assert_eq!(decision.label, "neutral");
    assert_eq!(
        decision.confidences,
        confidences(&[("positive", None), ("negative", None), ("neutral", Some(6000))])
    );
    assert_eq!(decision.call_record_id, None);
    assert_eq!(
        ran.records
            .iter()
            .map(|record| (record.id.as_str(), record.kind))
            .collect::<Vec<_>>(),
        [
            ("req-1/schema", Kind::Classification),
            ("req-1/tool", Kind::Classification)
        ],
        "a lexical classifier makes no call and writes no record"
    );
}

#[test]
fn a_chain_where_every_classifier_abstains_returns_the_last_abstention_as_final() {
    let catalog = catalog();
    let clock = Scripted::at(1_000);
    let fleet = Fleet::of([(
        "local-small",
        Fake::on(
            &catalog,
            "local-small",
            &clock,
            Reply::Text(answer("furious", 9000, 500, 500).to_string()),
        ),
    )]);
    let chain = chain(vec![
        Box::new(ModelClassifier::json_schema(id("schema"), "code")),
        Box::new(LexicalClassifier::new(id("words"), Words)),
    ]);
    let ran = run(&chain, &catalog, &fleet, &clock, "it was awful", None);
    assert!(ran.outcome.decision().is_none());
    let finals: Vec<bool> = ran
        .outcome
        .attempts
        .iter()
        .map(|attempt| matches!(attempt, Classification::Abstained(a) if a.is_final))
        .collect();
    assert_eq!(finals, [false, true]);
    let Some(Classification::Abstained(last)) = ran.outcome.result() else {
        panic!("every classifier abstained: {:?}", ran.outcome);
    };
    assert_eq!(last.classifier_id, id("words"));
    assert_eq!(last.cause, AbstentionCause::OffLabel);
}

/// One per thousand input tokens and two per thousand output tokens, in USD, for `local-small`.
fn prices(catalog: &Catalog) -> PriceBook {
    let binding = catalog.binding(&id("local-small")).unwrap();
    let rate = |amount: &str| {
        Some(Rate {
            amount: Amount::parse(amount).unwrap(),
            per_units: 1000,
        })
    };
    PriceBook::from_document(PriceDocument::new(
        id("prices-r1"),
        "fixture price list",
        1,
        Currency::new("USD").unwrap(),
        vec![ModelPrice {
            id: id("price-1"),
            serving_model_id: id("local-small"),
            binding_revision: binding.provenance().binding_revision.clone(),
            upstream_model: id("example/Small-Model"),
            input: rate("1"),
            cached_input: None,
            cache_creation_input: None,
            output: rate("2"),
        }],
    ))
    .unwrap()
}

#[test]
fn each_model_classifier_call_yields_a_call_record_with_latency_and_cost() {
    let catalog = catalog();
    let prices = prices(&catalog);
    let usage = Usage {
        input_tokens: Some(100),
        output_tokens: Some(20),
        cached_input_tokens: Some(0),
        cache_creation_input_tokens: Some(0),
        reasoning_output_tokens: None,
    };
    let answering = |clock: &Arc<Scripted>| {
        let mut fake = Fake::on(
            &catalog,
            "local-small",
            clock,
            Reply::Text(answer("positive", 9000, 500, 500).to_string()),
        );
        fake.takes_ms = 40;
        fake.usage = Some(usage.clone());
        fake
    };

    // One attempt: latency from the scripted clock, cost fully priced.
    let clock = Scripted::at(1_000);
    let fleet = Fleet::of([("local-small", answering(&clock))]);
    let chain = chain(vec![Box::new(ModelClassifier::json_schema(
        id("schema"),
        "code",
    ))]);
    let ran = run(&chain, &catalog, &fleet, &clock, INPUT, Some(&prices));
    let decision = ran.outcome.decision().expect("decides");
    assert_eq!(ran.records.len(), 1);
    let record = &ran.records[0];
    assert_eq!(record.kind, Kind::Classification);
    assert_eq!(Some(&record.id), decision.call_record_id.as_ref());
    assert_eq!(record.latency_ms, Some(40));
    assert_eq!(decision.latency_ms, 40);
    let cost = record.cost.as_ref().expect("a priced call has a cost");
    assert!(cost.complete, "{cost:?}");
    assert_eq!(cost.known_subtotal, Amount::parse("0.14").unwrap());

    // An accepted retriable failure is retried: the record counts and prices both attempts, so
    // the first attempt's incomplete usage leaves the call's cost incomplete.
    let clock = Scripted::at(1_000);
    let mut retried = answering(&clock);
    retried.fail_first = Some(
        Error::new(ErrorCode::RateLimited, "scripted")
            .with_dispatch(Dispatch::Accepted)
            .with_retriable(true),
    );
    let fleet = Fleet::of([("local-small", retried)]);
    let ran = run(&chain, &catalog, &fleet, &clock, INPUT, Some(&prices));
    let decision = ran.outcome.decision().expect("the retry decides");
    assert_eq!(ran.records.len(), 1);
    let record = &ran.records[0];
    assert_eq!(record.attempts, 2);
    assert_eq!(record.latency_ms, Some(decision.latency_ms));
    assert!(decision.latency_ms > 40, "the retry wait is part of the call");
    let cost = record.cost.as_ref().expect("a priced call has a cost");
    assert!(!cost.complete, "{cost:?}");
}

#[test]
fn a_label_set_with_a_duplicate_or_a_single_label_is_refused() {
    let set = |labels: &[&str]| {
        LabelSet::new(
            id("set"),
            "v1",
            labels.iter().map(|label| (*label).to_owned()).collect(),
        )
    };
    assert_eq!(
        set(&["yes", "no", "yes"]).err(),
        Some(SetupError::DuplicateLabel("yes".to_owned()))
    );
    assert_eq!(set(&["yes"]).err(), Some(SetupError::TooFewLabels));
    assert_eq!(set(&[]).err(), Some(SetupError::TooFewLabels));
    assert_eq!(set(&["yes", ""]).err(), Some(SetupError::EmptyLabel));
    assert!(set(&["yes", "no"]).is_ok());
    assert_eq!(
        LabelSet::new(id("set"), "", vec!["a".into(), "b".into()]).err(),
        Some(SetupError::EmptyVersion)
    );
    let decoded: Result<LabelSet, _> =
        serde_json::from_str(r#"{"id":"set","version":"v1","labels":["yes","yes"]}"#);
    assert!(decoded.is_err(), "a decoded label set is held to the same rules");

    assert_eq!(
        Chain::new(id("chain"), labels(), Duration::from_secs(1), Vec::new()).err(),
        Some(SetupError::NoClassifier)
    );
    assert_eq!(
        Chain::new(
            id("chain"),
            labels(),
            Duration::from_secs(1),
            vec![
                Box::new(LexicalClassifier::new(id("w"), Words)),
                Box::new(LexicalClassifier::new(id("w"), Words)),
            ],
        )
        .err(),
        Some(SetupError::DuplicateClassifier(id("w")))
    );
    assert_eq!(
        Chain::new(
            id("chain"),
            labels(),
            Duration::ZERO,
            vec![Box::new(LexicalClassifier::new(id("w"), Words))],
        )
        .err(),
        Some(SetupError::ZeroDeadline)
    );
}
