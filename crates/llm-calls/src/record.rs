//! The call record, the two ports a caller injects (sink and clock), and the pieces a record is
//! built from: a stopwatch for its timings and a priced cost for its usage.
//!
//! No field of [`CallRecord`] holds text: every one is an identifier, a closed set, a count, a
//! time or an exact amount. A failure is its code, never its message.

use llm_core::{Dispatch, ErrorCode, Id, Usage};
use llm_cost::{AttemptUsage, PriceBook};
use serde::{Deserialize, Serialize};
use std::{
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Why a call was made: the caller's own call, or a second call compared with one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Primary,
    Shadow,
    Replay,
}

/// What the call asked the model for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Turn,
    Classification,
    Verdict,
    EmbedBatch,
}

/// How the call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    Succeeded,
    Failed,
    Cancelled,
    /// Not sent because a cap would have been passed (a shadow or replay call).
    Skipped,
}

/// The call's priced usage under the caller's price book. `complete` is false when any line was
/// unknown; `known_subtotal` then sums only the known lines and is a lower bound, never a total.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cost {
    pub currency: llm_cost::Currency,
    pub known_subtotal: llm_cost::Amount,
    pub complete: bool,
    pub price_revision: Id,
}

impl Cost {
    /// Prices one attempt's usage. `None` when the price book cannot quote it at all.
    pub fn quote(prices: &PriceBook, usage: AttemptUsage) -> Option<Self> {
        let input = llm_cost::AccountingInput::new(vec![llm_cost::Observation::Attempt(usage)]);
        let report = prices.quote(&input).ok()?;
        let total = &report.records.first()?.total;
        Some(Self {
            currency: report.currency.clone(),
            known_subtotal: total.known_subtotal,
            complete: total.complete_total.is_some(),
            price_revision: report.price_revision.clone(),
        })
    }
}

/// One model call, as `llm.calls.CallRecord` declares it. It has no text field of any kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallRecord {
    pub id: Id,
    pub role: Role,
    pub kind: Kind,
    pub route_id: Id,
    /// The target that answered or failed last; absent when no target was attempted.
    pub route_target_id: Option<Id>,
    /// The model the far side reported, not the alias asked for; absent when it reported none.
    pub upstream_model: Option<Id>,
    pub outcome: Outcome,
    pub error_code: Option<ErrorCode>,
    /// The failure's dispatch evidence; absent when the call succeeded.
    pub dispatch: Option<Dispatch>,
    pub attempts: u32,
    pub started_at_unix_ms: u64,
    pub latency_ms: Option<u64>,
    pub time_to_first_event_ms: Option<u64>,
    /// The counters reported for the last attempt; a counter never reported stays absent.
    pub usage: Usage,
    /// Absent when the caller supplied no price book or no attempt was made.
    pub cost: Option<Cost>,
    /// The primary call a shadow or replay call is compared with.
    pub primary_record_id: Option<Id>,
}

/// Who asked for the call and as what; the rest of a record comes from the call itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallIdentity {
    pub id: Id,
    pub role: Role,
    pub primary_record_id: Option<Id>,
}

impl CallIdentity {
    /// The caller's own call.
    pub const fn primary(id: Id) -> Self {
        Self {
            id,
            role: Role::Primary,
            primary_record_id: None,
        }
    }
}

/// What the call itself reported, for [`CallRecord::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallFacts {
    pub route_target_id: Option<Id>,
    pub upstream_model: Option<Id>,
    pub outcome: Outcome,
    pub error_code: Option<ErrorCode>,
    pub dispatch: Option<Dispatch>,
    pub attempts: u32,
    pub usage: Usage,
    pub cost: Option<Cost>,
}

/// When the call started and how long it took, from [`Stopwatch::stop`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    pub started_at_unix_ms: u64,
    pub latency_ms: Option<u64>,
    pub time_to_first_event_ms: Option<u64>,
}

impl CallRecord {
    /// Builds a record of any kind; [`crate::record_turn`] is this for a routed turn.
    pub fn new(
        identity: CallIdentity,
        kind: Kind,
        route_id: Id,
        timing: Timing,
        facts: CallFacts,
    ) -> Self {
        Self {
            id: identity.id,
            role: identity.role,
            kind,
            route_id,
            route_target_id: facts.route_target_id,
            upstream_model: facts.upstream_model,
            outcome: facts.outcome,
            error_code: facts.error_code,
            dispatch: facts.dispatch,
            attempts: facts.attempts,
            started_at_unix_ms: timing.started_at_unix_ms,
            latency_ms: timing.latency_ms,
            time_to_first_event_ms: timing.time_to_first_event_ms,
            usage: facts.usage,
            cost: facts.cost,
            primary_record_id: identity.primary_record_id,
        }
    }
}

/// Where records go. Recording cannot fail and returns nothing, so a sink can never change what
/// the caller receives; a sink that must report its own trouble does so elsewhere.
pub trait CallSink: Send + Sync {
    fn record(&self, record: CallRecord);
}

/// Keeps every record in memory, in order.
#[derive(Debug, Default)]
pub struct CallLog(Mutex<Vec<CallRecord>>);

impl CallLog {
    pub const fn new() -> Self {
        Self(Mutex::new(Vec::new()))
    }
    /// A copy of every record so far.
    pub fn records(&self) -> Vec<CallRecord> {
        self.0
            .lock()
            .map_or_else(|poisoned| poisoned.into_inner().clone(), |held| held.clone())
    }
}

impl CallSink for CallLog {
    fn record(&self, record: CallRecord) {
        match self.0.lock() {
            Ok(mut held) => held.push(record),
            Err(poisoned) => poisoned.into_inner().push(record),
        }
    }
}

/// The time a record reads: wall-clock milliseconds for when a call started, and a monotonic
/// reading for how long it took. Tests script it.
pub trait Clock: Send + Sync {
    fn unix_ms(&self) -> u64;
    /// Time since an origin of the clock's choosing; never goes back.
    fn monotonic(&self) -> Duration;
}

/// The system's clocks: `SystemTime` for the start, `Instant` for durations.
#[derive(Debug, Clone, Copy)]
pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn unix_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| millis(since))
    }
    fn monotonic(&self) -> Duration {
        self.origin.elapsed()
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Measures one call: started when made, marked at the first output event, stopped at the end.
#[derive(Debug, Clone, Copy)]
pub struct Stopwatch {
    started_at_unix_ms: u64,
    start: Duration,
    first_event: Option<Duration>,
}

impl Stopwatch {
    pub fn start(clock: &dyn Clock) -> Self {
        Self {
            started_at_unix_ms: clock.unix_ms(),
            start: clock.monotonic(),
            first_event: None,
        }
    }
    /// Marks the first output event; a later mark changes nothing.
    pub fn first_event(&mut self, clock: &dyn Clock) {
        if self.first_event.is_none() {
            self.first_event = Some(clock.monotonic());
        }
    }
    pub fn stop(self, clock: &dyn Clock) -> Timing {
        let since = |at: Duration| millis(at.saturating_sub(self.start));
        Timing {
            started_at_unix_ms: self.started_at_unix_ms,
            latency_ms: Some(since(clock.monotonic())),
            time_to_first_event_ms: self.first_event.map(since),
        }
    }
}
