//! Adversary cases for ordered fallback: scripted `llm_core::Model` fakes, no network.

use llm_core::{
    BoxFuture, Cancel, Dispatch, Error, ErrorCode, Id, Item, Model, StopReason, StreamSink,
    TurnObservation, TurnOutcome, TurnRequest, Usage, VecSink,
};
use llm_routing::{
    AttemptResult, Catalog, CatalogDocument, FallbackPolicy, FallbackRun, Halt, MAX_ROUTE_TARGETS,
    Models, Ports, Selection,
};
use std::{
    collections::BTreeMap,
    future::Future,
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Poll, Waker},
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

fn request() -> TurnRequest {
    let mut request = TurnRequest::new("code", vec![Item::user("hello")]);
    request.max_output_tokens = Some(128);
    request
}

#[derive(Clone, Copy)]
enum Script {
    Succeed,
    /// Returns `Ok` with an outcome that fails `TurnOutcome::validate_for` (caller-owned item),
    /// while its observation is valid for the binding and reports usage.
    InvalidOutcome,
    /// Cancels the caller's token, then fails with an eligible, unsent failure.
    CancelThenFailUnsent,
    /// Fails with the given code, rejected by upstream, with valid bound evidence.
    FailRejected(ErrorCode),
}

struct Fake {
    provenance: llm_core::Provenance,
    capabilities: llm_core::Capabilities,
    script: Script,
    calls: AtomicUsize,
}

impl Model for Fake {
    fn provenance(&self) -> &llm_core::Provenance {
        &self.provenance
    }
    fn capabilities(&self) -> &llm_core::Capabilities {
        &self.capabilities
    }
    fn turn<'a>(
        &'a self,
        _: &'a TurnRequest,
        _: &'a mut dyn StreamSink,
        cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut observation = TurnObservation::new(self.provenance.clone());
            observation.final_usage = true;
            observation.usage = Some(Usage {
                input_tokens: Some(7),
                ..Usage::default()
            });
            match self.script {
                Script::Succeed => Ok(TurnOutcome {
                    stop_reason: StopReason::EndTurn,
                    items: vec![Item::assistant("done")],
                    observation,
                }),
                Script::InvalidOutcome => Ok(TurnOutcome {
                    stop_reason: StopReason::EndTurn,
                    items: vec![Item::user("caller-owned content in model output")],
                    observation,
                }),
                Script::CancelThenFailUnsent => {
                    cancel.cancel();
                    Err(Error::new(ErrorCode::Transport, "scripted"))
                }
                Script::FailRejected(code) => Err(Error::new(code, "scripted")
                    .with_dispatch(Dispatch::Rejected)
                    .with_observation(observation)),
            }
        })
    }
}

struct Fleet(BTreeMap<Id, Fake>);

impl Fleet {
    fn new(catalog: &Catalog, primary: Script) -> Self {
        let make = |serving: &str, script| {
            let binding = catalog.binding(&id(serving)).unwrap();
            (
                id(serving),
                Fake {
                    provenance: binding.provenance().clone(),
                    capabilities: binding.capabilities().clone(),
                    script,
                    calls: AtomicUsize::new(0),
                },
            )
        };
        Self(
            [
                make("local-small", primary),
                make("remote-large", Script::Succeed),
            ]
            .into_iter()
            .collect(),
        )
    }
    fn calls(&self, serving: &str) -> usize {
        self.0[&id(serving)].calls.load(Ordering::SeqCst)
    }
}

impl Models for Fleet {
    fn model(&self, serving_model_id: &Id) -> Option<&dyn Model> {
        self.0
            .get(serving_model_id)
            .map(|model| model as &dyn Model)
    }
}

fn run_with(
    catalog: &Catalog,
    models: &Fleet,
    policy: FallbackPolicy,
    cancel: &Cancel,
) -> Result<FallbackRun, Error> {
    let mut sink = VecSink::new(16, 4096);
    let mut admit = |_: &Selection<'_>| Ok(());
    block_on(catalog.run_turn(
        &request(),
        Some(100),
        policy,
        Ports {
            models,
            admit: &mut admit,
            sink: &mut sink,
            cancel,
        },
    ))
}

/// Context: "Every attempted route contributes certainty/usage/cost evidence". An accepted
/// attempt whose outcome is refused still reported valid, bound usage; the run must keep it.
#[test]
fn adversary_refused_outcome_keeps_the_attempts_valid_usage_evidence() {
    let catalog = catalog();
    let models = Fleet::new(&catalog, Script::InvalidOutcome);
    let run = run_with(&catalog, &models, FallbackPolicy::default(), &Cancel::new()).unwrap();
    assert_eq!(run.halt, Halt::IneligibleFailure);
    assert_eq!(models.calls("remote-large"), 0);
    match &run.attempts[0].result {
        AttemptResult::Failed {
            dispatch,
            observation,
            ..
        } => {
            assert_eq!(*dispatch, Dispatch::Accepted);
            let observation = observation
                .as_ref()
                .expect("accepted attempt's valid bound observation was dropped");
            assert_eq!(observation.usage.as_ref().unwrap().input_tokens, Some(7));
        }
        AttemptResult::Succeeded { .. } => panic!("outcome was scripted to be refused"),
    }
}

/// Kills the mutant that drops the pre-attempt cancellation check (no existing case reaches
/// `Halt::Cancelled`).
#[test]
fn adversary_a_cancelled_caller_starts_no_attempt() {
    let catalog = catalog();
    let models = Fleet::new(&catalog, Script::Succeed);
    let cancel = Cancel::new();
    cancel.cancel();
    let run = run_with(&catalog, &models, FallbackPolicy::default(), &cancel).unwrap();
    assert_eq!(run.halt, Halt::Cancelled);
    assert!(run.attempts.is_empty());
    assert_eq!(models.calls("local-small"), 0);
    assert_eq!(run.result.unwrap_err().code, ErrorCode::Cancelled);
}

/// Cancellation during an attempt that then failed eligibly must stop the fallback.
#[test]
fn adversary_cancellation_during_an_attempt_stops_fallback() {
    let catalog = catalog();
    let models = Fleet::new(&catalog, Script::CancelThenFailUnsent);
    let cancel = Cancel::new();
    let run = run_with(&catalog, &models, FallbackPolicy::default(), &cancel).unwrap();
    assert_eq!(run.halt, Halt::Cancelled);
    assert_eq!(run.attempts.len(), 1);
    assert_eq!(models.calls("remote-large"), 0);
}

/// `docs/verification/routing-fallback.md` names `too-large` and `unsupported` as ineligible;
/// the existing ineligible list covers neither, so widening `eligible` to them survives.
#[test]
fn adversary_too_large_and_unsupported_never_fall_back() {
    for code in [ErrorCode::TooLarge, ErrorCode::Unsupported] {
        let catalog = catalog();
        let models = Fleet::new(&catalog, Script::FailRejected(code));
        let run = run_with(&catalog, &models, FallbackPolicy::default(), &Cancel::new()).unwrap();
        assert_eq!(run.halt, Halt::IneligibleFailure, "{code:?}");
        assert_eq!(models.calls("remote-large"), 0, "{code:?}");
    }
}

/// Kills `>` -> `>=` on the upper attempt bound: the documented range is one through 64.
#[test]
fn adversary_attempt_bound_accepts_the_route_maximum_and_refuses_one_more() {
    let at_max = FallbackPolicy {
        max_attempts: MAX_ROUTE_TARGETS,
        deadline: None,
    };
    assert!(at_max.validate().is_ok());
    let over = FallbackPolicy {
        max_attempts: MAX_ROUTE_TARGETS + 1,
        deadline: None,
    };
    assert_eq!(over.validate().unwrap_err().code, ErrorCode::InvalidRequest);
}
