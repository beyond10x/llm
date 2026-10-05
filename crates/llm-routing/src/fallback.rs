//! Explicit ordered fallback over single-attempt `Model` ports, with same-target retry.
//!
//! Only the route's declared, compatible, admitted targets are attempted, in declared order.
//! A target is attempted again, up to the retry policy's count, only after a failure whose class
//! may be retried ([`Error::may_retry`]) and that left nothing visible. The run moves to the next
//! target only after a failure that left nothing visible and is either such a class or was
//! provably not accepted. Every attempt is recorded with its own dispatch evidence.

use crate::{Catalog, MAX_ROUTE_TARGETS, RouteExplanation, Selection};
use llm_core::{
    AuthKind, BillingKind, BoxFuture, Cancel, Dispatch, Error, ErrorCode, Id, Model, Provenance,
    StreamEvent, StreamSink, TurnObservation, TurnOutcome, TurnRequest,
};
use std::{
    future::{Future, poll_fn},
    pin::pin,
    task::Poll,
    time::{Duration, Instant},
};

/// Supplies the single-attempt port that serves a declared serving model.
pub trait Models: Sync {
    fn model(&self, serving_model_id: &Id) -> Option<&dyn Model>;
}

/// The widest same-target retry bound, the first attempt included.
pub const MAX_RETRY_ATTEMPTS: u32 = 16;

/// The warning code every same-target retry states on the caller's sink before its wait.
pub const RETRY_WARNING: &str = "turn-retried";

/// How many attempts one target gets and how the waits between them grow (Harness
/// `RetryPolicy`, `harness-http/src/retry.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Attempts per target, the first included; one disables same-target retry.
    pub max_attempts: u32,
    /// Doubled once per attempt already made, so the wait before the second attempt is twice it.
    pub backoff_base: Duration,
    /// Doublings after which the wait stops growing.
    pub max_doublings: u32,
    /// The longest server-requested delay honoured.
    pub max_server_delay: Duration,
}

impl RetryPolicy {
    /// Harness's: four attempts, waiting 1 s, 2 s and 4 s (8 s at most), server delays capped
    /// at 30 s.
    pub const DEFAULT: Self = Self {
        max_attempts: 4,
        backoff_base: Duration::from_millis(500),
        max_doublings: 4,
        max_server_delay: Duration::from_secs(30),
    };

    /// One attempt per target.
    pub const fn disabled() -> Self {
        Self {
            max_attempts: 1,
            ..Self::DEFAULT
        }
    }

    /// The local wait after `attempt` attempts were made, doubling and capped.
    pub fn backoff(self, attempt: u32) -> Duration {
        self.backoff_base
            .saturating_mul(2_u32.saturating_pow(attempt.min(self.max_doublings)))
    }

    /// The wait after `attempt` attempts: a server delay is honoured up to the cap and never
    /// shortens the local back-off.
    pub fn delay(self, attempt: u32, server: Option<Duration>) -> Duration {
        let local = self.backoff(attempt);
        server.map_or(local, |requested| {
            requested.min(self.max_server_delay).max(local)
        })
    }

    /// # Errors
    /// Refuses an attempt bound outside one through [`MAX_RETRY_ATTEMPTS`].
    pub fn validate(&self) -> Result<(), Error> {
        if self.max_attempts == 0 || self.max_attempts > MAX_RETRY_ATTEMPTS {
            return Err(Error::invalid(
                "retry attempt bound must be between one and 16",
            ));
        }
        Ok(())
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Caller-owned bounds. `max_attempts == 1` disables fallback regardless of the route; it
/// counts targets, and `retry` bounds the attempts on each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FallbackPolicy {
    pub max_attempts: usize,
    /// No attempt starts, and no retry wait begins that would end, at or after this instant.
    pub deadline: Option<Instant>,
    /// Same-target retry; on by default with Harness's policy.
    pub retry: RetryPolicy,
}

impl Default for FallbackPolicy {
    fn default() -> Self {
        Self {
            max_attempts: MAX_ROUTE_TARGETS,
            deadline: None,
            retry: RetryPolicy::DEFAULT,
        }
    }
}

impl FallbackPolicy {
    /// Attempt only the first compatible target. Same-target retry stays on.
    pub const fn disabled() -> Self {
        Self {
            max_attempts: 1,
            deadline: None,
            retry: RetryPolicy::DEFAULT,
        }
    }

    /// # Errors
    /// Refuses an attempt bound outside one through the route target bound, or an invalid retry
    /// policy.
    pub fn validate(&self) -> Result<(), Error> {
        if self.max_attempts == 0 || self.max_attempts > MAX_ROUTE_TARGETS {
            return Err(Error::invalid(
                "fallback attempt bound must be between one and 64",
            ));
        }
        self.retry.validate()
    }
}

/// What one started attempt returned. Evidence is kept for failures as well as successes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptResult {
    Succeeded {
        observation: TurnObservation,
    },
    Failed {
        code: ErrorCode,
        dispatch: Dispatch,
        observation: Option<TurnObservation>,
    },
}

/// One started attempt against one declared route target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub target_id: Id,
    pub serving_model_id: Id,
    pub position: usize,
    pub provenance: Provenance,
    pub auth_kind: AuthKind,
    pub billing_kind: BillingKind,
    /// Stream events offered to the caller's sink during this attempt.
    pub visible_events: usize,
    pub result: AttemptResult,
}

/// Why the run stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Halt {
    Succeeded,
    /// No compatible target remained; the explanation names every rejection.
    Exhausted,
    /// The failure class is not eligible for fallback.
    IneligibleFailure,
    /// Output had already reached the caller.
    VisibleOutput,
    /// The request may have been accepted, so it is never replayed elsewhere.
    AmbiguousDispatch,
    /// The caller's admission check refused the next target.
    LimitRefused,
    Deadline,
    Cancelled,
    /// The caller's attempt bound was reached.
    AttemptBound,
    /// The caller's sink refused a retry warning; the run ends with the sink's error.
    SinkRefused,
}

impl Halt {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Exhausted => "exhausted",
            Self::IneligibleFailure => "ineligible-failure",
            Self::VisibleOutput => "visible-output",
            Self::AmbiguousDispatch => "ambiguous-dispatch",
            Self::LimitRefused => "limit-refused",
            Self::Deadline => "deadline",
            Self::Cancelled => "cancelled",
            Self::AttemptBound => "attempt-bound",
            Self::SinkRefused => "sink-refused",
        }
    }
}

/// The ordered record of a fallback run and its final result.
#[derive(Debug)]
pub struct FallbackRun {
    pub explanation: RouteExplanation,
    pub attempts: Vec<Attempt>,
    pub halt: Halt,
    pub result: Result<TurnOutcome, Error>,
}

struct Counting<'s> {
    inner: &'s mut dyn StreamSink,
    offered: usize,
}

impl StreamSink for Counting<'_> {
    fn emit(&mut self, event: StreamEvent) -> BoxFuture<'_, Result<(), Error>> {
        self.offered = self.offered.saturating_add(1);
        self.inner.emit(event)
    }
}

const fn eligible(error: &Error) -> bool {
    matches!(error.dispatch, Dispatch::NotSent | Dispatch::Rejected)
        && matches!(
            error.code,
            ErrorCode::Transport | ErrorCode::RateLimited | ErrorCode::Unavailable
        )
}

/// Waits the given duration; the run races it against cancellation itself.
pub type Pause<'p> = &'p (dyn Fn(Duration) -> BoxFuture<'static, ()> + Sync);

/// The caller's side of a run: model ports, limit decision, output sink, cancellation and the
/// clock a retry waits on.
pub struct Ports<'p> {
    pub models: &'p dyn Models,
    /// The caller's limit decision, consulted before every attempt including the first and
    /// every retry.
    pub admit: &'p mut (dyn FnMut(&Selection<'_>) -> Result<(), Error> + Send),
    pub sink: &'p mut dyn StreamSink,
    pub cancel: &'p Cancel,
    /// Waits between attempts on one target. Routing owns no timer, so it runs on any executor;
    /// a Tokio caller passes `&|wait| Box::pin(tokio::time::sleep(wait))`.
    pub pause: Pause<'p>,
}

/// The kebab-case name of a failure code, as it appears on the wire.
fn code_label(code: ErrorCode) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Waits `wait` through the caller's pause port unless the caller cancels first. Returns whether
/// the caller cancelled.
async fn pause_unless_cancelled(pause: Pause<'_>, wait: Duration, cancel: &Cancel) -> bool {
    let mut waiting = pause(wait);
    let mut cancelled = pin!(cancel.cancelled());
    poll_fn(|context| {
        if cancelled.as_mut().poll(context).is_ready() {
            return Poll::Ready(true);
        }
        if waiting.as_mut().poll(context).is_ready() {
            return Poll::Ready(false);
        }
        Poll::Pending
    })
    .await
}

/// Clears the class of a failure the run retried as far as it will, naming the count, so a
/// caller's own loop does not multiply the attempts (Harness `RetryPolicy::exhausted`).
fn exhausted(result: Result<TurnOutcome, Error>, tries: u32) -> Result<TurnOutcome, Error> {
    result.map_err(|mut error| {
        if error.may_retry() {
            error.retriable = false;
            let unit = if tries == 1 { "attempt" } else { "attempts" };
            error.message = format!("{} (after {tries} {unit})", error.message);
        }
        error
    })
}

impl Catalog {
    /// Run a turn over the route's declared compatible targets in order, falling back only after
    /// an eligible failure that exposed no output.
    /// # Errors
    /// Refuses invalid input, unknown aliases, invalid policies and missing or mismatched models
    /// before any attempt. Every later outcome, including refusal, is reported in the run.
    pub async fn run_turn(
        &self,
        request: &TurnRequest,
        input_tokens: Option<u64>,
        policy: FallbackPolicy,
        ports: Ports<'_>,
    ) -> Result<FallbackRun, Error> {
        let Ports {
            models,
            admit,
            sink,
            cancel,
            pause,
        } = ports;
        policy.validate()?;
        let explanation = self.explain(request, input_tokens)?;
        // Only targets the run could attempt are bound and checked for a model.
        let mut candidates = Vec::new();
        let mut bounded = false;
        for target in &explanation.targets {
            if !target.rejections.is_empty() {
                continue;
            }
            if candidates.len() >= policy.max_attempts {
                bounded = true;
                break;
            }
            let selection = self.select(&explanation.alias, &target.target_id, request)?;
            let model = models
                .model(&selection.target.serving_model_id)
                .ok_or_else(|| Error::invalid("no model serves a compatible route target"))?;
            if model.provenance() != selection.binding.provenance() {
                return Err(Error::invalid(
                    "model does not serve its route target's binding",
                ));
            }
            candidates.push((selection, model));
        }
        let retry = policy.retry;
        let mut attempts = Vec::new();
        let mut last = Err(no_compatible_target(&explanation));
        let mut last_tries = 0;
        for (selection, model) in candidates {
            let mut tries: u32 = 0;
            loop {
                if let Err((halt, error)) = may_start(cancel, &policy, admit, &selection) {
                    return Ok(finish(explanation, attempts, halt, Err(error)));
                }
                let mut counting = Counting {
                    inner: &mut *sink,
                    offered: 0,
                };
                let returned = model.turn(&selection.request, &mut counting, cancel).await;
                let visible_events = counting.offered;
                let (recorded, halt, result) = settle(returned, &selection, visible_events);
                attempts.push(attempt(&selection, visible_events, recorded));
                tries = tries.saturating_add(1);
                if let Some(halt) = halt {
                    return Ok(finish(explanation, attempts, halt, result));
                }
                // Only an eligible failure that showed nothing reaches here.
                let Err(error) = &result else {
                    return Ok(finish(explanation, attempts, Halt::Succeeded, result));
                };
                if !error.may_retry() || tries >= retry.max_attempts {
                    last = result;
                    last_tries = tries;
                    break;
                }
                let waited = wait_to_retry(
                    &policy,
                    tries,
                    error,
                    selection.target.id.as_str(),
                    Waiting {
                        sink: &mut *sink,
                        cancel,
                        pause,
                    },
                )
                .await;
                if let Err((halt, error)) = waited {
                    return Ok(finish(explanation, attempts, halt, Err(error)));
                }
            }
        }
        let halt = if bounded {
            Halt::AttemptBound
        } else {
            Halt::Exhausted
        };
        Ok(finish(
            explanation,
            attempts,
            halt,
            exhausted(last, last_tries),
        ))
    }
}

fn deadline_reached() -> Error {
    Error::new(ErrorCode::Deadline, "fallback deadline reached")
}

/// The checks before every attempt, first or retry: cancellation, the caller's deadline, then
/// the caller's limit decision.
fn may_start(
    cancel: &Cancel,
    policy: &FallbackPolicy,
    admit: &mut (dyn FnMut(&Selection<'_>) -> Result<(), Error> + Send),
    selection: &Selection<'_>,
) -> Result<(), (Halt, Error)> {
    if cancel.is_cancelled() {
        return Err((Halt::Cancelled, Error::cancelled()));
    }
    if policy
        .deadline
        .is_some_and(|deadline| Instant::now() >= deadline)
    {
        return Err((Halt::Deadline, deadline_reached()));
    }
    admit(selection).map_err(|error| (Halt::LimitRefused, error))
}

/// The caller's ports a retry wait uses.
struct Waiting<'w> {
    sink: &'w mut dyn StreamSink,
    cancel: &'w Cancel,
    pause: Pause<'w>,
}

/// Waits before attempt `tries + 1` on `target` after `error`, stating the retry first. Refuses
/// with the run's halt when the wait would pass the caller's deadline, the sink refuses the
/// warning, or the caller cancels during the wait.
async fn wait_to_retry(
    policy: &FallbackPolicy,
    tries: u32,
    error: &Error,
    target: &str,
    ports: Waiting<'_>,
) -> Result<(), (Halt, Error)> {
    let wait = policy
        .retry
        .delay(tries, error.retry_after_ms.map(Duration::from_millis));
    if policy.deadline.is_some_and(|deadline| {
        Instant::now()
            .checked_add(wait)
            .is_none_or(|end| end >= deadline)
    }) {
        return Err((Halt::Deadline, deadline_reached()));
    }
    let warning = StreamEvent::Warning {
        code: RETRY_WARNING.to_owned(),
        message: format!(
            "attempt {tries} of {} on {target} failed before any output was visible ({}) and is \
             retried after {} ms",
            policy.retry.max_attempts,
            code_label(error.code),
            wait.as_millis()
        ),
    };
    ports
        .sink
        .emit(warning)
        .await
        .map_err(|refusal| (Halt::SinkRefused, refusal))?;
    if pause_unless_cancelled(ports.pause, wait, ports.cancel).await {
        return Err((Halt::Cancelled, Error::cancelled()));
    }
    Ok(())
}

/// Classify one attempt's return. `None` permits trying the next declared target.
fn settle(
    returned: Result<TurnOutcome, Error>,
    selection: &Selection<'_>,
    visible_events: usize,
) -> (AttemptResult, Option<Halt>, Result<TurnOutcome, Error>) {
    let provenance = selection.binding.provenance();
    match returned {
        Ok(outcome) => match outcome.validate_for(&selection.request, provenance) {
            Ok(()) => (
                AttemptResult::Succeeded {
                    observation: outcome.observation.clone(),
                },
                Some(Halt::Succeeded),
                Ok(outcome),
            ),
            Err(error) => {
                let mut error = error.with_dispatch(Dispatch::Accepted);
                // A refused outcome still reported its attempt's evidence; keep it when bound.
                if outcome.observation.validate_for(provenance).is_ok() {
                    error = error.with_observation(outcome.observation);
                }
                (failed(&error), Some(Halt::IneligibleFailure), Err(error))
            }
        },
        Err(mut error) => {
            let halt = if error.validate_for(provenance).is_err() {
                // Contradictory evidence proves nothing about dispatch. Foreign evidence is not
                // this attempt's; bound evidence is kept even when its dispatch claim is not.
                if error
                    .observation
                    .as_ref()
                    .is_some_and(|observation| observation.validate_for(provenance).is_err())
                {
                    error.observation = None;
                }
                error.dispatch = Dispatch::Unknown;
                Some(Halt::AmbiguousDispatch)
            } else if visible_events > 0 {
                Some(Halt::VisibleOutput)
            } else if error.may_retry() {
                // A retriable class showed nothing: retried on this target, then eligible for
                // the next, whatever its dispatch. The record keeps that dispatch, so a possibly
                // billed attempt is recorded rather than assumed free.
                None
            } else if error.dispatch == Dispatch::Unknown {
                Some(Halt::AmbiguousDispatch)
            } else if eligible(&error) {
                None
            } else {
                Some(Halt::IneligibleFailure)
            };
            (failed(&error), halt, Err(error))
        }
    }
}

fn failed(error: &Error) -> AttemptResult {
    AttemptResult::Failed {
        code: error.code,
        dispatch: error.dispatch,
        observation: error.observation.as_deref().cloned(),
    }
}

fn attempt(selection: &Selection<'_>, visible_events: usize, result: AttemptResult) -> Attempt {
    let account = &selection.binding.declaration().account;
    Attempt {
        target_id: selection.target.id.clone(),
        serving_model_id: selection.target.serving_model_id.clone(),
        position: selection.target.position,
        provenance: selection.binding.provenance().clone(),
        auth_kind: account.auth_kind,
        billing_kind: account.billing_kind,
        visible_events,
        result,
    }
}

const fn finish(
    explanation: RouteExplanation,
    attempts: Vec<Attempt>,
    halt: Halt,
    result: Result<TurnOutcome, Error>,
) -> FallbackRun {
    FallbackRun {
        explanation,
        attempts,
        halt,
        result,
    }
}

pub(crate) fn no_compatible_target(explanation: &RouteExplanation) -> Error {
    let reasons = explanation
        .targets
        .iter()
        .flat_map(|target| target.rejections.iter().map(|reason| reason.label()))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(", ");
    Error::unsupported(format!("route has no compatible target: {reasons}"))
}
