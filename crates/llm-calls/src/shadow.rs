//! Shadow calls: a second route answering the primary's request after the primary returned, out
//! of the caller's sight, under a daily money cap held on an `llm-cost` budget ledger.
//!
//! [`record_turn_shadowed`] runs the primary call exactly as [`crate::record_turn`] does and
//! returns its run together with a [`ShadowCalls`] handle that has sent nothing yet. The caller
//! answers its own caller first and then drives the handle on any executor it likes, or drops
//! it, which sends nothing and holds nothing: llm owns no runtime, as `Ports::pause` owns no
//! timer. A shadow's events go to no sink; its answer reaches only its [`CallRecord`].
//!
//! Each shadow call is admitted on its [`ShadowLedger`]: one `llm-cost` [`BudgetEngine`] per
//! shadow and UTC day, whose limit is the shadow's daily cap. Before the call is sent, its reserve
//! is reserved and started in one step under the ledger's lock, so two shadows can never both
//! take the last room under the cap. Afterwards the reservation is settled with the priced cost of
//! every attempt; a cost that is incomplete or not in the ledger's currency is held at the
//! reservation and never settled as zero, and the engine then admits no further shadow that day.

use crate::{CallFacts, CallIdentity, CallRecord, Cost, Kind, Outcome, Recording, Role, Timing};
use llm_core::{
    BoxFuture, Cancel, Dispatch, Error, ErrorCode, Id, StreamEvent, StreamSink, TurnRequest, Usage,
};
use llm_cost::{
    Amount, Currency,
    budget::{
        BudgetCommand, BudgetEngine, BudgetError, BudgetPolicy, BudgetTotals, Operation,
        ReservationRequest, Settlement,
    },
};
use llm_routing::{Catalog, FallbackPolicy, FallbackRun, Models, Pause, Ports, Selection};
use std::{
    collections::{BTreeMap, btree_map::Entry},
    sync::{Arc, Mutex, PoisonError},
};

const DAY_MS: u64 = 86_400_000;

/// The UTC day a wall-clock reading falls on, counted from the Unix epoch.
pub const fn utc_day(unix_ms: u64) -> u64 {
    unix_ms / DAY_MS
}

/// Where shadow calls are admitted: one `llm.budget` ledger per shadow and UTC day, its limit the
/// shadow's daily cap. Keep one for as long as the shadows it admits run: a new ledger starts
/// every day's cap afresh.
#[derive(Debug)]
pub struct ShadowLedger {
    id: Id,
    currency: Currency,
    max_active: u32,
    days: Mutex<Days>,
}

#[derive(Debug, Default)]
struct Days {
    /// The latest UTC day a shadow was admitted on; no earlier day opens again.
    latest: Option<u64>,
    engines: BTreeMap<(Id, u64), BudgetEngine>,
}

impl ShadowLedger {
    /// A ledger in `currency` that holds at most `max_active` unsettled shadow calls per shadow
    /// and day.
    ///
    /// # Errors
    /// Refuses a concurrency bound the budget engine refuses.
    pub fn new(id: Id, currency: Currency, max_active: u32) -> Result<Self, BudgetError> {
        BudgetEngine::new(BudgetPolicy {
            id: id.clone(),
            currency: currency.clone(),
            limit: Amount::ZERO,
            max_active,
        })?;
        Ok(Self {
            id,
            currency,
            max_active,
            days: Mutex::new(Days::default()),
        })
    }
    pub const fn id(&self) -> &Id {
        &self.id
    }
    pub const fn currency(&self) -> &Currency {
        &self.currency
    }

    /// One shadow's totals for one UTC day: settled, held and their sum, the exposure the cap
    /// is checked against. `None` for a day the shadow was never admitted on, or one forgotten:
    /// a day before yesterday is dropped once it holds nothing unsettled.
    pub fn totals(&self, shadow_id: &Id, day: u64) -> Option<BudgetTotals> {
        let days = self.days.lock().unwrap_or_else(PoisonError::into_inner);
        let engine = days.engines.get(&(shadow_id.clone(), day))?;
        engine.view().ok().map(|view| view.totals)
    }

    /// Checks the cap and holds the reservation in one step, then marks it started.
    fn open(
        &self,
        shadow: &Shadow,
        day: u64,
        now_ms: u64,
        request: ReservationRequest,
    ) -> Result<(), BudgetError> {
        let mut days = self.days.lock().map_err(|_| BudgetError::Storage)?;
        if days.latest.is_some_and(|latest| day < latest) {
            return Err(BudgetError::ClockReversed);
        }
        days.latest = Some(day);
        days.engines
            .retain(|(_, opened), engine| opened.saturating_add(1) >= day || !idle(engine));
        let policy = BudgetPolicy {
            id: self.id.clone(),
            currency: self.currency.clone(),
            limit: shadow.daily_cap,
            max_active: self.max_active,
        };
        let engine = match days.engines.entry((shadow.id.clone(), day)) {
            Entry::Occupied(held) if held.get().policy() != &policy => {
                return Err(BudgetError::PolicyMismatch);
            }
            Entry::Occupied(held) => held.into_mut(),
            Entry::Vacant(free) => free.insert(BudgetEngine::new(policy)?),
        };
        let now = now_ms.max(engine.now_ms());
        let id = request.id.clone();
        engine.apply(now, BudgetCommand::Reserve { request })?;
        if let Err(error) = engine.apply(now, BudgetCommand::Begin { id: id.clone() }) {
            // An unstarted reservation is freed; if even that fails it stays held.
            let _ = engine.apply(now, BudgetCommand::Cancel { id });
            return Err(error);
        }
        Ok(())
    }

    /// Settles a started reservation. One the engine refuses stays held at its reservation.
    fn settle(&self, shadow_id: &Id, day: u64, now_ms: u64, id: Id, settlement: Settlement) {
        let mut days = self.days.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(engine) = days.engines.get_mut(&(shadow_id.clone(), day)) {
            let now = now_ms.max(engine.now_ms());
            let _ = engine.apply(now, BudgetCommand::Settle { id, settlement });
        }
    }
}

/// A day ledger with nothing unsettled in it.
fn idle(engine: &BudgetEngine) -> bool {
    engine
        .view()
        .is_ok_and(|view| view.totals.active_count == 0 && view.totals.uncertain_count == 0)
}

/// A shadow route for a primary route, with its daily cap in its ledger's currency and the
/// amount each of its calls holds on the ledger until its priced cost settles it.
#[derive(Debug, Clone)]
pub struct Shadow {
    id: Id,
    primary_route_id: Id,
    route_id: Id,
    daily_cap: Amount,
    reserve: Amount,
    ledger: Arc<ShadowLedger>,
}

impl Shadow {
    /// The reserve is the explicit forecast bound of one shadow call. A call that costs more is
    /// settled at what it cost, and the cap then admits correspondingly less.
    ///
    /// # Errors
    /// Refuses a reserve of zero or above the daily cap, which would admit unbounded or no calls.
    pub fn new(
        id: Id,
        primary_route_id: Id,
        shadow_route_id: Id,
        daily_cap: Amount,
        reserve: Amount,
        ledger: Arc<ShadowLedger>,
    ) -> Result<Self, Error> {
        if reserve == Amount::ZERO {
            return Err(Error::invalid("a shadow call must reserve more than zero"));
        }
        if reserve > daily_cap {
            return Err(Error::invalid(
                "a shadow call must not reserve more than the daily cap",
            ));
        }
        Ok(Self {
            id,
            primary_route_id,
            route_id: shadow_route_id,
            daily_cap,
            reserve,
            ledger,
        })
    }
    pub const fn id(&self) -> &Id {
        &self.id
    }
    pub const fn primary_route_id(&self) -> &Id {
        &self.primary_route_id
    }
    pub const fn shadow_route_id(&self) -> &Id {
        &self.route_id
    }
    pub const fn daily_cap(&self) -> Amount {
        self.daily_cap
    }
    pub const fn reserve(&self) -> Amount {
        self.reserve
    }
    pub fn ledger(&self) -> &ShadowLedger {
        &self.ledger
    }
}

/// How one shadow ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowOutcome {
    /// Sent; how the call ended, as its record says.
    Sent(Outcome),
    /// Not sent: the ledger refused to hold its reserve, past the cap or for the cause named.
    /// Its record says `skipped`.
    Skipped(BudgetError),
    /// No call: the shadow route refused the request before any attempt (an unknown route,
    /// a request it cannot take). Nothing is recorded and nothing stays held.
    Refused(ErrorCode),
}

/// One shadow's result, for the caller that drove the handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowResult {
    pub shadow_id: Id,
    pub shadow_route_id: Id,
    /// `<primary record id>/shadow/<shadow id>`; absent when the shadow was refused.
    pub record_id: Option<Id>,
    pub outcome: ShadowOutcome,
}

/// The shadows a call may have and the ports they run on. A shadow is admitted by its ledger
/// alone; its events go to no sink.
#[derive(Clone, Copy)]
pub struct Shadowing<'s> {
    pub shadows: &'s [Shadow],
    pub models: &'s dyn Models,
    pub cancel: &'s Cancel,
    pub pause: Pause<'s>,
}

/// The shadow calls of one primary call, none sent yet. Drive [`ShadowCalls::run`] after
/// answering the caller, on any executor; dropping it sends nothing. Dropped while a shadow is in
/// flight, it leaves that shadow's reservation held for the rest of its day, never released.
pub struct ShadowCalls<'a> {
    catalog: &'a Catalog,
    request: TurnRequest,
    input_tokens: Option<u64>,
    policy: FallbackPolicy,
    primary_record_id: Id,
    recording: Recording<'a>,
    shadowing: Shadowing<'a>,
    shadows: Vec<&'a Shadow>,
}

impl ShadowCalls<'_> {
    /// How many shadows follow the primary call: those of its route.
    pub fn len(&self) -> usize {
        self.shadows.len()
    }
    pub fn is_empty(&self) -> bool {
        self.shadows.is_empty()
    }

    /// Sends each shadow in turn, records it and settles its ledger. Nothing here can reach the
    /// primary call's caller, which already has its result.
    pub async fn run(self) -> Vec<ShadowResult> {
        let mut results = Vec::with_capacity(self.shadows.len());
        for shadow in &self.shadows {
            results.push(self.one(shadow).await);
        }
        results
    }

    /// The shadow's request, re-addressed to its route, and the reservation it needs, read at
    /// the clock's `now`; the code of the refusal when the shadow route cannot take it.
    fn prepare(
        &self,
        shadow: &Shadow,
        now: u64,
    ) -> Result<(TurnRequest, ReservationRequest), ErrorCode> {
        let route = self
            .catalog
            .routes()
            .find(|route| route.id == shadow.route_id)
            .ok_or(ErrorCode::InvalidRequest)?;
        let mut request = self.request.clone();
        route.alias.as_str().clone_into(&mut request.model);
        let explanation = self
            .catalog
            .explain(&request, self.input_tokens)
            .map_err(|error| error.code)?;
        // The reservation names the target the route would try first; a fallback is spend of
        // the same reservation.
        let target = explanation
            .targets
            .iter()
            .find(|target| explanation.selected_target_id.as_ref() == Some(&target.target_id))
            .or_else(|| explanation.targets.first())
            .ok_or(ErrorCode::InvalidRequest)?;
        let record_id = Id::new(format!("{}/shadow/{}", self.primary_record_id, shadow.id))
            .map_err(|_| ErrorCode::InvalidRequest)?;
        let assumption = Id::new("shadow-reserve").map_err(|_| ErrorCode::InvalidRequest)?;
        let reservation = ReservationRequest {
            id: record_id.clone(),
            operation: Operation::Inference {
                binding: target.provenance.clone(),
                billing: target.billing_kind,
                request_id: record_id,
            },
            reserved: shadow.reserve,
            assumption,
            expires_at_ms: utc_day(now).saturating_add(1).saturating_mul(DAY_MS),
        };
        Ok((request, reservation))
    }

    async fn one(&self, shadow: &Shadow) -> ShadowResult {
        let result = |record_id, outcome| ShadowResult {
            shadow_id: shadow.id.clone(),
            shadow_route_id: shadow.route_id.clone(),
            record_id,
            outcome,
        };
        let refused = |code| result(None, ShadowOutcome::Refused(code));
        let now = self.recording.clock.unix_ms();
        let day = utc_day(now);
        let (request, reservation) = match self.prepare(shadow, now) {
            Ok(prepared) => prepared,
            Err(code) => return refused(code),
        };
        let record_id = reservation.id.clone();
        let identity = CallIdentity {
            id: record_id.clone(),
            role: Role::Shadow,
            primary_record_id: Some(self.primary_record_id.clone()),
        };
        if let Err(cause) = shadow.ledger.open(shadow, day, now, reservation) {
            self.recording
                .sink
                .record(skipped(identity, &shadow.route_id, now));
            return result(Some(record_id), ShadowOutcome::Skipped(cause));
        }
        let mut admit = |_: &Selection<'_>| Ok(());
        let run = crate::record_turn(
            self.catalog,
            &request,
            self.input_tokens,
            self.policy,
            Ports {
                models: self.shadowing.models,
                admit: &mut admit,
                sink: &mut Unseen,
                cancel: self.shadowing.cancel,
                pause: self.shadowing.pause,
            },
            identity,
            self.recording,
        )
        .await;
        let settled_at = self.recording.clock.unix_ms();
        let currency = shadow.ledger.currency();
        match run {
            Ok(run) => {
                let settlement = self.settlement(currency, &record_id, &run);
                shadow
                    .ledger
                    .settle(&shadow.id, day, settled_at, record_id.clone(), settlement);
                result(
                    Some(record_id),
                    ShadowOutcome::Sent(crate::run_outcome(&run)),
                )
            }
            Err(error) => {
                // Refused before any attempt: nothing was sent, so nothing was spent.
                let nothing = Settlement {
                    currency: currency.clone(),
                    known: Amount::ZERO,
                    complete: true,
                    evidence: record_id.clone(),
                };
                shadow
                    .ledger
                    .settle(&shadow.id, day, settled_at, record_id, nothing);
                refused(error.code)
            }
        }
    }

    /// What the run spent, as the record prices it. Complete only when every attempt that may
    /// have cost something is priced in the ledger's currency; otherwise the known part is a
    /// lower bound and the reservation stays held.
    fn settlement(&self, currency: &Currency, record_id: &Id, run: &FallbackRun) -> Settlement {
        let held = Settlement {
            currency: currency.clone(),
            known: Amount::ZERO,
            complete: false,
            evidence: record_id.clone(),
        };
        let Some(prices) = self.recording.prices else {
            return held;
        };
        let attempts = crate::costly_attempts(record_id, run, prices);
        if attempts.is_empty() {
            return Settlement {
                complete: true,
                ..held
            };
        }
        match Cost::of_attempts(prices, attempts) {
            Some(cost) if &cost.currency == currency => Settlement {
                known: cost.known_subtotal,
                complete: cost.complete,
                ..held
            },
            _ => held,
        }
    }
}

/// The record of a shadow the ledger did not admit: not sent, no attempt, no latency.
fn skipped(identity: CallIdentity, route_id: &Id, now: u64) -> CallRecord {
    CallRecord::new(
        identity,
        Kind::Turn,
        route_id.clone(),
        Timing {
            started_at_unix_ms: now,
            latency_ms: None,
            time_to_first_event_ms: None,
        },
        CallFacts {
            route_target_id: None,
            upstream_model: None,
            outcome: Outcome::Skipped,
            error_code: None,
            dispatch: Some(Dispatch::NotSent),
            attempts: 0,
            usage: Usage::default(),
            cost: None,
        },
    )
}

/// Takes a shadow's events and shows them to no one.
struct Unseen;

impl StreamSink for Unseen {
    fn emit(&mut self, _: StreamEvent) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async { Ok(()) })
    }
}

/// Runs [`crate::record_turn`] and returns its run with the shadow calls of its route, none of
/// them sent yet.
///
/// The primary call, its record and what its caller is shown are exactly those of
/// [`crate::record_turn`]: the shadows start only when the returned handle is driven. A primary
/// call refused before it starts has no record and no shadow.
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
    let primary_record_id = identity.id.clone();
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
    let shadows = shadowing
        .shadows
        .iter()
        .filter(|shadow| shadow.primary_route_id == run.explanation.route_id)
        .collect();
    Ok((
        run,
        ShadowCalls {
            catalog,
            request: request.clone(),
            input_tokens,
            policy,
            primary_record_id,
            recording,
            shadowing,
            shadows,
        },
    ))
}
