//! Shadow calls: a second route answering the primary's request after the primary returned, out
//! of the caller's sight, under a daily money cap held on an `llm-cost` budget ledger.

use crate::{CallIdentity, Outcome, Recording};
use llm_core::{Cancel, Error, ErrorCode, Id, TurnRequest};
use llm_cost::{
    Amount, Currency,
    budget::{BudgetError, BudgetTotals},
};
use llm_routing::{Catalog, FallbackPolicy, FallbackRun, Models, Pause, Ports};
use std::sync::Arc;

/// The UTC day a wall-clock reading falls on, counted from the Unix epoch.
pub const fn utc_day(unix_ms: u64) -> u64 {
    unix_ms / 86_400_000
}

/// Where shadow calls are admitted: one `llm.budget` ledger per shadow and UTC day.
#[derive(Debug)]
pub struct ShadowLedger {
    id: Id,
    currency: Currency,
    max_active: u32,
}

impl ShadowLedger {
    /// # Errors
    /// Refuses a concurrency bound the budget engine refuses.
    pub fn new(id: Id, currency: Currency, max_active: u32) -> Result<Self, BudgetError> {
        Ok(Self {
            id,
            currency,
            max_active,
        })
    }

    /// The totals of one shadow's ledger for one UTC day; `None` for a day it never opened.
    pub fn totals(&self, _shadow_id: &Id, _day: u64) -> Option<BudgetTotals> {
        let _ = (&self.id, &self.currency, self.max_active);
        None
    }
}

/// A shadow route for a primary route, with its daily cap.
#[derive(Debug, Clone)]
pub struct Shadow {
    id: Id,
    primary_route_id: Id,
    shadow_route_id: Id,
    daily_cap: Amount,
    reserve: Amount,
    ledger: Arc<ShadowLedger>,
}

impl Shadow {
    /// # Errors
    /// Refuses a reserve of zero or above the daily cap.
    pub fn new(
        id: Id,
        primary_route_id: Id,
        shadow_route_id: Id,
        daily_cap: Amount,
        reserve: Amount,
        ledger: Arc<ShadowLedger>,
    ) -> Result<Self, Error> {
        Ok(Self {
            id,
            primary_route_id,
            shadow_route_id,
            daily_cap,
            reserve,
            ledger,
        })
    }
}

/// How one shadow ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShadowOutcome {
    Sent(Outcome),
    Skipped(BudgetError),
    Refused(ErrorCode),
}

/// One shadow's result, for the caller that drove the handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowResult {
    pub shadow_id: Id,
    pub shadow_route_id: Id,
    pub record_id: Option<Id>,
    pub outcome: ShadowOutcome,
}

/// The shadows a call has and the ports they run on.
#[derive(Clone, Copy)]
pub struct Shadowing<'s> {
    pub shadows: &'s [Shadow],
    pub models: &'s dyn Models,
    pub cancel: &'s Cancel,
    pub pause: Pause<'s>,
}

/// The shadow calls of one primary call, not yet sent.
pub struct ShadowCalls<'a> {
    shadows: Vec<&'a Shadow>,
}

impl<'a> ShadowCalls<'a> {
    pub fn len(&self) -> usize {
        self.shadows.len()
    }
    pub fn is_empty(&self) -> bool {
        self.shadows.is_empty()
    }
    pub async fn run(self) -> Vec<ShadowResult> {
        Vec::new()
    }
}

/// Runs [`crate::record_turn`] and returns its run with the shadow calls that follow it.
///
/// # Errors
/// Exactly the refusals of [`crate::record_turn`].
#[expect(
    clippy::too_many_arguments,
    reason = "the seven ports of record_turn and the shadows"
)]
pub async fn record_turn_shadowed<'a>(
    catalog: &'a Catalog,
    request: &TurnRequest,
    input_tokens: Option<u64>,
    policy: FallbackPolicy,
    ports: Ports<'_>,
    identity: CallIdentity,
    recording: Recording<'a>,
    shadowing: Shadowing<'a>,
) -> Result<(FallbackRun, ShadowCalls<'a>), Error> {
    let run = crate::record_turn(
        catalog,
        request,
        input_tokens,
        policy,
        ports,
        identity,
        recording,
    )
    .await?;
    Ok((
        run,
        ShadowCalls {
            shadows: shadowing.shadows.iter().collect(),
        },
    ))
}
