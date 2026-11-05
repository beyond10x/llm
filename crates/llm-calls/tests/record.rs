//! Routed turns through scripted `llm_core::Model`s, a scripted clock and an in-memory record
//! sink: no projection, no network.

use llm_calls::{
    CallIdentity, CallLog, CallRecord, Clock, Kind, Outcome, Recording, Role, record_turn,
};
use llm_core::{
    BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item, Model, Provenance,
    StopReason, StreamEvent, StreamSink, ToolCall, ToolName, ToolSpec, TurnObservation,
    TurnOutcome, TurnRequest, Usage, VecSink,
};
use llm_cost::{Amount, Currency, ModelPrice, PriceBook, PriceDocument, Rate};
use llm_routing::{Catalog, CatalogDocument, FallbackPolicy, FallbackRun, Models, Ports};
use std::{
    collections::BTreeMap,
    future::Future,
    sync::atomic::{AtomicU64, AtomicUsize, Ordering},
    task::{Context, Poll, Waker},
    time::Duration,
};

const EXAMPLE: &str = include_str!("../../../examples/catalog.toml");

const INSTRUCTIONS: &str = "instructions-marker-heron-5521";
const PROMPT: &str = "prompt-marker-quokka-7781";
const TOOL_DESCRIPTION: &str = "tool-description-marker-ibis-3307";
const SCHEMA_TEXT: &str = "schema-marker-tapir-9183";
const ANSWER: &str = "answer-marker-okapi-4412";
const REASONING: &str = "reasoning-marker-lemur-6650";
const ARGUMENTS: &str = "argument-marker-gecko-2096";
const ERROR_PROSE: &str = "error-prose-marker-civet-8834";

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

/// `coding-primary` (`local-small`, self-hosted, no tools) then `coding-secondary`
/// (`remote-large`, metered, tools).
fn catalog(fallback_enabled: bool) -> Catalog {
    let mut doc = CatalogDocument::parse(EXAMPLE).unwrap();
    doc.routes[0].fallback_enabled = fallback_enabled;
    doc.validate().unwrap()
}

/// A clock that moves only when a scripted model says time passed.
struct Scripted {
    unix_ms: u64,
    now_ms: AtomicU64,
}

impl Scripted {
    const fn at(unix_ms: u64) -> Self {
        Self {
            unix_ms,
            now_ms: AtomicU64::new(0),
        }
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

/// Replays one authored script: time passes, events are shown, then the outcome or failure.
struct Fake<'c> {
    provenance: Provenance,
    capabilities: Capabilities,
    clock: &'c Scripted,
    before_first_ms: u64,
    events: Vec<StreamEvent>,
    after_ms: u64,
    items: Vec<Item>,
    stop_reason: StopReason,
    usage: Option<Usage>,
    upstream_model: Option<Id>,
    fail: Option<Error>,
    /// Fails only the first n attempts; every attempt when absent.
    fail_times: Option<usize>,
    calls: AtomicUsize,
}

impl<'c> Fake<'c> {
    fn on(catalog: &Catalog, serving: &str, clock: &'c Scripted) -> Self {
        let binding = catalog.binding(&id(serving)).unwrap();
        Self {
            provenance: binding.provenance().clone(),
            capabilities: binding.capabilities().clone(),
            clock,
            before_first_ms: 0,
            events: vec![StreamEvent::TextDelta {
                text: "done".to_owned(),
            }],
            after_ms: 0,
            items: vec![Item::assistant("done")],
            stop_reason: StopReason::EndTurn,
            usage: None,
            upstream_model: None,
            fail: None,
            fail_times: None,
            calls: AtomicUsize::new(0),
        }
    }
    fn failing(mut self, error: Error) -> Self {
        self.events.clear();
        self.fail = Some(error);
        self
    }
    fn usage(mut self, usage: Usage) -> Self {
        self.usage = Some(usage);
        self
    }
}

impl Model for Fake<'_> {
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
            self.clock.advance(self.before_first_ms);
            if fail.is_none() {
                for event in &self.events {
                    sink.emit(event.clone()).await?;
                }
            }
            self.clock.advance(self.after_ms);
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
                Some(error) if error.dispatch == Dispatch::NotSent => Err(error),
                Some(error) => Err(error.with_observation(observation)),
            }
        })
    }
}

struct Fleet<'c>(BTreeMap<Id, Fake<'c>>);

impl Models for Fleet<'_> {
    fn model(&self, serving_model_id: &Id) -> Option<&dyn Model> {
        self.0
            .get(serving_model_id)
            .map(|model| model as &dyn Model)
    }
}

fn fleet<'c>(models: impl IntoIterator<Item = (&'static str, Fake<'c>)>) -> Fleet<'c> {
    Fleet(
        models
            .into_iter()
            .map(|(serving, model)| (id(serving), model))
            .collect(),
    )
}

fn request() -> TurnRequest {
    let mut request = TurnRequest::new("code", vec![Item::user(PROMPT)]);
    request.max_output_tokens = Some(128);
    request
}

/// One per thousand input tokens and two per thousand output tokens, in USD, for `serving`.
fn prices(catalog: &Catalog, serving: &str, upstream: &str) -> PriceBook {
    let binding = catalog.binding(&id(serving)).unwrap();
    let rate = |amount: &str| {
        Some(Rate {
            amount: Amount::parse(amount).unwrap(),
            per_units: 1000,
        })
    };
    PriceBook::from_document(PriceDocument::new(
        id("prices-r7"),
        "fixture price list",
        1,
        Currency::new("USD").unwrap(),
        vec![ModelPrice {
            id: id("price-1"),
            serving_model_id: id(serving),
            binding_revision: binding.provenance().binding_revision.clone(),
            upstream_model: id(upstream),
            input: rate("1"),
            cached_input: None,
            cache_creation_input: None,
            output: rate("2"),
        }],
    ))
    .unwrap()
}

struct Ran {
    run: Result<FallbackRun, Error>,
    records: Vec<CallRecord>,
    shown: Vec<StreamEvent>,
}

fn run(
    catalog: &Catalog,
    models: &Fleet<'_>,
    request: &TurnRequest,
    clock: &Scripted,
    prices: Option<&PriceBook>,
) -> Ran {
    let log = CallLog::new();
    let mut sink = VecSink::new(64, 64 * 1024);
    let cancel = Cancel::new();
    let mut admit = |_: &llm_routing::Selection<'_>| Ok(());
    let pause = |_: Duration| -> BoxFuture<'static, ()> { Box::pin(async {}) };
    let run = block_on(record_turn(
        catalog,
        request,
        Some(100),
        FallbackPolicy::default(),
        Ports {
            models,
            admit: &mut admit,
            sink: &mut sink,
            cancel: &cancel,
            pause: &pause,
        },
        CallIdentity::primary(id("call-1")),
        Recording {
            sink: &log,
            clock,
            prices,
        },
    ));
    Ran {
        run,
        records: log.records(),
        shown: sink.into_events(),
    }
}

fn only(records: &[CallRecord]) -> &CallRecord {
    assert_eq!(records.len(), 1, "one call writes one record: {records:?}");
    &records[0]
}

#[test]
fn a_serialized_record_holds_none_of_the_turns_text() {
    let catalog = catalog(true);
    let clock = Scripted::at(1_000);
    let mut request = request();
    request.instructions = INSTRUCTIONS.to_owned();
    request.tools = vec![ToolSpec {
        name: ToolName::new("lookup").unwrap(),
        description: TOOL_DESCRIPTION.to_owned(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {"q": {"type": "string", "description": SCHEMA_TEXT}}
        }),
    }];
    let call = ToolCall {
        call_id: llm_core::CallId::new("call-a").unwrap(),
        name: ToolName::new("lookup").unwrap(),
        arguments: serde_json::json!({"q": ARGUMENTS}),
    };
    // The first attempt fails with prose in its message and is retried; the second answers with
    // text, reasoning and a tool call.
    let mut answering = Fake::on(&catalog, "remote-large", &clock)
        .failing(
            Error::new(ErrorCode::RateLimited, ERROR_PROSE)
                .with_dispatch(Dispatch::Rejected)
                .with_retriable(true),
        )
        .usage(Usage {
            input_tokens: Some(100),
            output_tokens: Some(20),
            cached_input_tokens: Some(0),
            cache_creation_input_tokens: Some(0),
            reasoning_output_tokens: Some(5),
        });
    answering.fail_times = Some(1);
    answering.events = vec![
        StreamEvent::ReasoningDelta {
            text: REASONING.to_owned(),
        },
        StreamEvent::TextDelta {
            text: ANSWER.to_owned(),
        },
        StreamEvent::ToolCallStarted {
            call_id: call.call_id.clone(),
            name: call.name.clone(),
        },
        StreamEvent::ToolArgumentsDelta {
            call_id: call.call_id.clone(),
            delta: serde_json::json!({"q": ARGUMENTS}).to_string(),
        },
    ];
    answering.items = vec![Item::assistant(ANSWER), Item::ToolCall(call)];
    answering.stop_reason = StopReason::ToolCalls;
    answering.upstream_model = Some(id("example/Large-Model"));
    let models = fleet([("remote-large", answering)]);
    let book = prices(&catalog, "remote-large", "example/Large-Model");
    let ran = run(&catalog, &models, &request, &clock, Some(&book));

    let outcome = ran.run.as_ref().unwrap().result.as_ref().unwrap();
    assert!(
        serde_json::to_string(outcome).unwrap().contains(ANSWER),
        "the caller still receives the answer"
    );
    let record = only(&ran.records);
    assert_eq!(
        (
            record.outcome,
            record.attempts,
            record.route_target_id.as_ref()
        ),
        (Outcome::Succeeded, 2, Some(&id("coding-secondary"))),
        "the record is of this turn, so its silence about the text means something"
    );
    let json = serde_json::to_string(record).unwrap();
    for text in [
        INSTRUCTIONS,
        PROMPT,
        TOOL_DESCRIPTION,
        SCHEMA_TEXT,
        ANSWER,
        REASONING,
        ARGUMENTS,
        ERROR_PROSE,
    ] {
        assert!(!json.contains(text), "the record holds {text:?}: {json}");
    }

    // A failure is recorded by its code; its message, prose included, stays the caller's.
    let primary_only = self::catalog(false);
    let refused = Fake::on(&primary_only, "local-small", &clock).failing(
        Error::new(ErrorCode::Unauthorized, ERROR_PROSE).with_dispatch(Dispatch::Rejected),
    );
    let models = fleet([("local-small", refused)]);
    let ran = run(&primary_only, &models, &self::request(), &clock, None);
    let error = ran.run.as_ref().unwrap().result.as_ref().unwrap_err();
    assert!(error.message.contains(ERROR_PROSE));
    let record = only(&ran.records);
    assert_eq!(record.error_code, Some(ErrorCode::Unauthorized));
    let json = serde_json::to_string(record).unwrap();
    for text in [PROMPT, ERROR_PROSE] {
        assert!(!json.contains(text), "the record holds {text:?}: {json}");
    }
}

#[test]
fn a_fallback_run_records_the_target_that_answered_and_its_attempt_count() {
    let catalog = catalog(true);
    let clock = Scripted::at(1_000);
    let mut answering = Fake::on(&catalog, "remote-large", &clock);
    answering.upstream_model = Some(id("example/Large-Model-2026"));
    let models = fleet([
        (
            "local-small",
            Fake::on(&catalog, "local-small", &clock)
                .failing(Error::new(ErrorCode::Unavailable, "down")),
        ),
        ("remote-large", answering),
    ]);
    let ran = run(&catalog, &models, &request(), &clock, None);
    let record = only(&ran.records);
    assert_eq!(record.id, id("call-1"));
    assert_eq!((record.role, record.kind), (Role::Primary, Kind::Turn));
    assert_eq!(record.route_id, id("coding"));
    assert_eq!(record.route_target_id, Some(id("coding-secondary")));
    assert_eq!(record.upstream_model, Some(id("example/Large-Model-2026")));
    assert_eq!(record.attempts, 2);
    assert_eq!(
        (record.outcome, record.error_code, record.dispatch),
        (Outcome::Succeeded, None, None)
    );
    assert_eq!(record.primary_record_id, None);
}

#[test]
fn a_record_keeps_unknown_usage_unknown_and_its_cost_incomplete() {
    let catalog = catalog(false);
    let clock = Scripted::at(1_000);
    let mut answering = Fake::on(&catalog, "local-small", &clock).usage(Usage {
        input_tokens: Some(100),
        cached_input_tokens: Some(0),
        cache_creation_input_tokens: Some(0),
        ..Usage::default()
    });
    answering.upstream_model = Some(id("example/Small-Model"));
    let models = fleet([("local-small", answering)]);
    let book = prices(&catalog, "local-small", "example/Small-Model");
    let ran = run(&catalog, &models, &request(), &clock, Some(&book));
    let record = only(&ran.records);
    assert_eq!(record.usage.input_tokens, Some(100));
    assert_eq!(record.usage.output_tokens, None, "unknown is never zero");
    let json = serde_json::to_value(record).unwrap();
    assert!(json["usage"].get("output_tokens").is_none(), "{json}");
    let cost = record.cost.as_ref().unwrap();
    assert!(
        !cost.complete,
        "an unknown output count leaves the cost incomplete"
    );
    assert_eq!(cost.known_subtotal.to_string(), "0.1");
    assert_eq!(
        (cost.currency.as_str(), cost.price_revision.as_str()),
        ("USD", "prices-r7")
    );

    // The same call with every counter known is complete.
    let mut answering = Fake::on(&catalog, "local-small", &clock).usage(Usage {
        input_tokens: Some(100),
        output_tokens: Some(20),
        cached_input_tokens: Some(0),
        cache_creation_input_tokens: Some(0),
        reasoning_output_tokens: None,
    });
    answering.upstream_model = Some(id("example/Small-Model"));
    let models = fleet([("local-small", answering)]);
    let ran = run(&catalog, &models, &request(), &clock, Some(&book));
    let cost = only(&ran.records).cost.clone().unwrap();
    assert!(cost.complete);
    assert_eq!(cost.known_subtotal.to_string(), "0.14");
}

#[test]
fn a_failed_call_records_its_error_code_and_dispatch_without_a_zero_cost() {
    let catalog = catalog(false);
    let clock = Scripted::at(1_000);
    let models = fleet([(
        "local-small",
        Fake::on(&catalog, "local-small", &clock)
            .failing(Error::new(ErrorCode::Unauthorized, "no").with_dispatch(Dispatch::Rejected)),
    )]);
    let book = prices(&catalog, "local-small", "example/Small-Model");
    let ran = run(&catalog, &models, &request(), &clock, Some(&book));
    let record = only(&ran.records);
    assert_eq!(record.outcome, Outcome::Failed);
    assert_eq!(record.error_code, Some(ErrorCode::Unauthorized));
    assert_eq!(record.dispatch, Some(Dispatch::Rejected));
    assert_eq!(record.attempts, 1);
    assert_eq!(record.route_target_id, Some(id("coding-primary")));
    assert_eq!(record.usage, Usage::default());
    let cost = record.cost.as_ref().unwrap();
    assert!(
        !cost.complete,
        "an attempt with unknown usage is never priced at zero"
    );
}

#[test]
fn a_record_carries_latency_and_time_to_first_event_from_the_scripted_clock() {
    let catalog = catalog(false);
    let clock = Scripted::at(1_762_300_800_000);
    let mut answering = Fake::on(&catalog, "local-small", &clock);
    answering.before_first_ms = 40;
    answering.after_ms = 210;
    let models = fleet([("local-small", answering)]);
    let ran = run(&catalog, &models, &request(), &clock, None);
    let record = only(&ran.records);
    assert_eq!(record.started_at_unix_ms, 1_762_300_800_000);
    assert_eq!(record.time_to_first_event_ms, Some(40));
    assert_eq!(record.latency_ms, Some(250));
}

#[test]
fn a_retry_warning_is_not_the_first_event() {
    let catalog = catalog(false);
    let clock = Scripted::at(5_000);
    let mut answering = Fake::on(&catalog, "local-small", &clock).failing(
        Error::new(ErrorCode::Unavailable, "busy")
            .with_dispatch(Dispatch::Rejected)
            .with_retriable(true),
    );
    answering.fail_times = Some(1);
    answering.before_first_ms = 100;
    answering.after_ms = 10;
    answering.events = vec![StreamEvent::TextDelta {
        text: "late".to_owned(),
    }];
    let models = fleet([("local-small", answering)]);
    let ran = run(&catalog, &models, &request(), &clock, None);
    assert!(
        matches!(ran.shown.first(), Some(StreamEvent::Warning { .. })),
        "the retry warning came first: {:?}",
        ran.shown
    );
    let record = only(&ran.records);
    assert_eq!(record.attempts, 2);
    // Warning at 110 ms; the first text after the second attempt's own 100 ms, at 210 ms.
    assert_eq!(record.time_to_first_event_ms, Some(210));
    assert_eq!(record.latency_ms, Some(220));
}

#[test]
fn a_call_without_a_price_book_records_usage_and_no_cost() {
    let catalog = catalog(false);
    let clock = Scripted::at(1_000);
    let models = fleet([(
        "local-small",
        Fake::on(&catalog, "local-small", &clock).usage(Usage {
            input_tokens: Some(12),
            output_tokens: Some(3),
            ..Usage::default()
        }),
    )]);
    let ran = run(&catalog, &models, &request(), &clock, None);
    let record = only(&ran.records);
    assert_eq!(
        (record.usage.input_tokens, record.usage.output_tokens),
        (Some(12), Some(3))
    );
    assert_eq!(record.cost, None);
}

#[test]
fn recording_returns_the_run_unchanged() {
    let catalog = catalog(true);
    let clock = Scripted::at(1_000);
    let make = || {
        fleet([
            (
                "local-small",
                Fake::on(&catalog, "local-small", &clock).failing(
                    Error::new(ErrorCode::Transport, "cut").with_dispatch(Dispatch::Rejected),
                ),
            ),
            ("remote-large", Fake::on(&catalog, "remote-large", &clock)),
        ])
    };
    let recorded = run(&catalog, &make(), &request(), &clock, None);
    let models = make();
    let mut sink = VecSink::new(64, 64 * 1024);
    let cancel = Cancel::new();
    let mut admit = |_: &llm_routing::Selection<'_>| Ok(());
    let pause = |_: Duration| -> BoxFuture<'static, ()> { Box::pin(async {}) };
    let plain = block_on(catalog.run_turn(
        &request(),
        Some(100),
        FallbackPolicy::default(),
        Ports {
            models: &models,
            admit: &mut admit,
            sink: &mut sink,
            cancel: &cancel,
            pause: &pause,
        },
    ))
    .unwrap();
    let with = recorded.run.unwrap();
    assert_eq!(with.halt, plain.halt);
    assert_eq!(with.attempts, plain.attempts);
    assert_eq!(with.result, plain.result);
    assert_eq!(recorded.shown, sink.into_events());
}
