//! Adversary cases for `record_turn` over runs with more than one attempt, and a cancelled run.
//!
//! Each case scripts per-attempt behaviour on one serving model, so an earlier attempt can be
//! accepted by the far side and fail retriably while a later one answers.

use llm_calls::{CallIdentity, CallLog, CallRecord, Clock, Outcome, Recording, record_turn};
use llm_core::{
    BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item, Model, Provenance,
    StopReason, StreamEvent, StreamSink, TurnObservation, TurnOutcome, TurnRequest, Usage, VecSink,
};
use llm_cost::{Amount, Currency, ModelPrice, PriceBook, PriceDocument, Rate};
use llm_routing::{Catalog, CatalogDocument, FallbackPolicy, Models, Ports, Selection};
use std::{
    future::Future,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
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

/// The example catalog with fallback off: only `coding-primary` (`local-small`) is attempted,
/// and same-target retry stays on under `FallbackPolicy::default`.
fn catalog() -> Catalog {
    let mut doc = CatalogDocument::parse(EXAMPLE).unwrap();
    doc.routes[0].fallback_enabled = false;
    doc.validate().unwrap()
}

struct Still;

impl Clock for Still {
    fn unix_ms(&self) -> u64 {
        1_000
    }
    fn monotonic(&self) -> Duration {
        Duration::ZERO
    }
}

/// One scripted step per attempt, in order.
enum Step {
    /// The far side accepted the request, then failed retriably before any output, reporting
    /// the given usage (none when `None`).
    AcceptedThenFailed(Option<Usage>),
    /// Answers with the given usage, naming `example/Small-Model`.
    Answers(Usage),
}

struct PerAttempt {
    provenance: Provenance,
    capabilities: Capabilities,
    steps: Mutex<Vec<Step>>,
    calls: AtomicUsize,
}

impl PerAttempt {
    fn on(catalog: &Catalog, steps: Vec<Step>) -> Self {
        let binding = catalog.binding(&id("local-small")).unwrap();
        Self {
            provenance: binding.provenance().clone(),
            capabilities: binding.capabilities().clone(),
            steps: Mutex::new(steps),
            calls: AtomicUsize::new(0),
        }
    }
}

impl Model for PerAttempt {
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
            self.calls.fetch_add(1, Ordering::SeqCst);
            let step = self.steps.lock().unwrap().remove(0);
            let mut observation = TurnObservation::new(self.provenance.clone());
            observation.upstream_model = Some(id("example/Small-Model"));
            match step {
                Step::AcceptedThenFailed(usage) => {
                    observation.usage = usage;
                    observation.final_usage = true;
                    Err(
                        Error::new(ErrorCode::Unavailable, "the provider failed this response")
                            .with_dispatch(Dispatch::Accepted)
                            .with_retriable(true)
                            .with_observation(observation),
                    )
                }
                Step::Answers(usage) => {
                    sink.emit(StreamEvent::TextDelta {
                        text: "done".to_owned(),
                    })
                    .await?;
                    observation.usage = Some(usage);
                    observation.final_usage = true;
                    Ok(TurnOutcome {
                        stop_reason: StopReason::EndTurn,
                        items: vec![Item::assistant("done")],
                        observation,
                    })
                }
            }
        })
    }
}

struct One<'m>(&'m PerAttempt);

impl Models for One<'_> {
    fn model(&self, serving_model_id: &Id) -> Option<&dyn Model> {
        (serving_model_id == &id("local-small")).then_some(self.0 as &dyn Model)
    }
}

/// One per thousand input tokens and two per thousand output tokens for `local-small`.
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

fn full_usage() -> Usage {
    Usage {
        input_tokens: Some(100),
        output_tokens: Some(20),
        cached_input_tokens: Some(0),
        cache_creation_input_tokens: Some(0),
        reasoning_output_tokens: None,
    }
}

/// Runs one routed turn through `record_turn`; `admit_first` attempts are admitted, every later
/// one is refused by the caller's limit check.
fn record(
    catalog: &Catalog,
    model: &PerAttempt,
    prices: Option<&PriceBook>,
    cancel: &Cancel,
    admit_first: usize,
) -> Vec<CallRecord> {
    let log = CallLog::new();
    let mut sink = VecSink::new(64, 64 * 1024);
    let admitted = AtomicUsize::new(0);
    let mut admit = |_: &Selection<'_>| {
        if admitted.fetch_add(1, Ordering::SeqCst) < admit_first {
            Ok(())
        } else {
            Err(Error::new(ErrorCode::RateLimited, "budget spent"))
        }
    };
    let pause = |_: Duration| -> BoxFuture<'static, ()> { Box::pin(async {}) };
    let models = One(model);
    let run = block_on(record_turn(
        catalog,
        &TurnRequest::new("code", vec![Item::user("hello")]),
        Some(100),
        FallbackPolicy::default(),
        Ports {
            models: &models,
            admit: &mut admit,
            sink: &mut sink,
            cancel,
            pause: &pause,
        },
        CallIdentity::primary(id("call-1")),
        Recording {
            sink: &log,
            clock: &Still,
            prices,
        },
    ));
    run.unwrap();
    log.records()
}

/// The first attempt reached the far side (dispatch accepted) and reported no usage, so what it
/// cost is unknown; routing retried it because nothing was shown. The call's cost has an unknown
/// line and must not claim to be complete.
#[test]
fn a_retried_accepted_attempt_with_unknown_usage_leaves_the_call_cost_incomplete() {
    let catalog = catalog();
    let book = prices(&catalog);
    let model = PerAttempt::on(
        &catalog,
        vec![Step::AcceptedThenFailed(None), Step::Answers(full_usage())],
    );
    let records = record(&catalog, &model, Some(&book), &Cancel::new(), usize::MAX);
    assert_eq!(records.len(), 1, "{records:?}");
    let record = &records[0];
    assert_eq!(
        (record.outcome, record.attempts),
        (Outcome::Succeeded, 2),
        "the run retried the accepted attempt: {record:?}"
    );
    let cost = record.cost.as_ref().unwrap();
    assert!(
        !cost.complete,
        "an accepted attempt with unknown usage is an unknown line of this call's cost, yet the \
         record says complete with subtotal {}: {record:?}",
        cost.known_subtotal
    );
}

/// The only attempt was accepted by the far side; the caller's limit check then refused the
/// retry. The record's dispatch is the evidence of what the far side may have done with the
/// call, and the far side accepted it.
#[test]
fn a_limit_refusal_after_an_accepted_attempt_is_not_recorded_as_not_sent() {
    let catalog = catalog();
    let model = PerAttempt::on(&catalog, vec![Step::AcceptedThenFailed(None)]);
    let records = record(&catalog, &model, None, &Cancel::new(), 1);
    assert_eq!(records.len(), 1, "{records:?}");
    let record = &records[0];
    assert_eq!(
        (record.attempts, record.route_target_id.as_ref()),
        (1, Some(&id("coding-primary"))),
        "{record:?}"
    );
    assert_ne!(
        record.dispatch,
        Some(Dispatch::NotSent),
        "the call's one attempt was accepted, yet the record says it was never sent: {record:?}"
    );
}

/// A run the caller cancelled before any attempt is recorded as cancelled, with no target, no
/// attempt and no cost even under a price book.
#[test]
fn a_run_cancelled_before_its_first_attempt_is_recorded_cancelled_without_a_target() {
    let catalog = catalog();
    let book = prices(&catalog);
    let model = PerAttempt::on(&catalog, vec![Step::Answers(full_usage())]);
    let cancel = Cancel::new();
    cancel.cancel();
    let records = record(&catalog, &model, Some(&book), &cancel, usize::MAX);
    assert_eq!(records.len(), 1, "{records:?}");
    let record = &records[0];
    assert_eq!(record.outcome, Outcome::Cancelled, "{record:?}");
    assert_eq!(record.error_code, Some(ErrorCode::Cancelled));
    assert_eq!(
        (
            record.attempts,
            record.route_target_id.as_ref(),
            record.cost.as_ref()
        ),
        (0, None, None)
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}
