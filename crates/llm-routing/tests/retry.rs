//! Same-target retry before any output is visible, and how it composes with fallback.
//!
//! Harness: `RetryPolicy` and `DEFAULT` (`harness-http/src/retry.rs:19`, `:39`), `exhausted`
//! (`:77`), the cancellable `pause` (`:100`), `WitnessedSink` (`witness.rs:34`), the attempt loop
//! and its `turn-retried` warning (`transport.rs:169`, `:208`-`217`) and the capped server delay
//! (`transport.rs:40`, `:431`). Scripted `Model`s and a recording pause port: no network and no
//! wall-clock wait, except where a test measures that a wait was cut short.

use llm_core::{
    BoxFuture, Cancel, Dispatch, Error, ErrorCode, Id, Item, Model, StopReason, StreamEvent,
    StreamSink, TurnObservation, TurnOutcome, TurnRequest, VecSink,
};
use llm_routing::{
    AttemptResult, Catalog, CatalogDocument, FallbackPolicy, FallbackRun, Halt, Models, Ports,
    RetryPolicy, Selection,
};
use std::{
    collections::BTreeMap,
    future::Future,
    path::Path,
    pin::Pin,
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

/// The example catalog: `coding-primary` (local-small), then `coding-secondary` (remote-large).
fn catalog(fallback_enabled: bool) -> Catalog {
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets the manifest dir");
    let source = std::fs::read_to_string(Path::new(&manifest).join("../../examples/catalog.toml"))
        .expect("the example catalog");
    let mut doc = CatalogDocument::parse(&source).unwrap();
    doc.routes[0].fallback_enabled = fallback_enabled;
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
    retriable: bool,
    retry_after_ms: Option<u64>,
}

const fn failure(code: ErrorCode, dispatch: Dispatch) -> Failure {
    Failure {
        code,
        dispatch,
        retriable: true,
        retry_after_ms: None,
    }
}

/// Fails its first `fail_times` attempts (every attempt when `None`), then succeeds.
struct Scripted {
    provenance: llm_core::Provenance,
    capabilities: llm_core::Capabilities,
    emits: usize,
    fail: Option<Failure>,
    fail_times: Option<usize>,
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
        _: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        Box::pin(async move {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            for _ in 0..self.emits {
                sink.emit(StreamEvent::TextDelta {
                    text: format!("from {}", self.provenance.endpoint.as_str()),
                })
                .await?;
            }
            let mut observation = TurnObservation::new(self.provenance.clone());
            let failing = self
                .fail
                .filter(|_| self.fail_times.is_none_or(|times| call < times));
            match failing {
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

impl Fleet {
    fn new(
        catalog: &Catalog,
        primary: Option<Failure>,
        fail_times: Option<usize>,
        emits: usize,
    ) -> Self {
        let scripted = |serving: &str, emits, fail, fail_times| {
            let binding = catalog.binding(&id(serving)).unwrap();
            (
                id(serving),
                Scripted {
                    provenance: binding.provenance().clone(),
                    capabilities: binding.capabilities().clone(),
                    emits,
                    fail,
                    fail_times,
                    calls: AtomicUsize::new(0),
                },
            )
        };
        Self(BTreeMap::from([
            scripted("local-small", emits, primary, fail_times),
            scripted("remote-large", 1, None, None),
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

/// A pause port that records every wait the run asks for and returns at once.
#[derive(Default)]
struct Recorder(Mutex<Vec<Duration>>);

impl Recorder {
    fn port(&self) -> impl Fn(Duration) -> BoxFuture<'static, ()> + Sync + '_ {
        |duration| {
            self.0.lock().unwrap().push(duration);
            Box::pin(std::future::ready(()))
        }
    }
    fn waits(&self) -> Vec<Duration> {
        self.0.lock().unwrap().clone()
    }
}

struct Outcome {
    run: FallbackRun,
    waits: Vec<Duration>,
    warnings: Vec<(String, String)>,
}

fn run_with(
    catalog: &Catalog,
    policy: FallbackPolicy,
    models: &Fleet,
    admit: &mut (dyn FnMut(&Selection<'_>) -> Result<(), Error> + Send),
) -> Result<Outcome, Error> {
    let cancel = Cancel::new();
    let recorder = Recorder::default();
    let pause = recorder.port();
    let mut sink = VecSink::new(64, 64 * 1024);
    let run = block_on(catalog.run_turn(
        &request(),
        Some(100),
        policy,
        Ports {
            models,
            admit,
            sink: &mut sink,
            cancel: &cancel,
            pause: &pause,
        },
    ))?;
    let warnings = sink
        .events()
        .iter()
        .filter_map(|event| match event {
            StreamEvent::Warning { code, message } => Some((code.clone(), message.clone())),
            _ => None,
        })
        .collect();
    Ok(Outcome {
        run,
        waits: recorder.waits(),
        warnings,
    })
}

fn run(catalog: &Catalog, policy: FallbackPolicy, models: &Fleet) -> Outcome {
    run_with(catalog, policy, models, &mut |_: &Selection<'_>| Ok(())).unwrap()
}

fn attempted(run: &FallbackRun) -> Vec<&str> {
    run.attempts
        .iter()
        .map(|attempt| attempt.target_id.as_str())
        .collect()
}

fn with_retry(retry: RetryPolicy) -> FallbackPolicy {
    FallbackPolicy {
        retry,
        ..FallbackPolicy::default()
    }
}

const fn secs(value: u64) -> Duration {
    Duration::from_secs(value)
}

/// H15: Harness's policy is the default, and retry is on without the caller asking.
#[test]
fn the_default_policy_is_four_attempts_pausing_one_two_four_then_eight_seconds() {
    let policy = RetryPolicy::DEFAULT;
    assert_eq!(policy.max_attempts, 4);
    assert_eq!(policy.backoff_base, Duration::from_millis(500));
    assert_eq!(policy.max_doublings, 4);
    assert_eq!(policy.max_server_delay, secs(30));
    assert_eq!(RetryPolicy::default(), policy);
    assert_eq!(FallbackPolicy::default().retry, policy);
    assert_eq!(
        FallbackPolicy::disabled().retry,
        policy,
        "disabling fallback keeps retry"
    );
    let waits: Vec<_> = [1, 2, 3, 4, 5, 99].map(|n| policy.backoff(n)).into();
    assert_eq!(
        waits,
        [secs(1), secs(2), secs(4), secs(8), secs(8), secs(8)]
    );
}

/// H15, H19, R42 (retry before answering): a retriable failure that showed nothing is attempted
/// again on the same target, by default, with a stated warning before each wait.
#[test]
fn a_retriable_failure_is_retried_on_the_same_target_before_any_output() {
    let catalog = catalog(true);
    let models = Fleet::new(
        &catalog,
        Some(failure(ErrorCode::RateLimited, Dispatch::Rejected)),
        Some(2),
        0,
    );
    let outcome = run(&catalog, FallbackPolicy::default(), &models);
    assert_eq!(outcome.run.halt, Halt::Succeeded);
    assert_eq!(
        attempted(&outcome.run),
        ["coding-primary", "coding-primary", "coding-primary"]
    );
    assert_eq!(models.calls("remote-large"), 0);
    assert_eq!(outcome.waits, [secs(1), secs(2)]);
    assert_eq!(
        outcome.warnings,
        [
            (
                "turn-retried".to_owned(),
                "attempt 1 of 4 on coding-primary failed before any output was visible \
                 (rate-limited) and is retried after 1000 ms"
                    .to_owned()
            ),
            (
                "turn-retried".to_owned(),
                "attempt 2 of 4 on coding-primary failed before any output was visible \
                 (rate-limited) and is retried after 2000 ms"
                    .to_owned()
            ),
        ]
    );
    // A warning is the library's own statement, not output of an attempt.
    assert!(
        outcome
            .run
            .attempts
            .iter()
            .all(|attempt| attempt.visible_events == 0)
    );
}

/// H16: once the last target's attempts are spent the error goes up final, naming the count.
#[test]
fn an_error_still_retriable_after_the_last_attempt_goes_up_final_naming_the_count() {
    let catalog = catalog(false);
    let models = Fleet::new(
        &catalog,
        Some(failure(ErrorCode::Transport, Dispatch::Unknown)),
        None,
        0,
    );
    let outcome = run(&catalog, FallbackPolicy::default(), &models);
    assert_eq!(outcome.run.halt, Halt::Exhausted);
    assert_eq!(attempted(&outcome.run), ["coding-primary"; 4]);
    assert_eq!(outcome.waits, [secs(1), secs(2), secs(4)]);
    let error = outcome.run.result.unwrap_err();
    assert_eq!(error.code, ErrorCode::Transport);
    assert!(
        !error.retriable,
        "an exhausted error must not invite four more attempts"
    );
    assert_eq!(error.message, "scripted failure (after 4 attempts)");
}

/// A future that is ready only once its instant has passed; polled in a busy loop it bounds a
/// test that would otherwise hang if cancellation were ignored.
struct Until(Instant);

impl Future for Until {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        if Instant::now() >= self.0 {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

/// H17: the caller's cancellation ends a back-off at once, whatever the pause port does.
#[test]
fn a_cancellation_during_back_off_ends_the_run_at_once() {
    let catalog = catalog(true);
    let models = Fleet::new(
        &catalog,
        Some(failure(ErrorCode::Transport, Dispatch::Unknown)),
        None,
        0,
    );
    let cancel = Cancel::new();
    let asked = Mutex::new(Vec::new());
    // The port cancels the run, then would hold it for three seconds.
    let pause = |duration: Duration| -> BoxFuture<'static, ()> {
        asked.lock().unwrap().push(duration);
        cancel.cancel();
        Box::pin(Until(Instant::now() + secs(3)))
    };
    let mut sink = VecSink::new(64, 64 * 1024);
    let started = Instant::now();
    let run = block_on(catalog.run_turn(
        &request(),
        Some(100),
        FallbackPolicy::default(),
        Ports {
            models: &models,
            admit: &mut |_: &Selection<'_>| Ok(()),
            sink: &mut sink,
            cancel: &cancel,
            pause: &pause,
        },
    ))
    .unwrap();
    assert!(
        started.elapsed() < secs(1),
        "a cancelled back-off was waited out: {:?}",
        started.elapsed()
    );
    assert_eq!(run.halt, Halt::Cancelled);
    assert_eq!(attempted(&run), ["coding-primary"]);
    assert_eq!(run.result.unwrap_err().code, ErrorCode::Cancelled);
    assert_eq!(*asked.lock().unwrap(), [secs(1)]);
}

/// H18, R42 (never after answering): an attempt that showed anything is final, though its class
/// stays honest for a caller that owns the transcript.
#[test]
fn an_attempt_that_showed_output_is_never_retried() {
    let catalog = catalog(true);
    let models = Fleet::new(
        &catalog,
        Some(failure(ErrorCode::Transport, Dispatch::Rejected)),
        None,
        1,
    );
    let outcome = run(&catalog, FallbackPolicy::default(), &models);
    assert_eq!(outcome.run.halt, Halt::VisibleOutput);
    assert_eq!(attempted(&outcome.run), ["coding-primary"]);
    assert_eq!(outcome.run.attempts[0].visible_events, 1);
    assert!(outcome.waits.is_empty());
    assert!(outcome.warnings.is_empty());
    let error = outcome.run.result.unwrap_err();
    assert!(error.retriable);
    assert_eq!(error.message, "scripted failure");
}

/// H20: a server delay is honoured up to the cap, waited inside the retry, and never shortens
/// the local back-off.
#[test]
fn a_server_delay_is_capped_waited_inside_the_retry_and_never_shortens_the_back_off() {
    let catalog = catalog(true);
    for (requested_ms, expected) in [
        (60_000, secs(30)),
        (5_000, secs(5)),
        (0, secs(1)),
        (u64::MAX, secs(30)),
    ] {
        let mut limited = failure(ErrorCode::RateLimited, Dispatch::Rejected);
        limited.retry_after_ms = Some(requested_ms);
        let models = Fleet::new(&catalog, Some(limited), Some(1), 0);
        let outcome = run(&catalog, FallbackPolicy::default(), &models);
        assert_eq!(outcome.run.halt, Halt::Succeeded, "{requested_ms}");
        assert_eq!(outcome.waits, [expected], "{requested_ms}");
        assert!(
            outcome.warnings[0]
                .1
                .ends_with(&format!("is retried after {} ms", expected.as_millis())),
            "{:?}",
            outcome.warnings
        );
    }
}

/// 400, 401/403, 409 and credential refusals are never retried, even when a producer marked them.
#[test]
fn final_classes_are_never_retried_even_when_marked() {
    let catalog = catalog(false);
    for code in [
        ErrorCode::Unauthorized,
        ErrorCode::Refused,
        ErrorCode::InvalidRequest,
        ErrorCode::TooLarge,
        ErrorCode::Unsupported,
        ErrorCode::Deadline,
    ] {
        let models = Fleet::new(&catalog, Some(failure(code, Dispatch::Rejected)), None, 0);
        let outcome = run(&catalog, FallbackPolicy::default(), &models);
        assert_eq!(attempted(&outcome.run), ["coding-primary"], "{code:?}");
        assert!(outcome.waits.is_empty(), "{code:?}");
        assert!(outcome.warnings.is_empty(), "{code:?}");
    }
}

/// H23: the retry classes compose with fallback. A target's attempts are spent first, then the
/// route falls back, whatever the class's dispatch evidence; the record keeps that evidence.
#[test]
fn retries_are_spent_on_one_target_then_the_route_falls_back() {
    let catalog = catalog(true);
    let models = Fleet::new(
        &catalog,
        Some(failure(ErrorCode::Transport, Dispatch::Unknown)),
        None,
        0,
    );
    let retry = RetryPolicy {
        max_attempts: 3,
        ..RetryPolicy::DEFAULT
    };
    let outcome = run(&catalog, with_retry(retry), &models);
    assert_eq!(outcome.run.halt, Halt::Succeeded);
    assert_eq!(
        attempted(&outcome.run),
        [
            "coding-primary",
            "coding-primary",
            "coding-primary",
            "coding-secondary"
        ]
    );
    for attempt in &outcome.run.attempts[..3] {
        assert!(
            matches!(
                attempt.result,
                AttemptResult::Failed {
                    code: ErrorCode::Transport,
                    dispatch: Dispatch::Unknown,
                    ..
                }
            ),
            "{:?}",
            attempt.result
        );
    }
    assert_eq!(outcome.waits, [secs(1), secs(2)]);
}

/// H8: a stream cut before anything was shown is retried although its dispatch is accepted.
#[test]
fn a_cut_stream_that_showed_nothing_is_retried() {
    let catalog = catalog(false);
    let models = Fleet::new(
        &catalog,
        Some(failure(ErrorCode::Protocol, Dispatch::Accepted)),
        Some(1),
        0,
    );
    let outcome = run(&catalog, FallbackPolicy::default(), &models);
    assert_eq!(outcome.run.halt, Halt::Succeeded);
    assert_eq!(
        attempted(&outcome.run),
        ["coding-primary", "coding-primary"]
    );
}

/// A retriable class falls back even with unknown dispatch; one attempt per target disables
/// same-target retry. An unmarked failure keeps today's rules: an eligible one falls back with no
/// same-target retry, and an unknown dispatch still halts as ambiguous.
#[test]
fn fallback_follows_the_retry_class_and_unmarked_failures_keep_todays_rules() {
    let catalog = catalog(true);
    let once = with_retry(RetryPolicy {
        max_attempts: 1,
        ..RetryPolicy::DEFAULT
    });
    let marked = Fleet::new(
        &catalog,
        Some(failure(ErrorCode::Transport, Dispatch::Unknown)),
        None,
        0,
    );
    let outcome = run(&catalog, once, &marked);
    assert_eq!(
        attempted(&outcome.run),
        ["coding-primary", "coding-secondary"]
    );
    assert!(outcome.waits.is_empty());

    let mut unsent = failure(ErrorCode::Transport, Dispatch::NotSent);
    unsent.retriable = false;
    let models = Fleet::new(&catalog, Some(unsent), None, 0);
    let outcome = run(&catalog, FallbackPolicy::default(), &models);
    assert_eq!(
        attempted(&outcome.run),
        ["coding-primary", "coding-secondary"]
    );
    assert!(outcome.waits.is_empty() && outcome.warnings.is_empty());

    let mut ambiguous = failure(ErrorCode::Transport, Dispatch::Unknown);
    ambiguous.retriable = false;
    let models = Fleet::new(&catalog, Some(ambiguous), None, 0);
    let outcome = run(&catalog, FallbackPolicy::default(), &models);
    assert_eq!(outcome.run.halt, Halt::AmbiguousDispatch);
    assert_eq!(attempted(&outcome.run), ["coding-primary"]);
}

/// The caller's limit decision is consulted before every attempt, retries included.
#[test]
fn admission_is_consulted_before_every_retry() {
    let catalog = catalog(false);
    let models = Fleet::new(
        &catalog,
        Some(failure(ErrorCode::Transport, Dispatch::Unknown)),
        None,
        0,
    );
    let mut admitted = 0;
    let mut admit = |_: &Selection<'_>| {
        admitted += 1;
        if admitted > 1 {
            Err(Error::new(ErrorCode::Refused, "caller limit exhausted"))
        } else {
            Ok(())
        }
    };
    let outcome = run_with(&catalog, FallbackPolicy::default(), &models, &mut admit).unwrap();
    assert_eq!(outcome.run.halt, Halt::LimitRefused);
    assert_eq!(attempted(&outcome.run), ["coding-primary"]);
    assert_eq!(admitted, 2);
}

/// No wait starts that would end at or after the caller's deadline.
#[test]
fn a_back_off_that_would_pass_the_deadline_is_not_waited() {
    let catalog = catalog(false);
    let models = Fleet::new(
        &catalog,
        Some(failure(ErrorCode::Transport, Dispatch::Unknown)),
        None,
        0,
    );
    let policy = FallbackPolicy {
        deadline: Some(Instant::now() + Duration::from_millis(200)),
        ..FallbackPolicy::default()
    };
    let outcome = run(&catalog, policy, &models);
    assert_eq!(outcome.run.halt, Halt::Deadline);
    assert_eq!(attempted(&outcome.run), ["coding-primary"]);
    assert!(outcome.waits.is_empty(), "{:?}", outcome.waits);
}

/// The retry bound is one through sixteen; anything else is refused before any attempt.
#[test]
fn a_retry_bound_outside_one_through_sixteen_is_refused() {
    let catalog = catalog(false);
    for (max_attempts, accepted) in [(0, false), (1, true), (16, true), (17, false)] {
        let models = Fleet::new(
            &catalog,
            Some(failure(ErrorCode::Transport, Dispatch::Unknown)),
            None,
            0,
        );
        let retry = RetryPolicy {
            max_attempts,
            backoff_base: Duration::ZERO,
            ..RetryPolicy::DEFAULT
        };
        let mut admit = |_: &Selection<'_>| Ok(());
        match run_with(&catalog, with_retry(retry), &models, &mut admit) {
            Ok(outcome) => {
                assert!(accepted, "{max_attempts} was accepted");
                assert_eq!(
                    outcome.run.attempts.len(),
                    usize::try_from(max_attempts).unwrap()
                );
            }
            Err(error) => {
                assert!(!accepted, "{max_attempts} was refused: {error}");
                assert_eq!(error.code, ErrorCode::InvalidRequest);
                assert_eq!(models.calls("local-small"), 0);
            }
        }
    }
}
