//! Shadow calls through scripted `llm_core::Model`s, a scripted clock, an in-memory record sink
//! and a shadow ledger: no projection, no network.

use llm_calls::{
    CallIdentity, CallLog, CallRecord, Clock, Outcome, Recording, Role, Shadow, ShadowCalls,
    ShadowLedger, ShadowOutcome, ShadowResult, Shadowing, record_turn_shadowed, utc_day,
};
use llm_core::{
    BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item, Model, Provenance,
    StopReason, StreamEvent, StreamSink, TurnObservation, TurnOutcome, TurnRequest, Usage, VecSink,
};
use llm_cost::budget::BudgetError;
use llm_cost::{Amount, Currency, ModelPrice, PriceBook, PriceDocument, Rate};
use llm_routing::{
    Catalog, CatalogDocument, FallbackPolicy, FallbackRun, Models, Ports, Route, RouteTarget,
};
use std::{
    future::{Future, poll_fn},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
    time::Duration,
};

const EXAMPLE: &str = include_str!("../../../examples/catalog.toml");

const PRIMARY_ANSWER: &str = "primary-answer-marker-wren-1180";
const SHADOW_ANSWER: &str = "shadow-answer-marker-stoat-7243";
const SHADOW_PROSE: &str = "shadow-failure-marker-vole-3361";

/// 2025-11-05T23:59:00Z, one minute before a UTC midnight.
const LAST_MINUTE: u64 = 1_762_387_140_000;
/// 2025-11-06T00:00:10Z.
const NEXT_DAY: u64 = 1_762_387_210_000;

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

/// Route `coding` (alias `code`) answers on `local-small` only; route `coding-shadow` (alias
/// `code-shadow`) answers on `remote-large`.
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

/// A clock that moves only when a scripted model says time passed, or a test sets the date.
struct Scripted {
    unix_ms: AtomicU64,
    now_ms: AtomicU64,
}

impl Scripted {
    const fn at(unix_ms: u64) -> Self {
        Self {
            unix_ms: AtomicU64::new(unix_ms),
            now_ms: AtomicU64::new(0),
        }
    }
    fn advance(&self, ms: u64) {
        self.now_ms.fetch_add(ms, Ordering::SeqCst);
    }
    /// Sets the wall clock; the monotonic reading moves on by the same amount, or not at all
    /// when the wall clock is set back.
    fn set(&self, unix_ms: u64) {
        let was = self.unix_ms();
        self.advance(unix_ms.saturating_sub(was));
        let now = self.now_ms.load(Ordering::SeqCst);
        self.unix_ms.store(unix_ms - now, Ordering::SeqCst);
    }
}

impl Clock for Scripted {
    fn unix_ms(&self) -> u64 {
        self.unix_ms.load(Ordering::SeqCst) + self.now_ms.load(Ordering::SeqCst)
    }
    fn monotonic(&self) -> Duration {
        Duration::from_millis(self.now_ms.load(Ordering::SeqCst))
    }
}

/// Replays one authored script: time passes, events are shown, then the outcome or failure.
struct Fake<'c> {
    provenance: Provenance,
    capabilities: Capabilities,
    clock: &'c Scripted,
    before_first_ms: u64,
    text: String,
    after_ms: u64,
    usage: Option<Usage>,
    upstream_model: Option<Id>,
    fail: Option<Error>,
    /// Fails only the first n attempts; every attempt when absent.
    fail_times: Option<usize>,
    /// Holds every attempt until it is set.
    gate: Option<&'c AtomicBool>,
    calls: AtomicUsize,
}

impl<'c> Fake<'c> {
    fn on(catalog: &Catalog, serving: &str, clock: &'c Scripted, text: &str) -> Self {
        let binding = catalog.binding(&id(serving)).unwrap();
        Self {
            provenance: binding.provenance().clone(),
            capabilities: binding.capabilities().clone(),
            clock,
            before_first_ms: 0,
            text: text.to_owned(),
            after_ms: 0,
            usage: Some(usage(100, 50)),
            upstream_model: Some(id("example/Any-Model")),
            fail: None,
            fail_times: None,
            gate: None,
            calls: AtomicUsize::new(0),
        }
    }
    fn failing(mut self, error: Error) -> Self {
        self.fail = Some(error);
        self
    }
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Model for Fake<'_> {
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
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(gate) = self.gate {
                poll_fn(|_| {
                    if gate.load(Ordering::SeqCst) {
                        Poll::Ready(())
                    } else {
                        Poll::Pending
                    }
                })
                .await;
            }
            let fail = self
                .fail
                .clone()
                .filter(|_| self.fail_times.is_none_or(|times| call < times));
            self.clock.advance(self.before_first_ms);
            if fail.is_none() {
                sink.emit(StreamEvent::TextDelta {
                    text: self.text.clone(),
                })
                .await?;
            }
            self.clock.advance(self.after_ms);
            let mut observation = TurnObservation::new(self.provenance.clone());
            observation.usage.clone_from(&self.usage);
            observation.upstream_model.clone_from(&self.upstream_model);
            observation.final_usage = true;
            match fail {
                None => Ok(TurnOutcome {
                    stop_reason: StopReason::EndTurn,
                    items: vec![Item::assistant(self.text.clone())],
                    observation,
                }),
                Some(error) if error.dispatch == Dispatch::NotSent => Err(error),
                Some(error) => Err(error.with_observation(observation)),
            }
        })
    }
}

/// `local-small` serves the primary route, `remote-large` the shadow route.
struct Fleet<'c> {
    primary: Fake<'c>,
    shadow: Fake<'c>,
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

fn fleet<'c>(catalog: &Catalog, clock: &'c Scripted) -> Fleet<'c> {
    Fleet {
        primary: Fake::on(catalog, "local-small", clock, PRIMARY_ANSWER),
        shadow: Fake::on(catalog, "remote-large", clock, SHADOW_ANSWER),
    }
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

fn request() -> TurnRequest {
    let mut request = TurnRequest::new("code", vec![Item::user("hello")]);
    request.max_output_tokens = Some(128);
    request
}

/// One per thousand input tokens and two per thousand output tokens, in USD, for both serving
/// models: 100 in and 50 out cost 0.2.
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

/// A daily cap of 0.3 with 0.25 held per shadow call: one 0.2 shadow fits a day, a second
/// does not.
fn shadow(ledger: &Arc<ShadowLedger>) -> Shadow {
    Shadow::new(
        id("compare-large"),
        id("coding"),
        id("coding-shadow"),
        amount("0.3"),
        amount("0.25"),
        Arc::clone(ledger),
    )
    .unwrap()
}

fn ledger() -> Arc<ShadowLedger> {
    Arc::new(ShadowLedger::new(id("shadow-spend"), usd(), 4).unwrap())
}

fn no_wait(_: Duration) -> BoxFuture<'static, ()> {
    Box::pin(async {})
}

/// Everything one test's calls share.
struct Env<'e> {
    catalog: &'e Catalog,
    models: &'e Fleet<'e>,
    clock: &'e Scripted,
    log: &'e CallLog,
    prices: Option<&'e PriceBook>,
    cancel: &'e Cancel,
    shadows: &'e [Shadow],
}

struct Primary<'e> {
    run: FallbackRun,
    calls: ShadowCalls<'e>,
    shown: Vec<StreamEvent>,
}

/// One primary call through `record_turn_shadowed`; its shadows are returned undriven.
fn primary<'e>(env: &Env<'e>, record_id: &str) -> Primary<'e> {
    let mut sink = VecSink::new(64, 64 * 1024);
    let mut admit = |_: &llm_routing::Selection<'_>| Ok(());
    let (run, calls) = block_on(record_turn_shadowed(
        env.catalog,
        &request(),
        Some(100),
        FallbackPolicy::default(),
        Ports {
            models: env.models,
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
            models: env.models,
            cancel: env.cancel,
            pause: &no_wait,
        },
    ))
    .unwrap();
    Primary {
        run,
        calls,
        shown: sink.into_events(),
    }
}

/// What the caller of a primary call received.
struct Returned {
    run: FallbackRun,
    shown: Vec<StreamEvent>,
}

/// A primary call whose shadows are driven to their end at once.
fn call_and_shadow(env: &Env<'_>, record_id: &str) -> (Returned, Vec<ShadowResult>) {
    let Primary { run, calls, shown } = primary(env, record_id);
    let results = block_on(calls.run());
    (Returned { run, shown }, results)
}

fn texts(events: &[StreamEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::TextDelta { text } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn answer(run: &FallbackRun) -> Vec<Item> {
    run.result.as_ref().unwrap().items.clone()
}

fn record<'r>(records: &'r [CallRecord], record_id: &str) -> &'r CallRecord {
    records
        .iter()
        .find(|record| record.id.as_str() == record_id)
        .unwrap_or_else(|| panic!("no record {record_id}: {records:?}"))
}

fn nanos(value: &str) -> String {
    amount(value).nanos().to_string()
}

#[test]
fn a_shadow_answer_never_reaches_the_caller() {
    let catalog = catalog();
    let clock = Scripted::at(LAST_MINUTE);
    let models = fleet(&catalog, &clock);
    let log = CallLog::new();
    let prices = prices(&catalog);
    let cancel = Cancel::new();
    let ledger = ledger();
    let shadows = [shadow(&ledger)];
    let env = Env {
        catalog: &catalog,
        models: &models,
        clock: &clock,
        log: &log,
        prices: Some(&prices),
        cancel: &cancel,
        shadows: &shadows,
    };
    let (primary, results) = call_and_shadow(&env, "call-1");

    assert_eq!(models.shadow.calls(), 1, "the shadow route was asked");
    assert_eq!(texts(&primary.shown), [PRIMARY_ANSWER]);
    assert_eq!(answer(&primary.run), [Item::assistant(PRIMARY_ANSWER)]);
    let serialized = serde_json::to_string(&log.records()).unwrap();
    assert!(!serialized.contains(SHADOW_ANSWER), "{serialized}");
    assert_eq!(
        results,
        [ShadowResult {
            shadow_id: id("compare-large"),
            shadow_route_id: id("coding-shadow"),
            record_id: Some(id("call-1/shadow/compare-large")),
            outcome: ShadowOutcome::Sent(Outcome::Succeeded),
        }]
    );
    let shadow = record(&log.records(), "call-1/shadow/compare-large").clone();
    assert_eq!(shadow.role, Role::Shadow);
    assert_eq!(shadow.route_id, id("coding-shadow"));
    assert_eq!(shadow.route_target_id, Some(id("shadow-large")));
}

#[test]
fn a_shadow_call_starts_only_after_the_primary_returned() {
    let catalog = catalog();
    let clock = Scripted::at(LAST_MINUTE);
    let mut models = fleet(&catalog, &clock);
    models.primary.after_ms = 300;
    models.shadow.before_first_ms = 5_000;
    let log = CallLog::new();
    let prices = prices(&catalog);
    let cancel = Cancel::new();
    let ledger = ledger();
    let shadows = [shadow(&ledger)];
    let env = Env {
        catalog: &catalog,
        models: &models,
        clock: &clock,
        log: &log,
        prices: Some(&prices),
        cancel: &cancel,
        shadows: &shadows,
    };
    let Primary { run, calls, .. } = primary(&env, "call-1");

    // The primary has returned; nothing of the shadow has happened, not even its reservation.
    assert!(run.result.is_ok());
    assert_eq!(models.shadow.calls(), 0);
    assert_eq!(log.records().len(), 1, "only the primary's record so far");
    assert_eq!(
        ledger.totals(&id("compare-large"), utc_day(LAST_MINUTE)),
        None
    );
    assert_eq!(calls.len(), 1);

    block_on(calls.run());
    let records = log.records();
    let first = record(&records, "call-1");
    let shadow = record(&records, "call-1/shadow/compare-large");
    assert_eq!(
        first.latency_ms,
        Some(300),
        "the shadow's 5 s are not the caller's"
    );
    assert_eq!(
        shadow.started_at_unix_ms,
        first.started_at_unix_ms + 300,
        "the shadow starts when the primary has returned"
    );
    assert_eq!(shadow.latency_ms, Some(5_000));

    // A handle dropped undriven sends nothing and holds nothing.
    let Primary { calls, .. } = primary(&env, "call-2");
    drop(calls);
    assert_eq!(models.shadow.calls(), 1);
    assert!(
        log.records()
            .iter()
            .all(|r| r.id.as_str() != "call-2/shadow/compare-large")
    );
}

#[test]
fn a_shadow_past_its_daily_cap_is_skipped_and_recorded() {
    let catalog = catalog();
    let clock = Scripted::at(LAST_MINUTE);
    let models = fleet(&catalog, &clock);
    let log = CallLog::new();
    let prices = prices(&catalog);
    let cancel = Cancel::new();
    let ledger = ledger();
    let shadows = [shadow(&ledger)];
    let env = Env {
        catalog: &catalog,
        models: &models,
        clock: &clock,
        log: &log,
        prices: Some(&prices),
        cancel: &cancel,
        shadows: &shadows,
    };
    let (_, first) = call_and_shadow(&env, "call-1");
    let (second_primary, second) = call_and_shadow(&env, "call-2");

    assert_eq!(first[0].outcome, ShadowOutcome::Sent(Outcome::Succeeded));
    // 0.2 settled plus 0.25 to hold would pass the 0.3 cap.
    assert_eq!(
        second[0].outcome,
        ShadowOutcome::Skipped(BudgetError::LimitExceeded)
    );
    assert_eq!(
        models.shadow.calls(),
        1,
        "the skipped shadow was never sent"
    );
    assert_eq!(
        answer(&second_primary.run),
        [Item::assistant(PRIMARY_ANSWER)]
    );

    let records = log.records();
    let skipped = record(&records, "call-2/shadow/compare-large");
    assert_eq!(skipped.role, Role::Shadow);
    assert_eq!(skipped.outcome, Outcome::Skipped);
    assert_eq!(skipped.route_id, id("coding-shadow"));
    assert_eq!(skipped.route_target_id, None);
    assert_eq!(skipped.attempts, 0);
    assert_eq!(skipped.dispatch, Some(Dispatch::NotSent));
    assert_eq!(skipped.error_code, None);
    assert_eq!(skipped.latency_ms, None);
    assert_eq!(skipped.cost, None);
    assert_eq!(skipped.usage, Usage::default());
    assert_eq!(skipped.primary_record_id, Some(id("call-2")));

    let totals = ledger
        .totals(&id("compare-large"), utc_day(LAST_MINUTE))
        .unwrap();
    assert_eq!(totals.settled_nanos, nanos("0.2"));
    assert_eq!(totals.exposure_nanos, nanos("0.2"));
}

#[test]
fn the_daily_cap_starts_again_at_utc_midnight() {
    let catalog = catalog();
    let clock = Scripted::at(LAST_MINUTE);
    let models = fleet(&catalog, &clock);
    let log = CallLog::new();
    let prices = prices(&catalog);
    let cancel = Cancel::new();
    let ledger = ledger();
    let shadows = [shadow(&ledger)];
    let env = Env {
        catalog: &catalog,
        models: &models,
        clock: &clock,
        log: &log,
        prices: Some(&prices),
        cancel: &cancel,
        shadows: &shadows,
    };
    let (_, first) = call_and_shadow(&env, "call-1");
    clock.set(LAST_MINUTE + 30_000);
    let (_, second) = call_and_shadow(&env, "call-2");
    clock.set(NEXT_DAY);
    let (_, third) = call_and_shadow(&env, "call-3");

    assert_eq!(utc_day(NEXT_DAY), utc_day(LAST_MINUTE) + 1);
    assert_eq!(first[0].outcome, ShadowOutcome::Sent(Outcome::Succeeded));
    assert_eq!(
        second[0].outcome,
        ShadowOutcome::Skipped(BudgetError::LimitExceeded)
    );
    assert_eq!(third[0].outcome, ShadowOutcome::Sent(Outcome::Succeeded));
    assert_eq!(models.shadow.calls(), 2);
    let today = ledger
        .totals(&id("compare-large"), utc_day(NEXT_DAY))
        .unwrap();
    assert_eq!(today.settled_nanos, nanos("0.2"));
    let yesterday = ledger
        .totals(&id("compare-large"), utc_day(LAST_MINUTE))
        .unwrap();
    assert_eq!(yesterday.settled_nanos, nanos("0.2"));
}

#[test]
fn a_clock_set_back_into_an_earlier_day_opens_no_second_cap() {
    let catalog = catalog();
    let clock = Scripted::at(NEXT_DAY);
    let models = fleet(&catalog, &clock);
    let log = CallLog::new();
    let prices = prices(&catalog);
    let cancel = Cancel::new();
    let ledger = ledger();
    let shadows = [shadow(&ledger)];
    let env = Env {
        catalog: &catalog,
        models: &models,
        clock: &clock,
        log: &log,
        prices: Some(&prices),
        cancel: &cancel,
        shadows: &shadows,
    };
    let (_, first) = call_and_shadow(&env, "call-1");
    clock.set(LAST_MINUTE);
    let (_, back) = call_and_shadow(&env, "call-2");

    assert_eq!(first[0].outcome, ShadowOutcome::Sent(Outcome::Succeeded));
    assert_eq!(
        back[0].outcome,
        ShadowOutcome::Skipped(BudgetError::ClockReversed)
    );
    assert_eq!(models.shadow.calls(), 1);
}

#[test]
fn a_failing_shadow_leaves_the_primary_result_unchanged() {
    let catalog = catalog();
    let clock = Scripted::at(LAST_MINUTE);
    let log = CallLog::new();
    let prices = prices(&catalog);
    let cancel = Cancel::new();

    // The same call with no shadow at all.
    let alone = fleet(&catalog, &clock);
    let none: [Shadow; 0] = [];
    let plain = call_and_shadow(
        &Env {
            catalog: &catalog,
            models: &alone,
            clock: &clock,
            log: &log,
            prices: Some(&prices),
            cancel: &cancel,
            shadows: &none,
        },
        "call-plain",
    )
    .0;

    let mut models = fleet(&catalog, &clock);
    models.shadow = models.shadow.failing(
        Error::new(ErrorCode::Unavailable, SHADOW_PROSE).with_dispatch(Dispatch::Accepted),
    );
    let ledger = ledger();
    let shadows = [shadow(&ledger)];
    let env = Env {
        catalog: &catalog,
        models: &models,
        clock: &clock,
        log: &log,
        prices: Some(&prices),
        cancel: &cancel,
        shadows: &shadows,
    };
    let (shadowed, results) = call_and_shadow(&env, "call-1");

    assert_eq!(results[0].outcome, ShadowOutcome::Sent(Outcome::Failed));
    assert_eq!(texts(&shadowed.shown), texts(&plain.shown));
    assert_eq!(answer(&shadowed.run), answer(&plain.run));
    assert_eq!(shadowed.run.halt, plain.run.halt);
    assert_eq!(shadowed.run.attempts.len(), plain.run.attempts.len());
    let records = log.records();
    let failed = record(&records, "call-1/shadow/compare-large");
    assert_eq!(failed.outcome, Outcome::Failed);
    assert_eq!(failed.error_code, Some(ErrorCode::Unavailable));
    assert_eq!(record(&records, "call-1").outcome, Outcome::Succeeded);
    assert!(
        !serde_json::to_string(&records)
            .unwrap()
            .contains(SHADOW_PROSE)
    );
}

#[test]
fn a_shadow_record_names_its_primary_record() {
    let catalog = catalog();
    let clock = Scripted::at(LAST_MINUTE);
    let models = fleet(&catalog, &clock);
    let log = CallLog::new();
    let prices = prices(&catalog);
    let cancel = Cancel::new();
    let ledger = ledger();
    let shadows = [shadow(&ledger)];
    let env = Env {
        catalog: &catalog,
        models: &models,
        clock: &clock,
        log: &log,
        prices: Some(&prices),
        cancel: &cancel,
        shadows: &shadows,
    };
    call_and_shadow(&env, "call-7");

    let records = log.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].id, id("call-7"));
    assert_eq!(records[0].role, Role::Primary);
    assert_eq!(records[0].primary_record_id, None);
    assert_eq!(records[1].role, Role::Shadow);
    assert_eq!(records[1].primary_record_id, Some(id("call-7")));
}

#[test]
fn every_attempt_of_a_retried_shadow_is_settled_on_the_ledger() {
    let catalog = catalog();
    let clock = Scripted::at(LAST_MINUTE);
    let mut models = fleet(&catalog, &clock);
    // The shadow route accepts, fails retriably and answers on the retry: 0.2 twice.
    models.shadow = models.shadow.failing(
        Error::new(ErrorCode::Unavailable, "retry me")
            .with_dispatch(Dispatch::Accepted)
            .with_retriable(true),
    );
    models.shadow.fail_times = Some(1);
    let log = CallLog::new();
    let prices = prices(&catalog);
    let cancel = Cancel::new();
    let ledger = Arc::new(ShadowLedger::new(id("shadow-spend"), usd(), 4).unwrap());
    let shadows = [Shadow::new(
        id("compare-large"),
        id("coding"),
        id("coding-shadow"),
        amount("1"),
        amount("0.25"),
        Arc::clone(&ledger),
    )
    .unwrap()];
    let env = Env {
        catalog: &catalog,
        models: &models,
        clock: &clock,
        log: &log,
        prices: Some(&prices),
        cancel: &cancel,
        shadows: &shadows,
    };
    let (_, results) = call_and_shadow(&env, "call-1");

    assert_eq!(results[0].outcome, ShadowOutcome::Sent(Outcome::Succeeded));
    assert_eq!(models.shadow.calls(), 2);
    let records = log.records();
    let shadow = record(&records, "call-1/shadow/compare-large");
    assert_eq!(shadow.attempts, 2);
    assert_eq!(shadow.cost.as_ref().unwrap().known_subtotal, amount("0.4"));
    let totals = ledger
        .totals(&id("compare-large"), utc_day(LAST_MINUTE))
        .unwrap();
    assert_eq!(totals.settled_nanos, nanos("0.4"), "every attempt is spend");
}

#[test]
fn an_incomplete_shadow_cost_is_held_at_its_reservation_and_never_settled_as_zero() {
    let catalog = catalog();
    let clock = Scripted::at(LAST_MINUTE);
    let mut models = fleet(&catalog, &clock);
    // The far side reported no usage: the shadow's cost is unknown.
    models.shadow.usage = None;
    let log = CallLog::new();
    let prices = prices(&catalog);
    let cancel = Cancel::new();
    let generous = |ledger: &Arc<ShadowLedger>| {
        [Shadow::new(
            id("compare-large"),
            id("coding"),
            id("coding-shadow"),
            amount("10"),
            amount("0.25"),
            Arc::clone(ledger),
        )
        .unwrap()]
    };

    for priced in [true, false] {
        let ledger = ledger();
        let shadows = generous(&ledger);
        let env = Env {
            catalog: &catalog,
            models: &models,
            clock: &clock,
            log: &log,
            // Without a price book no cost is known either.
            prices: priced.then_some(&prices),
            cancel: &cancel,
            shadows: &shadows,
        };
        let (_, first) = call_and_shadow(&env, "call-1");
        let (_, second) = call_and_shadow(&env, "call-2");

        assert_eq!(first[0].outcome, ShadowOutcome::Sent(Outcome::Succeeded));
        let totals = ledger
            .totals(&id("compare-large"), utc_day(LAST_MINUTE))
            .unwrap();
        assert_eq!(
            totals.exposure_nanos,
            nanos("0.25"),
            "held at the reservation"
        );
        assert_eq!(totals.uncertain_count, 1);
        // Spend the ledger cannot know stops further shadows that day, under any cap.
        assert_eq!(
            second[0].outcome,
            ShadowOutcome::Skipped(BudgetError::UncertainSpend)
        );
    }
}

#[test]
fn a_price_book_in_another_currency_settles_nothing_and_holds_the_reservation() {
    let catalog = catalog();
    let clock = Scripted::at(LAST_MINUTE);
    let models = fleet(&catalog, &clock);
    let log = CallLog::new();
    let prices = prices(&catalog);
    let cancel = Cancel::new();
    let ledger =
        Arc::new(ShadowLedger::new(id("shadow-spend"), Currency::new("EUR").unwrap(), 4).unwrap());
    let shadows = [shadow(&ledger)];
    let env = Env {
        catalog: &catalog,
        models: &models,
        clock: &clock,
        log: &log,
        prices: Some(&prices),
        cancel: &cancel,
        shadows: &shadows,
    };
    call_and_shadow(&env, "call-1");
    let totals = ledger
        .totals(&id("compare-large"), utc_day(LAST_MINUTE))
        .unwrap();
    assert_eq!(totals.settled_nanos, "0");
    assert_eq!(totals.exposure_nanos, nanos("0.25"));
    assert_eq!(totals.uncertain_count, 1);
}

#[test]
fn two_shadows_in_flight_cannot_both_take_the_last_reservation() {
    let catalog = catalog();
    let clock = Scripted::at(LAST_MINUTE);
    let gate = AtomicBool::new(false);
    let mut models = fleet(&catalog, &clock);
    models.shadow.gate = Some(&gate);
    let log = CallLog::new();
    let prices = prices(&catalog);
    let cancel = Cancel::new();
    let ledger = ledger();
    let shadows = [shadow(&ledger)];
    let env = Env {
        catalog: &catalog,
        models: &models,
        clock: &clock,
        log: &log,
        prices: Some(&prices),
        cancel: &cancel,
        shadows: &shadows,
    };
    let first = primary(&env, "call-1");
    let second = primary(&env, "call-2");

    // The first shadow is admitted and is still waiting for its answer.
    let mut first_run = std::pin::pin!(first.calls.run());
    let mut context = Context::from_waker(Waker::noop());
    assert!(first_run.as_mut().poll(&mut context).is_pending());
    assert_eq!(models.shadow.calls(), 1);

    // 0.25 held plus 0.25 more would pass 0.3: the second is not sent.
    let second_results = block_on(second.calls.run());
    assert_eq!(
        second_results[0].outcome,
        ShadowOutcome::Skipped(BudgetError::LimitExceeded)
    );
    assert_eq!(models.shadow.calls(), 1);

    gate.store(true, Ordering::SeqCst);
    let first_results = block_on(first_run);
    assert_eq!(
        first_results[0].outcome,
        ShadowOutcome::Sent(Outcome::Succeeded)
    );
}

#[test]
fn the_shadow_handle_is_driven_on_any_thread_the_caller_chooses() {
    fn send<T: Send>(value: T) -> T {
        value
    }
    let catalog = catalog();
    let clock = Scripted::at(LAST_MINUTE);
    let models = fleet(&catalog, &clock);
    let log = CallLog::new();
    let prices = prices(&catalog);
    let cancel = Cancel::new();
    let ledger = ledger();
    let shadows = [shadow(&ledger)];
    let env = Env {
        catalog: &catalog,
        models: &models,
        clock: &clock,
        log: &log,
        prices: Some(&prices),
        cancel: &cancel,
        shadows: &shadows,
    };
    let Primary { calls, .. } = primary(&env, "call-1");
    let detached = send(calls.run());
    let results = std::thread::scope(|scope| scope.spawn(|| block_on(detached)).join().unwrap());
    assert_eq!(results[0].outcome, ShadowOutcome::Sent(Outcome::Succeeded));
}

#[test]
fn only_a_shadow_of_the_primary_route_follows_it_and_a_bad_shadow_route_is_refused() {
    let catalog = catalog();
    let clock = Scripted::at(LAST_MINUTE);
    let models = fleet(&catalog, &clock);
    let log = CallLog::new();
    let prices = prices(&catalog);
    let cancel = Cancel::new();
    let ledger = ledger();
    let make = |shadow_id: &str, primary_route: &str, shadow_route: &str| {
        Shadow::new(
            id(shadow_id),
            id(primary_route),
            id(shadow_route),
            amount("10"),
            amount("0.25"),
            Arc::clone(&ledger),
        )
        .unwrap()
    };
    let shadows = [
        make("other-route", "elsewhere", "coding-shadow"),
        make("no-such-route", "coding", "missing"),
    ];
    let env = Env {
        catalog: &catalog,
        models: &models,
        clock: &clock,
        log: &log,
        prices: Some(&prices),
        cancel: &cancel,
        shadows: &shadows,
    };
    let (_, results) = call_and_shadow(&env, "call-1");

    assert_eq!(
        results,
        [ShadowResult {
            shadow_id: id("no-such-route"),
            shadow_route_id: id("missing"),
            record_id: None,
            outcome: ShadowOutcome::Refused(ErrorCode::InvalidRequest),
        }]
    );
    assert_eq!(models.shadow.calls(), 0);
    assert_eq!(log.records().len(), 1);
    assert_eq!(
        ledger.totals(&id("no-such-route"), utc_day(LAST_MINUTE)),
        None
    );
}

#[test]
fn a_shadow_reserving_nothing_or_more_than_its_cap_is_refused() {
    let ledger = ledger();
    let make = |cap: &str, reserve: &str| {
        Shadow::new(
            id("compare-large"),
            id("coding"),
            id("coding-shadow"),
            amount(cap),
            amount(reserve),
            Arc::clone(&ledger),
        )
    };
    assert!(make("1", "0").is_err());
    assert!(make("1", "1.000000001").is_err());
    assert!(make("1", "1").is_ok());
    assert!(ShadowLedger::new(id("shadow-spend"), usd(), 0).is_err());
}
