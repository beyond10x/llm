//! Adversary cases for shadow calls: which primary calls a shadow follows, how long one UTC day's
//! cap holds, and whether every sent-or-skipped shadow gets a record naming its primary.

use llm_calls::{
    CallIdentity, CallLog, CallRecord, Clock, Outcome, Recording, Role, Shadow, ShadowCalls,
    ShadowLedger, ShadowOutcome, Shadowing, record_turn_shadowed, utc_day,
};
use llm_core::{
    BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item, Model, Provenance,
    StopReason, StreamEvent, StreamSink, TurnObservation, TurnOutcome, TurnRequest, Usage, VecSink,
};
use llm_cost::budget::BudgetError;
use llm_cost::{Amount, Currency, ModelPrice, PriceBook, PriceDocument, Rate};
use llm_routing::{
    Catalog, CatalogDocument, FallbackPolicy, FallbackRun, Models, Ports, Route, RouteTarget,
    Selection,
};
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
    time::Duration,
};

const EXAMPLE: &str = include_str!("../../../examples/catalog.toml");

/// 2025-11-05T00:00:00Z.
const DAY_START: u64 = 1_762_300_800_000;
/// 2025-11-05T00:00:30Z, in the first hour of the day.
const EARLY: u64 = DAY_START + 30_000;
/// 2025-11-05T23:59:30Z, in the last hour of the same UTC day.
const LATE: u64 = DAY_START + 86_370_000;

fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}

fn amount(value: &str) -> Amount {
    Amount::parse(value).unwrap()
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

/// Route `coding` (alias `code`) on `local-small`; route `coding-shadow` (alias `code-shadow`)
/// on `remote-large`.
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

/// A wall clock a test sets; the monotonic reading never moves.
struct Wall(AtomicU64);

impl Clock for Wall {
    fn unix_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
    fn monotonic(&self) -> Duration {
        Duration::ZERO
    }
}

/// Answers every attempt with one text and 100 in / 50 out, counting the attempts it saw.
struct Fake {
    provenance: Provenance,
    capabilities: Capabilities,
    calls: AtomicUsize,
}

impl Fake {
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

impl Model for Fake {
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
                text: "answer".to_owned(),
            })
            .await?;
            let mut observation = TurnObservation::new(self.provenance.clone());
            observation.usage = Some(Usage {
                input_tokens: Some(100),
                output_tokens: Some(50),
                cached_input_tokens: Some(0),
                cache_creation_input_tokens: Some(0),
                reasoning_output_tokens: None,
            });
            observation.upstream_model = Some(id("example/Any-Model"));
            observation.final_usage = true;
            Ok(TurnOutcome {
                stop_reason: StopReason::EndTurn,
                items: vec![Item::assistant("answer")],
                observation,
            })
        })
    }
}

struct Fleet {
    primary: Fake,
    shadow: Fake,
}

impl Models for Fleet {
    fn model(&self, serving_model_id: &Id) -> Option<&dyn Model> {
        match serving_model_id.as_str() {
            "local-small" => Some(&self.primary),
            "remote-large" => Some(&self.shadow),
            _ => None,
        }
    }
}

/// 100 in and 50 out cost 0.2 USD on either serving model.
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
        Currency::new("USD").unwrap(),
        vec![
            price("price-small", "local-small"),
            price("price-large", "remote-large"),
        ],
    ))
    .unwrap()
}

/// A 0.3 daily cap with 0.25 held per call: one 0.2 shadow fits a day, a second does not.
fn shadows() -> (Arc<ShadowLedger>, [Shadow; 1]) {
    let ledger =
        Arc::new(ShadowLedger::new(id("shadow-spend"), Currency::new("USD").unwrap(), 4).unwrap());
    let shadow = Shadow::new(
        id("compare-large"),
        id("coding"),
        id("coding-shadow"),
        amount("0.3"),
        amount("0.25"),
        Arc::clone(&ledger),
    )
    .unwrap();
    (ledger, [shadow])
}

fn no_wait(_: Duration) -> BoxFuture<'static, ()> {
    Box::pin(async {})
}

struct Env<'e> {
    catalog: &'e Catalog,
    models: &'e Fleet,
    clock: &'e Wall,
    log: &'e CallLog,
    prices: &'e PriceBook,
    cancel: &'e Cancel,
    shadows: &'e [Shadow],
}

/// One primary call through `record_turn_shadowed`, its admission decided by `admit`.
fn primary<'e>(
    env: &Env<'e>,
    record_id: &str,
    admit: &mut (dyn FnMut(&Selection<'_>) -> Result<(), Error> + Send),
) -> (FallbackRun, ShadowCalls<'e>) {
    let mut sink = VecSink::new(64, 64 * 1024);
    let mut request = TurnRequest::new("code", vec![Item::user("hello")]);
    request.max_output_tokens = Some(128);
    block_on(record_turn_shadowed(
        env.catalog,
        &request,
        Some(100),
        FallbackPolicy::default(),
        Ports {
            models: env.models,
            admit,
            sink: &mut sink,
            cancel: env.cancel,
            pause: &no_wait,
        },
        CallIdentity::primary(id(record_id)),
        Recording {
            sink: env.log,
            clock: env.clock,
            prices: Some(env.prices),
        },
        Shadowing {
            shadows: env.shadows,
            models: env.models,
            cancel: env.cancel,
            pause: &no_wait,
        },
    ))
    .unwrap()
}

fn record<'r>(records: &'r [CallRecord], record_id: &str) -> Option<&'r CallRecord> {
    records
        .iter()
        .find(|record| record.id.as_str() == record_id)
}

/// The caller's own admission hook refuses the primary before anything is sent: the primary
/// never reached a model, so there is no answer for a shadow to be compared with. Driving the
/// handle must not send the request to the shadow route all the same.
#[test]
fn a_shadow_does_not_follow_a_primary_the_callers_admission_refused_before_sending() {
    let catalog = catalog();
    let clock = Wall(AtomicU64::new(EARLY));
    let models = Fleet {
        primary: Fake::on(&catalog, "local-small"),
        shadow: Fake::on(&catalog, "remote-large"),
    };
    let log = CallLog::new();
    let prices = prices(&catalog);
    let cancel = Cancel::new();
    let (_ledger, shadows) = shadows();
    let env = Env {
        catalog: &catalog,
        models: &models,
        clock: &clock,
        log: &log,
        prices: &prices,
        cancel: &cancel,
        shadows: &shadows,
    };
    let mut refuse =
        |_: &Selection<'_>| Err(Error::new(ErrorCode::Refused, "the caller's limit refused"));
    let (run, calls) = primary(&env, "call-1", &mut refuse);

    // The primary was refused by its caller and sent nowhere.
    assert!(run.result.is_err());
    assert!(run.attempts.is_empty());
    assert_eq!(models.primary.calls(), 0);
    let records = log.records();
    let first = record(&records, "call-1").unwrap();
    assert_eq!(first.attempts, 0);
    assert_eq!(first.dispatch, Some(Dispatch::NotSent));

    let results = block_on(calls.run());
    assert_eq!(
        models.shadow.calls(),
        0,
        "a primary refused before sending got a paid shadow call: {results:?}"
    );
}

/// The cap holds for the whole UTC day, not only until the next hour or minute: the shadow tests
/// observe the rollover only across 23:59 -> 00:00, which every period dividing a day shares, so a
/// `DAY_MS` of one hour keeps them green. Here the cap is spent at 00:00:30 and a shadow at
/// 23:59:30 of the same day is skipped.
#[test]
fn the_daily_cap_holds_from_the_first_to_the_last_hour_of_one_utc_day() {
    assert_eq!(utc_day(DAY_START), 20_397);
    assert_eq!(utc_day(LATE), 20_397);
    assert_eq!(utc_day(DAY_START + 86_400_000), 20_398);

    let catalog = catalog();
    let clock = Wall(AtomicU64::new(EARLY));
    let models = Fleet {
        primary: Fake::on(&catalog, "local-small"),
        shadow: Fake::on(&catalog, "remote-large"),
    };
    let log = CallLog::new();
    let prices = prices(&catalog);
    let cancel = Cancel::new();
    let (ledger, shadows) = shadows();
    let env = Env {
        catalog: &catalog,
        models: &models,
        clock: &clock,
        log: &log,
        prices: &prices,
        cancel: &cancel,
        shadows: &shadows,
    };
    let mut admit = |_: &Selection<'_>| Ok(());
    let (_, calls) = primary(&env, "call-1", &mut admit);
    let early = block_on(calls.run());
    clock.0.store(LATE, Ordering::SeqCst);
    let (_, calls) = primary(&env, "call-2", &mut admit);
    let late = block_on(calls.run());

    assert_eq!(early[0].outcome, ShadowOutcome::Sent(Outcome::Succeeded));
    assert_eq!(
        late[0].outcome,
        ShadowOutcome::Skipped(BudgetError::LimitExceeded)
    );
    assert_eq!(models.shadow.calls(), 1);
    let totals = ledger.totals(&id("compare-large"), 20_397).unwrap();
    assert_eq!(totals.settled_nanos, amount("0.2").nanos().to_string());
}

/// A primary record id is any valid id, up to its 256-byte limit. Its shadow is the shadow
/// route's call all the same: sent, or skipped, and recorded naming the primary. Not silently
/// dropped as if the shadow route had refused the request.
#[test]
fn a_shadow_of_a_primary_with_a_long_record_id_is_still_recorded() {
    let catalog = catalog();
    let clock = Wall(AtomicU64::new(EARLY));
    let models = Fleet {
        primary: Fake::on(&catalog, "local-small"),
        shadow: Fake::on(&catalog, "remote-large"),
    };
    let log = CallLog::new();
    let prices = prices(&catalog);
    let cancel = Cancel::new();
    let (_ledger, shadows) = shadows();
    let env = Env {
        catalog: &catalog,
        models: &models,
        clock: &clock,
        log: &log,
        prices: &prices,
        cancel: &cancel,
        shadows: &shadows,
    };
    let long_id = format!("req-{}", "0123456789abcdef".repeat(15));
    assert_eq!(long_id.len(), 244);
    let mut admit = |_: &Selection<'_>| Ok(());
    let (run, calls) = primary(&env, &long_id, &mut admit);
    assert!(run.result.is_ok());
    let results = block_on(calls.run());

    let records = log.records();
    let shadow = records
        .iter()
        .find(|record| record.role == Role::Shadow)
        .unwrap_or_else(|| panic!("no shadow record; the shadow ended {results:?}"));
    assert_eq!(shadow.primary_record_id, Some(id(&long_id)));
}
