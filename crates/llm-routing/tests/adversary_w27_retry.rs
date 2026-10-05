//! Adversary pass 1 (wave 2026-10-05-w27) on same-target retry: how retry composes with the
//! caller's deadline and cancellation. Scripted `Model`s and a recording pause port; no network
//! and no wall-clock wait.

use llm_core::{
    BoxFuture, Cancel, Dispatch, Error, ErrorCode, Id, Item, Model, StopReason, StreamEvent,
    StreamSink, TurnObservation, TurnOutcome, TurnRequest, VecSink,
};
use llm_routing::{Catalog, CatalogDocument, FallbackPolicy, FallbackRun, Halt, Models, Ports};
use std::{
    collections::BTreeMap,
    future::Future,
    path::Path,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};

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

/// The example catalog with fallback on: `coding-primary` (local-small), then
/// `coding-secondary` (remote-large).
fn catalog() -> Catalog {
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets the manifest dir");
    let source = std::fs::read_to_string(Path::new(&manifest).join("../../examples/catalog.toml"))
        .expect("the example catalog");
    let mut doc = CatalogDocument::parse(&source).unwrap();
    doc.routes[0].fallback_enabled = true;
    doc.validate().unwrap()
}

fn request() -> TurnRequest {
    let mut request = TurnRequest::new("code", vec![Item::user("hello")]);
    request.max_output_tokens = Some(128);
    request
}

#[derive(Clone, Copy)]
struct Failure {
    code: ErrorCode,
    dispatch: Dispatch,
    retry_after_ms: Option<u64>,
    /// Cancels the caller's token as the failure is returned: the cancellation lands in the
    /// window between the attempt's end and routing's retry decision.
    cancel_on_return: bool,
}

struct Scripted {
    provenance: llm_core::Provenance,
    capabilities: llm_core::Capabilities,
    fail: Option<Failure>,
    calls: AtomicUsize,
}

impl Model for Scripted {
    fn provenance(&self) -> &llm_core::Provenance {
        &self.provenance
    }
    fn capabilities(&self) -> &llm_core::Capabilities {
        &self.capabilities
    }
    fn turn<'a>(
        &'a self,
        _: &'a TurnRequest,
        sink: &'a mut dyn StreamSink,
        cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut observation = TurnObservation::new(self.provenance.clone());
            match self.fail {
                None => {
                    sink.emit(StreamEvent::TextDelta {
                        text: "answer".to_owned(),
                    })
                    .await?;
                    observation.final_usage = true;
                    Ok(TurnOutcome {
                        stop_reason: StopReason::EndTurn,
                        items: vec![Item::assistant("answer")],
                        observation,
                    })
                }
                Some(failure) => {
                    let mut error = Error::new(failure.code, "scripted failure")
                        .with_dispatch(failure.dispatch)
                        .with_retriable(true);
                    error.retry_after_ms = failure.retry_after_ms;
                    if failure.cancel_on_return {
                        cancel.cancel();
                    }
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

impl Fleet {
    fn new(catalog: &Catalog, primary: Failure) -> Self {
        let scripted = |serving: &str, fail| {
            let binding = catalog.binding(&id(serving)).unwrap();
            (
                id(serving),
                Scripted {
                    provenance: binding.provenance().clone(),
                    capabilities: binding.capabilities().clone(),
                    fail,
                    calls: AtomicUsize::new(0),
                },
            )
        };
        Self(BTreeMap::from([
            scripted("local-small", Some(primary)),
            scripted("remote-large", None),
        ]))
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

struct Outcome {
    run: FallbackRun,
    waits: Vec<Duration>,
    warnings: Vec<String>,
}

fn run(catalog: &Catalog, policy: FallbackPolicy, models: &Fleet) -> Outcome {
    let cancel = Cancel::new();
    let waits = Mutex::new(Vec::new());
    let pause = |wait: Duration| -> BoxFuture<'static, ()> {
        waits.lock().unwrap().push(wait);
        Box::pin(std::future::ready(()))
    };
    let mut sink = VecSink::new(64, 64 * 1024);
    let run = block_on(catalog.run_turn(
        &request(),
        Some(100),
        policy,
        Ports {
            models,
            admit: &mut |_| Ok(()),
            sink: &mut sink,
            cancel: &cancel,
            pause: &pause,
        },
    ))
    .unwrap();
    let warnings = sink
        .events()
        .iter()
        .filter_map(|event| match event {
            StreamEvent::Warning { code, message } => Some(format!("{code}: {message}")),
            _ => None,
        })
        .collect();
    Outcome {
        run,
        waits: waits.into_inner().unwrap(),
        warnings,
    }
}

fn attempted(run: &FallbackRun) -> Vec<&str> {
    run.attempts
        .iter()
        .map(|attempt| attempt.target_id.as_str())
        .collect()
}

/// `spec/domains/routing.yaml`: another attempt on the same target is made only when "the wait
/// would end before the caller's deadline", and a retriable failure "falls back to the next
/// declared target after its same-target attempts are spent". A rate-limited primary asking for
/// 30 s, with 5 s left on the caller's deadline, therefore gets no same-target retry, and the
/// healthy secondary can still start well before the deadline. Before this unit the same 429
/// (`RateLimited`/`Rejected`, eligible) fell back at once. The run must not end `deadline` while
/// a declared target could still start in time.
#[test]
fn a_retry_wait_that_would_pass_the_deadline_falls_back_instead_of_ending_the_run() {
    let catalog = catalog();
    let models = Fleet::new(
        &catalog,
        Failure {
            code: ErrorCode::RateLimited,
            dispatch: Dispatch::Rejected,
            retry_after_ms: Some(30_000),
            cancel_on_return: false,
        },
    );
    let policy = FallbackPolicy {
        deadline: Some(Instant::now() + Duration::from_secs(5)),
        ..FallbackPolicy::default()
    };
    let outcome = run(&catalog, policy, &models);
    assert_eq!(
        (outcome.run.halt, attempted(&outcome.run)),
        (Halt::Succeeded, vec!["coding-primary", "coding-secondary"]),
        "a 429 on the primary ended the run at the deadline check of a wait it never needed to \
         take; the secondary was called {} times; result {:?}",
        models.calls("remote-large"),
        outcome.run.result.as_ref().err()
    );
    assert_eq!(outcome.waits, [] as [Duration; 0]);
}

/// Harness `stream_turn` decides `again` only while `!self.cancel.is_cancelled()`
/// (`harness-http/src/transport.rs:193`-`196`) and states `turn-retried` only when it will retry.
/// A cancellation that lands as a retriable failure returns must end the run without telling
/// the caller a retry is coming and without asking the pause port for a wait.
#[test]
fn a_cancellation_that_lands_with_a_retriable_failure_states_no_retry() {
    let catalog = catalog();
    let models = Fleet::new(
        &catalog,
        Failure {
            code: ErrorCode::Transport,
            dispatch: Dispatch::NotSent,
            retry_after_ms: None,
            cancel_on_return: true,
        },
    );
    let outcome = run(&catalog, FallbackPolicy::default(), &models);
    assert_eq!(outcome.run.halt, Halt::Cancelled);
    assert_eq!(attempted(&outcome.run), ["coding-primary"]);
    assert_eq!(
        outcome.warnings,
        [] as [String; 0],
        "the caller cancelled, and was then told the attempt is retried"
    );
    assert_eq!(
        outcome.waits,
        [] as [Duration; 0],
        "a wait was asked for after cancel"
    );
}
