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
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, btree_map::Entry},
    fmt::Write as _,
    sync::{Arc, Mutex, PoisonError},
};

const DAY_MS: u64 = 86_400_000;

/// The UTC day a wall-clock reading falls on, counted from the Unix epoch.
pub const fn utc_day(unix_ms: u64) -> u64 {
    unix_ms / DAY_MS
}

/// Where shadow calls are admitted: one `llm.budget` ledger per shadow and UTC day, its limit the
/// shadow's daily cap.
///
/// [`ShadowLedger::new`] holds the cap in memory, in `llm-cost`'s `BudgetEngine`, which is not a
/// durable permission: it is per process and lost on restart. A restarted process, a second
/// process, or a second ledger in the same process each start every day's cap afresh, so one
/// ledger must serve every shadow of a deployment, for as long as the deployment runs.
///
/// With the `sqlite` feature, [`ShadowLedger::durable`] keeps each shadow's day on a
/// `llm-cost` `SqliteLedger` instead, so the day's spend survives a restart and a second ledger
/// on the same directory is refused while the first is alive.
#[derive(Debug)]
pub struct ShadowLedger {
    id: Id,
    currency: Currency,
    max_active: u32,
    store: Store,
    days: Mutex<Days>,
}

/// Where a shadow's day ledgers live.
#[derive(Debug)]
enum Store {
    Memory,
    #[cfg(feature = "sqlite")]
    Directory(std::path::PathBuf),
}

/// One shadow's day ledger: in memory, or on a durable `SqliteLedger`.
enum Day {
    Memory(BudgetEngine),
    #[cfg(feature = "sqlite")]
    Durable(llm_cost::budget::SqliteLedger),
}

impl std::fmt::Debug for Day {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Day")
    }
}

impl Day {
    fn view(&self) -> Result<llm_cost::budget::BudgetView, BudgetError> {
        match self {
            Self::Memory(engine) => engine.view(),
            #[cfg(feature = "sqlite")]
            Self::Durable(ledger) => ledger.view(),
        }
    }

    #[cfg_attr(
        not(feature = "sqlite"),
        expect(
            clippy::unnecessary_wraps,
            reason = "a durable day's policy is read from its ledger and can fail"
        )
    )]
    fn policy(&self) -> Result<BudgetPolicy, BudgetError> {
        match self {
            Self::Memory(engine) => Ok(engine.policy().clone()),
            #[cfg(feature = "sqlite")]
            Self::Durable(_) => self.view().map(|view| view.policy),
        }
    }

    fn now_ms(&self) -> u64 {
        match self {
            Self::Memory(engine) => engine.now_ms(),
            #[cfg(feature = "sqlite")]
            Self::Durable(_) => self.view().map_or(0, |view| view.now_ms),
        }
    }

    fn apply(&mut self, now_ms: u64, command: BudgetCommand) -> Result<(), BudgetError> {
        match self {
            Self::Memory(engine) => engine.apply(now_ms, command).map(|_| ()),
            #[cfg(feature = "sqlite")]
            Self::Durable(ledger) => ledger.apply(now_ms, command).map(|_| ()),
        }
    }

    /// A day ledger with nothing unsettled in it.
    fn idle(&self) -> bool {
        self.view()
            .is_ok_and(|view| view.totals.active_count == 0 && view.totals.uncertain_count == 0)
    }
}

#[derive(Debug, Default)]
struct Days {
    /// The latest UTC day a shadow was admitted on; no earlier day opens again.
    latest: Option<u64>,
    engines: BTreeMap<(Id, u64), Day>,
}

/// A directory name for a shadow id: the id itself when it is only letters, digits, `-` and `_`,
/// else each other byte as `%xx`, else a digest when that would be too long for a name. The
/// forms never meet: an escape is `%` and two hex digits, and the digest form starts `%sha256-`.
#[cfg(feature = "sqlite")]
fn directory_name(id: &Id) -> String {
    let mut name = String::new();
    for byte in id.as_str().bytes() {
        if byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' {
            name.push(char::from(byte));
        } else {
            let _ = write!(name, "%{byte:02x}");
        }
    }
    if name.len() <= 200 {
        return name;
    }
    let mut digest = String::from("%sha256-");
    for byte in Sha256::digest(id.as_str().as_bytes()) {
        let _ = write!(digest, "{byte:02x}");
    }
    digest
}

impl ShadowLedger {
    /// A ledger in `currency` whose day ledgers are `SqliteLedger`s under `directory`, one at
    /// `<directory>/<shadow id>/<utc day>`, created on a shadow's first admission that day and
    /// opened again after a restart. It holds at most `max_active` unsettled shadow calls per
    /// shadow and day.
    ///
    /// A day ledger has one owner: while one ledger holds it, another on the same directory,
    /// in this process or another, is refused and its shadows are skipped, never sent. A day
    /// ledger that cannot be opened (a changed cap or concurrency bound, damaged files, a clock
    /// before the day's last write) skips that shadow for the day: a cap is never started afresh.
    /// Keep one durable ledger per directory for as long as the deployment runs.
    ///
    /// # Errors
    /// Refuses a concurrency bound the budget engine refuses.
    #[cfg(feature = "sqlite")]
    pub fn durable(
        directory: impl Into<std::path::PathBuf>,
        id: Id,
        currency: Currency,
        max_active: u32,
    ) -> Result<Self, BudgetError> {
        let mut ledger = Self::new(id, currency, max_active)?;
        ledger.store = Store::Directory(directory.into());
        Ok(ledger)
    }

    /// A fresh day ledger for `shadow`, or the durable one already on disk.
    #[cfg_attr(not(feature = "sqlite"), expect(unused_variables))]
    fn day_ledger(
        &self,
        shadow: &Shadow,
        day: u64,
        now_ms: u64,
        policy: BudgetPolicy,
    ) -> Result<Day, BudgetError> {
        match &self.store {
            Store::Memory => Ok(Day::Memory(BudgetEngine::new(policy)?)),
            #[cfg(feature = "sqlite")]
            Store::Directory(directory) => {
                use llm_cost::budget::SqliteLedger;
                let parent = directory.join(directory_name(&shadow.id));
                std::fs::create_dir_all(&parent).map_err(|_| BudgetError::Storage)?;
                // A later day on disk means the clock was set back: never open an earlier cap.
                let later = std::fs::read_dir(&parent)
                    .map_err(|_| BudgetError::Storage)?
                    .any(|entry| {
                        entry
                            .ok()
                            .and_then(|entry| entry.file_name().to_str()?.parse::<u64>().ok())
                            .is_none_or(|held| held > day)
                    });
                if later {
                    return Err(BudgetError::ClockReversed);
                }
                let path = parent.join(day.to_string());
                let ledger = if std::fs::symlink_metadata(&path).is_ok() {
                    SqliteLedger::open(&path, &policy, now_ms)?
                } else {
                    SqliteLedger::create(&path, policy, now_ms)?
                };
                Ok(Day::Durable(ledger))
            }
        }
    }

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
            store: Store::Memory,
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
            .retain(|(_, opened), engine| opened.saturating_add(1) >= day || !engine.idle());
        let policy = BudgetPolicy {
            id: self.id.clone(),
            currency: self.currency.clone(),
            limit: shadow.daily_cap,
            max_active: self.max_active,
        };
        let engine = match days.engines.entry((shadow.id.clone(), day)) {
            Entry::Occupied(held) if held.get().policy().as_ref() != Ok(&policy) => {
                return Err(BudgetError::PolicyMismatch);
            }
            Entry::Occupied(held) => held.into_mut(),
            Entry::Vacant(free) => free.insert(self.day_ledger(shadow, day, now_ms, policy)?),
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
    /// `<primary record id>/shadow/<shadow id>`, or `shadow-sha256-<hex>` of both when that would
    /// pass the 256-byte id bound; absent when the shadow was refused.
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
        let record_id = shadow_record_id(&self.primary_record_id, &shadow.id)
            .ok_or(ErrorCode::InvalidRequest)?;
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

/// A shadow call's record id, which is also its reservation id: `<primary>/shadow/<shadow>`
/// while that fits an id, else `shadow-sha256-<hex>` of the two ids, length-prefixed so no two
/// pairs share an input. The readable form always holds `/shadow/` and the digest form never
/// does, so the two never meet.
fn shadow_record_id(primary: &Id, shadow: &Id) -> Option<Id> {
    Id::new(format!("{primary}/shadow/{shadow}"))
        .ok()
        .or_else(|| {
            let mut hasher = Sha256::new();
            for part in [primary.as_str(), shadow.as_str()] {
                hasher.update(u64::try_from(part.len()).unwrap_or(u64::MAX).to_be_bytes());
                hasher.update(part.as_bytes());
            }
            let mut digest = String::from("shadow-sha256-");
            for byte in hasher.finalize() {
                write!(digest, "{byte:02x}").ok()?;
            }
            Id::new(digest).ok()
        })
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
/// call refused before it starts has no record and no shadow, and one that made no attempt (its
/// caller's admission refused it, or it was cancelled first) has its record and no shadow.
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
    // A primary that sent nothing has nothing to compare with: no shadow is sent or paid.
    let shadows = shadowing
        .shadows
        .iter()
        .filter(|shadow| {
            !run.attempts.is_empty() && shadow.primary_route_id == run.explanation.route_id
        })
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
