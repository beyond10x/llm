//! Security review of unit u4 (story `shadow-calls`): the daily money cap of shadow calls under
//! hostile and misconfigured sequences, through scripted models and a scripted wall clock; no
//! network.
//!
//! The bound under test is the one `docs/design.md` states: a shadow call counts against the UTC
//! day it was admitted on, and a day's spend passes the cap by at most the excess over its
//! reserve of the shadows still in flight when that day's last shadow was admitted.

use llm_calls::{
    CallIdentity, CallLog, Clock, Outcome, Recording, Role, Shadow, ShadowCalls, ShadowLedger,
    ShadowOutcome, ShadowResult, Shadowing, record_turn_shadowed, utc_day,
};
use llm_core::{
    BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item, Model, Provenance,
    StopReason, StreamEvent, StreamSink, TurnObservation, TurnOutcome, TurnRequest, Usage, VecSink,
};
use llm_cost::budget::BudgetError;
use llm_cost::{Amount, Currency, ModelPrice, PriceBook, PriceDocument, Rate};
use llm_routing::{
    Catalog, CatalogDocument, FallbackPolicy, Models, Ports, Route, RouteTarget, Selection,
};
use std::{
    collections::BTreeSet,
    future::{Future, poll_fn},
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
    time::Duration,
};

const EXAMPLE: &str = include_str!("../../../examples/catalog.toml");

const DAY_MS: u64 = 86_400_000;
/// 2025-11-05T22:00:00Z, two hours before a UTC midnight.
const START: u64 = 1_762_380_000_000;

const PRIMARY_ANSWER: &str = "primary-answer-marker-heron-5521";
const SHADOW_ANSWER: &str = "shadow-answer-marker-marten-9034";
const SHADOW_PROSE: &str = "shadow-failure-marker-newt-4417";

fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}

fn amount(value: &str) -> Amount {
    Amount::parse(value).unwrap()
}

fn usd() -> Currency {
    Currency::new("USD").unwrap()
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

/// Route `coding` (alias `code`) answers on `local-small`; route `coding-shadow` (alias
/// `code-shadow`) on `remote-large`.
fn catalog() -> Catalog {
    let mut doc = CatalogDocument::parse(EXAMPLE).unwrap();
    doc.routes[0].fallback_enabled = false;
    doc.routes.push(Route {
        id: id("coding-shadow"),
        alias: id("code-shadow"),
        fallback_enabled: false,
        kind: llm_routing::RouteKind::Turn,
    });
    doc.targets.push(RouteTarget {
        id: id("shadow-large"),
        route_id: id("coding-shadow"),
        serving_model_id: id("remote-large"),
        position: 0,
    });
    doc.validate().unwrap()
}

/// One per thousand input tokens and two per thousand output tokens, in USD, for both models.
fn prices(catalog: &Catalog) -> PriceBook {
    let rate = |value: &str| {
        Some(Rate {
            amount: amount(value),
            per_units: 1000,
        })
    };
    let price = |price_id: &str, serving: &str| ModelPrice {
        id: id(price_id),
        serving_model_id: id(serving),
        binding_revision: catalog
            .binding(&id(serving))
            .unwrap()
            .provenance()
            .binding_revision
            .clone(),
        upstream_model: id("example/Any-Model"),
        input: rate("1"),
        cached_input: None,
        cache_creation_input: None,
        output: rate("2"),
    };
    PriceBook::from_document(PriceDocument::new(
        id("prices-r9"),
        "fixture price list",
        1,
        usd(),
        vec![
            price("price-small", "local-small"),
            price("price-large", "remote-large"),
        ],
    ))
    .unwrap()
}

/// What the far side bills for one attempt of 100 input and `output` output tokens, at the
/// rates of [`prices`], whether or not it reports the usage.
const fn attempt_nanos(output: u64) -> u64 {
    100_000_000 + 2_000_000 * output
}

fn usage(input: u64, output: u64) -> Usage {
    Usage {
        input_tokens: Some(input),
        output_tokens: Some(output),
        cached_input_tokens: Some(0),
        cache_creation_input_tokens: Some(0),
        reasoning_output_tokens: None,
    }
}

fn observed(provenance: &Provenance, usage: Option<Usage>) -> TurnObservation {
    let mut observation = TurnObservation::new(provenance.clone());
    observation.usage = usage;
    observation.upstream_model = Some(id("example/Any-Model"));
    observation.final_usage = true;
    observation
}

fn request() -> TurnRequest {
    let mut request = TurnRequest::new("code", vec![Item::user("hello")]);
    request.max_output_tokens = Some(1000);
    request
}

/// A wall clock the test sets anywhere, forward or back; the monotonic reading never goes back.
struct Wall {
    unix_ms: AtomicU64,
    monotonic_ms: AtomicU64,
}

impl Wall {
    const fn at(unix_ms: u64) -> Self {
        Self {
            unix_ms: AtomicU64::new(unix_ms),
            monotonic_ms: AtomicU64::new(0),
        }
    }
    fn set(&self, unix_ms: u64) {
        let was = self.unix_ms.swap(unix_ms, Ordering::SeqCst);
        self.monotonic_ms
            .fetch_add(unix_ms.saturating_sub(was), Ordering::SeqCst);
    }
}

impl Clock for Wall {
    fn unix_ms(&self) -> u64 {
        self.unix_ms.load(Ordering::SeqCst)
    }
    fn monotonic(&self) -> Duration {
        Duration::from_millis(self.monotonic_ms.load(Ordering::SeqCst))
    }
}

/// The primary route's model: answers every attempt at once, 0.2 each.
struct Plain {
    provenance: Provenance,
    capabilities: Capabilities,
    calls: AtomicUsize,
}

impl Plain {
    fn on(catalog: &Catalog, serving: &str) -> Self {
        let binding = catalog.binding(&id(serving)).unwrap();
        Self {
            provenance: binding.provenance().clone(),
            capabilities: binding.capabilities().clone(),
            calls: AtomicUsize::new(0),
        }
    }
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Model for Plain {
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
            sink.emit(StreamEvent::TextDelta {
                text: PRIMARY_ANSWER.to_owned(),
            })
            .await?;
            Ok(TurnOutcome {
                stop_reason: StopReason::EndTurn,
                items: vec![Item::assistant(PRIMARY_ANSWER)],
                observation: observed(&self.provenance, Some(usage(100, 50))),
            })
        })
    }
}

/// One shadow call as the far side saw it.
#[derive(Default)]
struct Billed {
    /// Per attempt: output tokens (100 input tokens each) and whether the usage is reported.
    /// Every attempt but the last fails retriably after it was accepted.
    attempts: Vec<(u64, bool)>,
    /// The first attempt waits until this is set.
    released: bool,
    tries: usize,
    cost_nanos: u128,
    /// The clock's UTC day when the first attempt was sent, and the order of sending.
    day: Option<u64>,
    order: u64,
    /// The other shadow calls sent and not finished when this one was sent.
    in_flight: Vec<usize>,
    finished: bool,
}

/// The shadow route's model. The test names the shadow call it serves before each poll; every
/// attempt bills its usage at the book's rates whether or not it reports it, a dropped one too.
struct Biller<'c> {
    provenance: Provenance,
    capabilities: Capabilities,
    clock: &'c Wall,
    serving: AtomicUsize,
    sent: AtomicU64,
    calls: Mutex<Vec<Billed>>,
}

impl<'c> Biller<'c> {
    fn on(catalog: &Catalog, serving: &str, clock: &'c Wall) -> Self {
        let binding = catalog.binding(&id(serving)).unwrap();
        Self {
            provenance: binding.provenance().clone(),
            capabilities: binding.capabilities().clone(),
            clock,
            serving: AtomicUsize::new(0),
            sent: AtomicU64::new(0),
            calls: Mutex::new(Vec::new()),
        }
    }
    fn add(&self, attempts: Vec<(u64, bool)>, released: bool) -> usize {
        let mut calls = self.calls.lock().unwrap();
        calls.push(Billed {
            attempts,
            released,
            ..Billed::default()
        });
        calls.len() - 1
    }
    fn serve(&self, call: usize) {
        self.serving.store(call, Ordering::SeqCst);
    }
    fn release(&self, call: usize) {
        self.calls.lock().unwrap()[call].released = true;
    }
    fn day(&self, call: usize) -> Option<u64> {
        self.calls.lock().unwrap()[call].day
    }
    fn sent(&self) -> u64 {
        self.sent.load(Ordering::SeqCst)
    }
}

impl Model for Biller<'_> {
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
            let call = self.serving.load(Ordering::SeqCst);
            let (attempt, output, reports, last) = {
                let mut state = self.calls.lock().unwrap();
                let in_flight: Vec<usize> = if state[call].tries == 0 {
                    state
                        .iter()
                        .enumerate()
                        .filter(|(other, billed)| {
                            *other != call && billed.day.is_some() && !billed.finished
                        })
                        .map(|(other, _)| other)
                        .collect()
                } else {
                    Vec::new()
                };
                let this = &mut state[call];
                let attempt = this.tries;
                this.tries += 1;
                let (output, reports) = this.attempts[attempt];
                this.cost_nanos += u128::from(attempt_nanos(output));
                if attempt == 0 {
                    this.day = Some(utc_day(self.clock.unix_ms()));
                    this.order = self.sent.fetch_add(1, Ordering::SeqCst);
                    this.in_flight = in_flight;
                }
                (attempt, output, reports, attempt + 1 == this.attempts.len())
            };
            if attempt == 0 {
                poll_fn(|_| {
                    if self.calls.lock().unwrap()[call].released {
                        Poll::Ready(())
                    } else {
                        Poll::Pending
                    }
                })
                .await;
            }
            let observation = observed(&self.provenance, reports.then(|| usage(100, output)));
            if last {
                self.calls.lock().unwrap()[call].finished = true;
                sink.emit(StreamEvent::TextDelta {
                    text: SHADOW_ANSWER.to_owned(),
                })
                .await?;
                Ok(TurnOutcome {
                    stop_reason: StopReason::EndTurn,
                    items: vec![Item::assistant(SHADOW_ANSWER)],
                    observation,
                })
            } else {
                Err(Error::new(ErrorCode::Unavailable, SHADOW_PROSE)
                    .with_dispatch(Dispatch::Accepted)
                    .with_retriable(true)
                    .with_observation(observation))
            }
        })
    }
}

struct Fleet<'c> {
    primary: Plain,
    shadow: Biller<'c>,
}

impl Models for Fleet<'_> {
    fn model(&self, serving_model_id: &Id) -> Option<&dyn Model> {
        match serving_model_id.as_str() {
            "local-small" => Some(&self.primary),
            "remote-large" => Some(&self.shadow),
            _ => None,
        }
    }
}

fn no_wait(_: Duration) -> BoxFuture<'static, ()> {
    Box::pin(async {})
}

struct Env<'e> {
    catalog: &'e Catalog,
    fleet: &'e Fleet<'e>,
    clock: &'e Wall,
    log: &'e CallLog,
    prices: Option<&'e PriceBook>,
    cancel: &'e Cancel,
    shadows: &'e [Shadow],
}

/// One primary call; its shadows come back undriven.
fn primary<'e>(env: &Env<'e>, record_id: &str) -> ShadowCalls<'e> {
    let mut sink = VecSink::new(64, 64 * 1024);
    let mut admit = |_: &Selection<'_>| Ok(());
    let (_, calls) = block_on(record_turn_shadowed(
        env.catalog,
        &request(),
        Some(100),
        FallbackPolicy::default(),
        Ports {
            models: env.fleet,
            admit: &mut admit,
            sink: &mut sink,
            cancel: env.cancel,
            pause: &no_wait,
        },
        CallIdentity::primary(id(record_id)),
        Recording {
            sink: env.log,
            clock: env.clock,
            prices: env.prices,
        },
        Shadowing {
            shadows: env.shadows,
            models: env.fleet,
            cancel: env.cancel,
            pause: &no_wait,
        },
    ))
    .unwrap();
    calls
}

type Running<'e> = Pin<Box<dyn Future<Output = Vec<ShadowResult>> + 'e>>;

fn poll_once(future: &mut Running<'_>) -> Poll<Vec<ShadowResult>> {
    future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
}

/// Drives a shadow handle whose gates are open to its end.
fn finish(future: &mut Running<'_>) -> Vec<ShadowResult> {
    for _ in 0..10_000 {
        if let Poll::Ready(results) = poll_once(future) {
            return results;
        }
    }
    panic!("a released shadow call did not finish");
}

/// splitmix64: a fixed seed gives the same sequence on every machine.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

/// What one sequence did: shadows sent, and days whose spend passed the cap.
struct Seen {
    sent: u64,
    days_over_cap: usize,
}

/// One hostile sequence: a random cap, reserve, concurrency bound and pricing; calls whose
/// shadows are held in flight, released, retried, abandoned or cost more than their reserve,
/// unreported usage, and a clock that steps on, crosses midnight, jumps days or is set back.
#[allow(clippy::too_many_lines)]
fn sequence(seed: u64, catalog: &Catalog, book: &PriceBook) -> Result<Seen, String> {
    let mut rng = Rng(seed);
    let cap_nanos = [300_000_000_u64, 500_000_000, 1_000_000_000, 2_500_000_000]
        [usize::try_from(rng.below(4)).unwrap()];
    let reserve_nanos = match rng.below(5) {
        0 => cap_nanos,
        1 => cap_nanos / 2,
        2 => cap_nanos / 5,
        3 => cap_nanos / 20,
        _ => 1,
    };
    let max_active = 1 + u32::try_from(rng.below(5)).unwrap();
    // Priced in the ledger's currency, no price book, or a ledger in another currency.
    let pricing = rng.below(10);
    let currency = Currency::new(if pricing == 9 { "EUR" } else { "USD" }).unwrap();
    let prices = (pricing != 8).then_some(book);
    let ledger = Arc::new(ShadowLedger::new(id("shadow-spend"), currency, max_active).unwrap());
    let shadows = [Shadow::new(
        id("compare-large"),
        id("coding"),
        id("coding-shadow"),
        Amount::from_nanos(cap_nanos),
        Amount::from_nanos(reserve_nanos),
        Arc::clone(&ledger),
    )
    .unwrap()];
    let clock = Wall::at(START);
    let fleet = Fleet {
        primary: Plain::on(catalog, "local-small"),
        shadow: Biller::on(catalog, "remote-large", &clock),
    };
    let log = CallLog::new();
    let cancel = Cancel::new();
    let env = Env {
        catalog,
        fleet: &fleet,
        clock: &clock,
        log: &log,
        prices,
        cancel: &cancel,
        shadows: &shadows,
    };
    let mut trace = vec![format!(
        "seed {seed:#x}: cap {cap_nanos} reserve {reserve_nanos} max_active {max_active} \
         pricing {pricing}"
    )];
    let mut live: Vec<(usize, Running<'_>)> = Vec::new();
    for step in 0..60 {
        let action = rng.below(100);
        if action < 40 {
            let failures = if rng.chance(25) { rng.below(3) } else { 0 };
            let attempts = (0..=failures)
                .map(|_| (rng.below(1001), rng.chance(90)))
                .collect::<Vec<_>>();
            let released = rng.chance(35);
            let call = fleet.shadow.add(attempts.clone(), released);
            let mut running: Running<'_> = Box::pin(primary(&env, &format!("call-{call}")).run());
            fleet.shadow.serve(call);
            let polled = if released {
                Poll::Ready(finish(&mut running))
            } else {
                poll_once(&mut running)
            };
            let sent = fleet.shadow.day(call);
            match polled {
                Poll::Ready(results) => {
                    let outcome = &results[0].outcome;
                    let consistent = matches!(outcome, ShadowOutcome::Sent(_)) == sent.is_some();
                    trace.push(format!(
                        "{step}: call-{call} {attempts:?} at {} -> {outcome:?}",
                        clock.unix_ms()
                    ));
                    if !consistent {
                        return Err(format!("outcome and model disagree\n{}", trace.join("\n")));
                    }
                }
                Poll::Pending => {
                    trace.push(format!(
                        "{step}: call-{call} {attempts:?} at {} -> in flight",
                        clock.unix_ms()
                    ));
                    if sent.is_none() {
                        return Err(format!("pending but not sent\n{}", trace.join("\n")));
                    }
                    live.push((call, running));
                }
            }
        } else if action < 65 && !live.is_empty() {
            let at = usize::try_from(rng.below(u64::try_from(live.len()).unwrap())).unwrap();
            let (call, mut running) = live.swap_remove(at);
            fleet.shadow.release(call);
            fleet.shadow.serve(call);
            let results = finish(&mut running);
            trace.push(format!(
                "{step}: release call-{call} -> {:?}",
                results[0].outcome
            ));
        } else if action < 75 && !live.is_empty() {
            let at = usize::try_from(rng.below(u64::try_from(live.len()).unwrap())).unwrap();
            let (call, running) = live.swap_remove(at);
            drop(running);
            trace.push(format!("{step}: abandon call-{call} in flight"));
        } else {
            let now = clock.unix_ms();
            let to = match rng.below(20) {
                0..=11 => now + 1_000 + rng.below(1_800_000),
                12..=14 => (utc_day(now) + 1) * DAY_MS + 1 + rng.below(60_000),
                15 => now + 2 * DAY_MS + rng.below(DAY_MS),
                _ => now.saturating_sub(1 + rng.below(7_200_000)),
            };
            clock.set(to);
            trace.push(format!("{step}: clock {now} -> {to} (day {})", utc_day(to)));
        }
    }

    let serialized = serde_json::to_string(&log.records()).unwrap();
    for text in [PRIMARY_ANSWER, SHADOW_ANSWER, SHADOW_PROSE] {
        if serialized.contains(text) {
            return Err(format!(
                "a record carries text {text}\n{}",
                trace.join("\n")
            ));
        }
    }

    let state = fleet.shadow.calls.lock().unwrap();
    let days: BTreeSet<u64> = state.iter().filter_map(|billed| billed.day).collect();
    let mut days_over_cap = 0;
    for day in days {
        let on_day = |call: &usize| state[*call].day == Some(day);
        let spend: u128 = state
            .iter()
            .filter(|billed| billed.day == Some(day))
            .map(|billed| billed.cost_nanos)
            .sum();
        let (last, latest) = state
            .iter()
            .enumerate()
            .filter(|(_, billed)| billed.day == Some(day))
            .max_by_key(|(_, billed)| billed.order)
            .unwrap();
        let excess: u128 = latest
            .in_flight
            .iter()
            .copied()
            .chain([last])
            .filter(on_day)
            .map(|call| {
                state[call]
                    .cost_nanos
                    .saturating_sub(u128::from(reserve_nanos))
            })
            .sum();
        let cap = u128::from(cap_nanos);
        if spend > cap {
            days_over_cap += 1;
        }
        if spend > cap + excess {
            return Err(format!(
                "day {day}: spent {spend} nanos, cap {cap} plus in-flight excess {excess}\n{}",
                trace.join("\n")
            ));
        }
    }
    let sent = fleet.shadow.sent();
    drop(state);
    drop(live);
    Ok(Seen {
        sent,
        days_over_cap,
    })
}

/// Property, fixed seeds: no sequence spends a UTC day past its cap by more than the documented
/// bound, and no record carries a model's text.
#[test]
fn no_sequence_spends_a_day_past_its_cap_beyond_the_excess_of_shadows_in_flight() {
    let catalog = catalog();
    let book = prices(&catalog);
    let mut sent = 0;
    let mut over = 0;
    for seed in 0..400 {
        match sequence(0x5EC0_0004_0000 + seed, &catalog, &book) {
            Ok(seen) => {
                sent += seen.sent;
                over += seen.days_over_cap;
            }
            Err(report) => panic!("{report}"),
        }
    }
    // The sequences reach both sides of the bound: shadows are sent, and some days pass the
    // cap through the excess of shadows in flight, so the bound is what is measured.
    assert!(sent > 1_000, "shadows sent: {sent}");
    assert!(
        over > 0,
        "no day passed its cap: the bound was never exercised"
    );
}

/// A shadow whose route is its primary's own route, and one whose primary route is another
/// shadow's route: each is capped, and a shadow call is never itself shadowed.
#[test]
fn a_self_shadow_and_a_shadow_of_a_shadow_route_are_capped_and_never_chain() {
    let catalog = catalog();
    let book = prices(&catalog);
    let clock = Wall::at(START);
    let fleet = Fleet {
        primary: Plain::on(&catalog, "local-small"),
        shadow: Biller::on(&catalog, "remote-large", &clock),
    };
    let ledger = Arc::new(ShadowLedger::new(id("shadow-spend"), usd(), 4).unwrap());
    let make = |shadow_id: &str, primary_route: &str, shadow_route: &str| {
        Shadow::new(
            id(shadow_id),
            id(primary_route),
            id(shadow_route),
            amount("0.3"),
            amount("0.25"),
            Arc::clone(&ledger),
        )
        .unwrap()
    };
    let shadows = [
        make("self", "coding", "coding"),
        make("back", "coding-shadow", "coding"),
        make("compare", "coding", "coding-shadow"),
    ];
    let log = CallLog::new();
    let cancel = Cancel::new();
    let env = Env {
        catalog: &catalog,
        fleet: &fleet,
        clock: &clock,
        log: &log,
        prices: Some(&book),
        cancel: &cancel,
        shadows: &shadows,
    };
    let mut outcomes = Vec::new();
    for call in 0..2 {
        let served = fleet.shadow.add(vec![(50, true)], true);
        fleet.shadow.serve(served);
        let mut running: Running<'_> = Box::pin(primary(&env, &format!("call-{call}")).run());
        outcomes.push(
            finish(&mut running)
                .into_iter()
                .map(|result| (result.shadow_id.as_str().to_owned(), result.outcome))
                .collect::<Vec<_>>(),
        );
    }

    let sent = ShadowOutcome::Sent(Outcome::Succeeded);
    let past_cap = ShadowOutcome::Skipped(BudgetError::LimitExceeded);
    assert_eq!(outcomes.len(), 2);
    assert_eq!(
        outcomes[0],
        [("self".to_owned(), sent), ("compare".to_owned(), sent)]
    );
    assert_eq!(
        outcomes[1],
        [
            ("self".to_owned(), past_cap),
            ("compare".to_owned(), past_cap)
        ]
    );
    // Two primaries and one self-shadow on the primary's model; one shadow on the other.
    assert_eq!(fleet.primary.calls(), 3);
    assert_eq!(fleet.shadow.sent(), 1);
    let day = utc_day(START);
    assert_eq!(ledger.totals(&id("back"), day), None, "never chained");
    for shadow_id in ["self", "compare"] {
        let totals = ledger.totals(&id(shadow_id), day).unwrap();
        assert_eq!(totals.settled_nanos, "200000000", "{shadow_id}");
    }
    let records = log.records();
    let own = records
        .iter()
        .find(|record| record.id.as_str() == "call-0/shadow/self")
        .unwrap();
    assert_eq!((own.role, own.route_id.as_str()), (Role::Shadow, "coding"));
    assert!(
        records
            .iter()
            .all(|record| !record.id.as_str().contains("/shadow/back"))
    );
}

/// A day's ledger is not forgotten while a shadow admitted on it is still in flight, however
/// many days later another shadow opens a ledger; once it settles and is idle it may go.
#[test]
fn a_day_ledger_is_kept_while_its_shadow_is_in_flight_days_later() {
    let catalog = catalog();
    let book = prices(&catalog);
    let clock = Wall::at(START);
    let fleet = Fleet {
        primary: Plain::on(&catalog, "local-small"),
        shadow: Biller::on(&catalog, "remote-large", &clock),
    };
    let ledger = Arc::new(ShadowLedger::new(id("shadow-spend"), usd(), 4).unwrap());
    let shadows = [Shadow::new(
        id("compare-large"),
        id("coding"),
        id("coding-shadow"),
        amount("0.3"),
        amount("0.25"),
        Arc::clone(&ledger),
    )
    .unwrap()];
    let log = CallLog::new();
    let cancel = Cancel::new();
    let env = Env {
        catalog: &catalog,
        fleet: &fleet,
        clock: &clock,
        log: &log,
        prices: Some(&book),
        cancel: &cancel,
        shadows: &shadows,
    };
    let first_day = utc_day(START);
    let held = fleet.shadow.add(vec![(50, true)], false);
    fleet.shadow.serve(held);
    let mut in_flight: Running<'_> = Box::pin(primary(&env, "call-held").run());
    assert!(poll_once(&mut in_flight).is_pending());

    clock.set(START + 3 * DAY_MS);
    let later = fleet.shadow.add(vec![(50, true)], true);
    fleet.shadow.serve(later);
    let mut running: Running<'_> = Box::pin(primary(&env, "call-later").run());
    assert_eq!(
        finish(&mut running)[0].outcome,
        ShadowOutcome::Sent(Outcome::Succeeded)
    );
    let kept = ledger.totals(&id("compare-large"), first_day).unwrap();
    assert_eq!(
        (kept.active_count, kept.held_nanos.as_str()),
        (1, "250000000")
    );

    fleet.shadow.release(held);
    fleet.shadow.serve(held);
    assert_eq!(
        finish(&mut in_flight)[0].outcome,
        ShadowOutcome::Sent(Outcome::Succeeded)
    );
    let settled = ledger.totals(&id("compare-large"), first_day).unwrap();
    assert_eq!(
        (settled.settled_nanos.as_str(), settled.active_count),
        ("200000000", 0)
    );

    clock.set(START + 5 * DAY_MS);
    let last = fleet.shadow.add(vec![(50, true)], true);
    fleet.shadow.serve(last);
    let mut running: Running<'_> = Box::pin(primary(&env, "call-last").run());
    finish(&mut running);
    assert_eq!(ledger.totals(&id("compare-large"), first_day), None);
}
