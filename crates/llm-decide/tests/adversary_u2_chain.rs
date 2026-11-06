//! Adversary cases for the closed-label classifier chain (unit u2): what the chain itself holds
//! a classifier's result to, which cause a provider's own deadline error abstains with, and the
//! deadline comparison on a clock finer than a millisecond.

use llm_calls::{CallLog, CallRecord, Clock, Recording};
use llm_core::{
    BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item, Model, Provenance,
    StopReason, StreamSink, TurnObservation, TurnOutcome, TurnRequest,
};
use llm_decide::{
    Abstention, AbstentionCause, Answer, Ask, Chain, ChainOutcome, Classification, Classifier,
    Decision, LabelConfidence, LabelSet, Lexical, LexicalClassifier, ModelClassifier, Path, Ports,
};
use llm_routing::{Catalog, CatalogDocument, Models, Selection};
use serde_json::json;
use std::{
    collections::BTreeMap,
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll, Waker},
    time::Duration,
};

const EXAMPLE: &str = include_str!("../../../examples/catalog.toml");

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

/// A clock in microseconds that moves only when a model, a lexical classifier or a pause says so.
struct Micros(AtomicU64);

impl Micros {
    fn new() -> Arc<Self> {
        Arc::new(Self(AtomicU64::new(0)))
    }
    fn advance(&self, micros: u64) {
        self.0.fetch_add(micros, Ordering::SeqCst);
    }
}

impl Clock for Micros {
    fn unix_ms(&self) -> u64 {
        1_762_430_400_000 + self.0.load(Ordering::SeqCst) / 1_000
    }
    fn monotonic(&self) -> Duration {
        Duration::from_micros(self.0.load(Ordering::SeqCst))
    }
}

#[derive(Clone)]
enum Reply {
    Text(String),
    Fail(Error),
}

/// Takes `takes_micros` on the clock, then replies.
struct Fake {
    provenance: Provenance,
    capabilities: Capabilities,
    clock: Arc<Micros>,
    takes_micros: u64,
    reply: Reply,
}

impl Fake {
    fn on(catalog: &Catalog, serving: &str, clock: &Arc<Micros>, reply: Reply) -> Self {
        let binding = catalog.binding(&id(serving)).unwrap();
        Self {
            provenance: binding.provenance().clone(),
            capabilities: binding.capabilities().clone(),
            clock: clock.clone(),
            takes_micros: 0,
            reply,
        }
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
        _request: &'a TurnRequest,
        _sink: &'a mut dyn StreamSink,
        _cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        Box::pin(async move {
            let mut observation = TurnObservation::new(self.provenance.clone());
            self.clock.advance(self.takes_micros);
            match &self.reply {
                Reply::Fail(error) => Err(error.clone().with_observation(observation)),
                Reply::Text(text) => {
                    observation.final_usage = true;
                    Ok(TurnOutcome {
                        stop_reason: StopReason::EndTurn,
                        items: vec![Item::assistant(text.clone())],
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

fn fleet(catalog: &Catalog, clock: &Arc<Micros>, reply: &Reply, takes_micros: u64) -> Fleet {
    Fleet(
        ["local-small", "remote-large"]
            .into_iter()
            .map(|serving| {
                let mut fake = Fake::on(catalog, serving, clock, reply.clone());
                fake.takes_micros = takes_micros;
                (id(serving), fake)
            })
            .collect(),
    )
}

struct Ran {
    outcome: ChainOutcome,
    records: Vec<CallRecord>,
}

fn run(chain: &Chain, catalog: &Catalog, models: &Fleet, clock: &Arc<Micros>) -> Ran {
    let log = CallLog::new();
    let cancel = Cancel::new();
    let admit = |_: &Selection<'_>| Ok(());
    let moved_by_waits = clock.clone();
    let pause = move |wait: Duration| -> BoxFuture<'static, ()> {
        let clock = moved_by_waits.clone();
        Box::pin(async move { clock.advance(u64::try_from(wait.as_micros()).unwrap()) })
    };
    let outcome = block_on(chain.classify(
        &id("req-1"),
        "the film was wonderful",
        Some(100),
        Ports {
            catalog,
            models,
            admit: &admit,
            cancel: &cancel,
            pause: &pause,
            recording: Recording {
                sink: &log,
                clock: &**clock,
                prices: None,
            },
        },
    ));
    Ran {
        outcome,
        records: log.records(),
    }
}

fn cause(classification: &Classification) -> Option<AbstentionCause> {
    match classification {
        Classification::Decided(_) => None,
        Classification::Abstained(abstention) => Some(abstention.cause),
    }
}

/// A classifier written against the public trait, returning exactly what it was built with.
struct Returns {
    id: Id,
    result: Classification,
}

impl Classifier for Returns {
    fn id(&self) -> &Id {
        &self.id
    }
    fn path(&self) -> Path {
        Path::Lexical
    }
    fn min_confidence_bps(&self) -> Option<u16> {
        None
    }
    fn classify<'a>(&'a self, _ask: Ask<'a>) -> BoxFuture<'a, Classification> {
        Box::pin(async move { self.result.clone() })
    }
}

/// Labels everything "neutral" with a known confidence.
struct Neutral;

impl Lexical for Neutral {
    fn classify(&self, _: &LabelSet, _: &str) -> Option<Answer> {
        Some(Answer {
            label: "neutral".into(),
            confidences_bps: BTreeMap::from([("neutral".to_owned(), 7000)]),
        })
    }
}

/// Takes `micros` on the clock and abstains.
struct Slow {
    clock: Arc<Micros>,
    micros: u64,
}

impl Lexical for Slow {
    fn classify(&self, _: &LabelSet, _: &str) -> Option<Answer> {
        self.clock.advance(self.micros);
        None
    }
}

/// The chain's answer is one label of its set or an abstention, whoever implements the
/// classifier: a decision naming "furious", with a confidence of 20000 basis points, is not one.
#[test]
fn a_chain_never_returns_a_decision_outside_its_label_set() {
    let catalog = catalog();
    let clock = Micros::new();
    let off_set = Decision {
        classifier_id: id("custom"),
        label_set_id: id("other-set"),
        label_set_version: "v9".into(),
        label: "furious".into(),
        confidences: vec![LabelConfidence {
            label: "furious".into(),
            confidence_bps: Some(20_000),
        }],
        latency_ms: 0,
        call_record_id: None,
    };
    let chain = Chain::new(
        id("chain"),
        labels(),
        Duration::from_secs(30),
        vec![Box::new(Returns {
            id: id("custom"),
            result: Classification::Decided(off_set),
        })],
    )
    .unwrap();
    let models = fleet(&catalog, &clock, &Reply::Text(String::new()), 0);
    let ran = run(&chain, &catalog, &models, &clock);
    if let Some(decision) = ran.outcome.decision() {
        let set = labels();
        assert!(
            set.contains(&decision.label),
            "the chain decided {:?}, outside {:?}",
            decision.label,
            set.labels()
        );
        assert!(
            decision
                .confidences
                .iter()
                .all(|c| set.contains(&c.label) && c.confidence_bps.is_none_or(|b| b <= 10_000)),
            "{:?}",
            decision.confidences
        );
    }
}

/// Only the last abstention of a chain is final: an abstention a classifier returned already
/// marked final is not the chain's answer when a later classifier decides.
#[test]
fn an_abstention_before_a_decision_is_never_final() {
    let catalog = catalog();
    let clock = Micros::new();
    let chain = Chain::new(
        id("chain"),
        labels(),
        Duration::from_secs(30),
        vec![
            Box::new(Returns {
                id: id("custom"),
                result: Classification::Abstained(Abstention {
                    classifier_id: id("custom"),
                    cause: AbstentionCause::OffLabel,
                    is_final: true,
                    latency_ms: 0,
                    call_record_id: None,
                }),
            }),
            Box::new(LexicalClassifier::new(id("words"), Neutral)),
        ],
    )
    .unwrap();
    let models = fleet(&catalog, &clock, &Reply::Text(String::new()), 0);
    let ran = run(&chain, &catalog, &models, &clock);
    assert_eq!(
        ran.outcome.decision().map(|d| d.label.as_str()),
        Some("neutral")
    );
    let finals: Vec<bool> = ran
        .outcome
        .attempts
        .iter()
        .map(|attempt| matches!(attempt, Classification::Abstained(a) if a.is_final))
        .collect();
    assert_eq!(finals, [false, false], "{:?}", ran.outcome.attempts);
}

/// A provider's own deadline error (llm-http's response-header deadline, a client's turn
/// deadline), 10 ms into a 30 s chain, is a failed call: it abstains `ProviderError(deadline)`,
/// not `Timeout`, whose own documentation says the chain's deadline passed.
#[test]
fn a_provider_deadline_error_inside_the_chain_deadline_abstains_with_its_code() {
    let catalog = catalog();
    let clock = Micros::new();
    let failure = Error::new(
        ErrorCode::Deadline,
        "HTTP response-header deadline exceeded",
    )
    .with_dispatch(Dispatch::Unknown);
    let models = fleet(&catalog, &clock, &Reply::Fail(failure), 10_000);
    let chain = Chain::new(
        id("chain"),
        labels(),
        Duration::from_secs(30),
        vec![Box::new(ModelClassifier::json_schema(id("schema"), "code"))],
    )
    .unwrap();
    let ran = run(&chain, &catalog, &models, &clock);
    assert_eq!(ran.records.len(), 1, "{:?}", ran.records);
    assert_eq!(ran.records[0].error_code, Some(ErrorCode::Deadline));
    let Some(Classification::Abstained(abstention)) = ran.outcome.result() else {
        panic!("a failed call abstains: {:?}", ran.outcome);
    };
    assert!(
        abstention.latency_ms < 30_000,
        "the chain deadline did not pass"
    );
    assert_eq!(
        abstention.cause,
        AbstentionCause::ProviderError(ErrorCode::Deadline),
        "{:?}",
        ran.outcome
    );
}

/// An answer that arrives before the deadline is used. The chain's 2000 ms deadline has
/// 1999.6 ms left after a lexical classifier spent 0.4 ms; the model answers 1999.2 ms later,
/// at 1999.6 ms, before the deadline, yet both durations truncate to 1999 whole milliseconds.
#[test]
fn an_answer_before_the_deadline_is_used_on_a_clock_finer_than_a_millisecond() {
    let catalog = catalog();
    let clock = Micros::new();
    let answer = json!({
        "label": "positive",
        "confidences": {"positive": 9000, "negative": 500, "neutral": 500}
    })
    .to_string();
    let models = fleet(&catalog, &clock, &Reply::Text(answer), 1_999_200);
    let chain = Chain::new(
        id("chain"),
        labels(),
        Duration::from_millis(2_000),
        vec![
            Box::new(LexicalClassifier::new(
                id("slow"),
                Slow {
                    clock: clock.clone(),
                    micros: 400,
                },
            )),
            Box::new(ModelClassifier::json_schema(id("schema"), "code")),
        ],
    )
    .unwrap();
    let ran = run(&chain, &catalog, &models, &clock);
    assert!(
        clock.monotonic() < Duration::from_millis(2_000),
        "the answer arrived before the deadline: {:?}",
        clock.monotonic()
    );
    assert_eq!(
        ran.outcome.result().and_then(cause),
        None,
        "an answer before the deadline decides: {:?}",
        ran.outcome
    );
    assert_eq!(
        ran.outcome.decision().map(|d| d.label.as_str()),
        Some("positive")
    );
}
