#![forbid(unsafe_code)]

//! One text-free record per routed model call, written to a sink the caller injects.
//!
//! [`record_turn`] runs a routed turn and writes its [`CallRecord`]; [`CallRecord::new`],
//! [`Stopwatch`] and [`Cost::quote`] build a record for a call that is not a turn.

mod record;

pub use record::{
    CallFacts, CallIdentity, CallLog, CallRecord, CallSink, Clock, Cost, Kind, Outcome, Role,
    Stopwatch, SystemClock, Timing,
};

use llm_core::{
    BoxFuture, Dispatch, Error, ErrorCode, Id, StreamEvent, StreamSink, TurnObservation,
    TurnRequest,
};
use llm_cost::{AttemptUsage, PriceBook};
use llm_routing::{Attempt, AttemptResult, Catalog, FallbackPolicy, FallbackRun, Halt, Ports};

/// Where a routed call's record goes and what it is measured and priced with.
#[derive(Clone, Copy)]
pub struct Recording<'r> {
    pub sink: &'r dyn CallSink,
    pub clock: &'r dyn Clock,
    /// Without a price book a record carries usage and no cost.
    pub prices: Option<&'r PriceBook>,
}

/// Runs [`Catalog::run_turn`], writes one record of the run to the caller's sink, and returns
/// the run unchanged.
///
/// The record's target, upstream model and usage are the last attempt's, its attempt count is
/// every attempt, and its outcome is the run's final result. Latency runs from the start to the
/// return; time to first event is to the first event offered to the caller's stream sink that is
/// not a warning. A turn the run refuses before it starts writes no record.
///
/// # Errors
/// Exactly the refusals of [`Catalog::run_turn`].
pub async fn record_turn(
    catalog: &Catalog,
    request: &TurnRequest,
    input_tokens: Option<u64>,
    policy: FallbackPolicy,
    ports: Ports<'_>,
    identity: CallIdentity,
    recording: Recording<'_>,
) -> Result<FallbackRun, Error> {
    let Ports {
        models,
        admit,
        sink,
        cancel,
        pause,
    } = ports;
    let mut timed = Timed {
        inner: sink,
        clock: recording.clock,
        stopwatch: Stopwatch::start(recording.clock),
    };
    let run = catalog
        .run_turn(
            request,
            input_tokens,
            policy,
            Ports {
                models,
                admit,
                sink: &mut timed,
                cancel,
                pause,
            },
        )
        .await?;
    let timing = timed.stopwatch.stop(recording.clock);
    recording
        .sink
        .record(turn_record(identity, &run, timing, recording.prices));
    Ok(run)
}

/// Passes every event on to the caller's sink and marks the first one that is output.
struct Timed<'t> {
    inner: &'t mut dyn StreamSink,
    clock: &'t dyn Clock,
    stopwatch: Stopwatch,
}

impl StreamSink for Timed<'_> {
    fn emit(&mut self, event: StreamEvent) -> BoxFuture<'_, Result<(), Error>> {
        // A warning is not output of an attempt, so a retry's warning starts no clock.
        if !matches!(event, StreamEvent::Warning { .. }) {
            self.stopwatch.first_event(self.clock);
        }
        self.inner.emit(event)
    }
}

fn observation(attempt: &Attempt) -> Option<&TurnObservation> {
    match &attempt.result {
        AttemptResult::Succeeded { observation } => Some(observation),
        AttemptResult::Failed { observation, .. } => observation.as_ref(),
    }
}

/// What the far side may have done with an attempt: a success was accepted.
const fn attempt_dispatch(attempt: &Attempt) -> Dispatch {
    match &attempt.result {
        AttemptResult::Succeeded { .. } => Dispatch::Accepted,
        AttemptResult::Failed { dispatch, .. } => *dispatch,
    }
}

/// Orders dispatch evidence by how much the far side may have done: not sent, rejected, unknown,
/// accepted.
const fn reach(dispatch: Dispatch) -> u8 {
    match dispatch {
        Dispatch::NotSent => 0,
        Dispatch::Rejected => 1,
        Dispatch::Unknown => 2,
        Dispatch::Accepted => 3,
    }
}

/// A failed call's dispatch: the furthest any attempt or the final failure may have reached. A
/// limit refusal, deadline or cancellation after an accepted attempt is itself not sent, and
/// does not make the call so.
fn call_dispatch(run: &FallbackRun, error: &Error) -> Dispatch {
    run.attempts
        .iter()
        .map(attempt_dispatch)
        .chain([error.dispatch])
        .max_by_key(|dispatch| reach(*dispatch))
        .unwrap_or(error.dispatch)
}

/// Every attempt that may have cost something, priced; `None` when none may have.
///
/// An attempt that was not sent and reported no usage cost nothing and is left out. Every other
/// attempt (accepted, rejected or of unknown dispatch, retried or fallen back from) is quoted, so
/// one with unknown usage leaves the call's cost incomplete.
fn call_cost(record_id: &Id, run: &FallbackRun, prices: &PriceBook) -> Option<Cost> {
    let usages: Vec<AttemptUsage> = run
        .attempts
        .iter()
        .filter(|attempt| {
            attempt_dispatch(attempt) != Dispatch::NotSent
                || observation(attempt).is_some_and(|observed| observed.usage.is_some())
        })
        .map(|attempt| attempt_usage(record_id, attempt, observation(attempt), prices))
        .collect();
    Cost::of_attempts(prices, usages)
}

fn turn_record(
    identity: CallIdentity,
    run: &FallbackRun,
    timing: Timing,
    prices: Option<&PriceBook>,
) -> CallRecord {
    let last = run.attempts.last();
    let observed = last.and_then(observation);
    let (outcome, error_code, dispatch) = match &run.result {
        Ok(_) => (Outcome::Succeeded, None, None),
        Err(error) => {
            let outcome = if run.halt == Halt::Cancelled || error.code == ErrorCode::Cancelled {
                Outcome::Cancelled
            } else {
                Outcome::Failed
            };
            (outcome, Some(error.code), Some(call_dispatch(run, error)))
        }
    };
    let cost = prices.and_then(|book| call_cost(&identity.id, run, book));
    CallRecord::new(
        identity,
        Kind::Turn,
        run.explanation.route_id.clone(),
        timing,
        CallFacts {
            route_target_id: last.map(|attempt| attempt.target_id.clone()),
            upstream_model: observed.and_then(|observed| observed.upstream_model.clone()),
            outcome,
            error_code,
            dispatch,
            attempts: u32::try_from(run.attempts.len()).unwrap_or(u32::MAX),
            usage: observed
                .and_then(|observed| observed.usage.clone())
                .unwrap_or_default(),
            cost,
        },
    )
}

/// The attempt as `llm-cost` prices it, under the price the book declares for its serving model
/// and binding revision (the one naming the reported upstream model first).
fn attempt_usage(
    record_id: &Id,
    attempt: &Attempt,
    observed: Option<&TurnObservation>,
    prices: &PriceBook,
) -> AttemptUsage {
    let upstream_model = observed.and_then(|observed| observed.upstream_model.clone());
    let declared = || {
        prices.document().models.iter().filter(|price| {
            price.serving_model_id == attempt.serving_model_id
                && price.binding_revision == attempt.provenance.binding_revision
        })
    };
    // No declared price leaves the serving model's id, which the book then reports unknown.
    let price_id = declared()
        .find(|price| upstream_model.as_ref() == Some(&price.upstream_model))
        .or_else(|| declared().next())
        .map_or_else(
            || attempt.serving_model_id.clone(),
            |price| price.id.clone(),
        );
    let dispatch = attempt_dispatch(attempt);
    let failed = matches!(attempt.result, AttemptResult::Failed { .. });
    AttemptUsage {
        id: record_id.clone(),
        usage_record_id: record_id.clone(),
        request_id: record_id.clone(),
        route_target_id: attempt.target_id.clone(),
        serving_model_id: attempt.serving_model_id.clone(),
        price_id,
        binding: attempt.provenance.clone(),
        upstream_model,
        billing: attempt.billing_kind,
        dispatch,
        failed,
        final_usage: observed.is_some_and(|observed| observed.final_usage),
        usage: observed
            .and_then(|observed| observed.usage.clone())
            .unwrap_or_default(),
    }
}
