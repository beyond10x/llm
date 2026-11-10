//! Adversary cases for `ShadowLedger::durable` (story:durable-shadow-cap): whether any sequence
//! across a restart, a crash, a changed cap, a backwards clock, an escaped shadow id or a
//! symlinked directory spends past a day's cap or sends a shadow the ledger skipped.
#![cfg(feature = "sqlite")]

use llm_calls::{
    CallIdentity, CallLog, Clock, Outcome, Recording, Shadow, ShadowCalls, ShadowLedger,
    ShadowOutcome, ShadowResult, Shadowing, record_turn_shadowed, utc_day,
};
use llm_core::{
    BoxFuture, Cancel, Capabilities, Error, Id, Item, Model, Provenance, StopReason, StreamEvent,
    StreamSink, TurnObservation, TurnOutcome, TurnRequest, Usage, VecSink,
};
use llm_cost::budget::BudgetError;
use llm_cost::{Amount, Currency, ModelPrice, PriceBook, PriceDocument, Rate};
use llm_routing::{Catalog, CatalogDocument, FallbackPolicy, Models, Ports, Route, RouteTarget};
use std::{
    future::{Future, poll_fn},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
    time::Duration,
};

const EXAMPLE: &str = include_str!("../../../examples/catalog.toml");

/// 2025-11-05T23:59:00Z.
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

struct Scripted {
    unix_ms: AtomicU64,
}

impl Scripted {
    const fn at(unix_ms: u64) -> Self {
        Self {
            unix_ms: AtomicU64::new(unix_ms),
        }
    }
    fn set(&self, unix_ms: u64) {
        self.unix_ms.store(unix_ms, Ordering::SeqCst);
    }
}

impl Clock for Scripted {
    fn unix_ms(&self) -> u64 {
        self.unix_ms.load(Ordering::SeqCst)
    }
    fn monotonic(&self) -> Duration {
        Duration::ZERO
    }
}

/// Answers every turn with 100 input and 50 output tokens (0.2 USD), after `gate` opens.
struct Fake<'c> {
    provenance: Provenance,
    capabilities: Capabilities,
    gate: Option<&'c AtomicBool>,
    calls: AtomicUsize,
}

impl Fake<'_> {
    fn on(catalog: &Catalog, serving: &str) -> Self {
        let binding = catalog.binding(&id(serving)).unwrap();
        Self {
            provenance: binding.provenance().clone(),
            capabilities: binding.capabilities().clone(),
            gate: None,
            calls: AtomicUsize::new(0),
        }
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
            self.calls.fetch_add(1, Ordering::SeqCst);
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

fn fleet<'c>(catalog: &Catalog) -> Fleet<'c> {
    Fleet {
        primary: Fake::on(catalog, "local-small"),
        shadow: Fake::on(catalog, "remote-large"),
    }
}

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

/// A daily cap of `cap` with 0.25 held per call; at a cap of 0.3, one 0.2 shadow fits a day.
fn shadow_named(name: &str, cap: &str, ledger: &Arc<ShadowLedger>) -> Shadow {
    Shadow::new(
        id(name),
        id("coding"),
        id("coding-shadow"),
        amount(cap),
        amount("0.25"),
        Arc::clone(ledger),
    )
    .unwrap()
}

fn durable(dir: &Path, max_active: u32) -> Arc<ShadowLedger> {
    Arc::new(ShadowLedger::durable(dir, id("shadow-spend"), usd(), max_active).unwrap())
}

fn no_wait(_: Duration) -> BoxFuture<'static, ()> {
    Box::pin(async {})
}

struct World {
    catalog: Catalog,
    clock: Scripted,
    log: CallLog,
    prices: PriceBook,
    cancel: Cancel,
}

impl World {
    fn at(unix_ms: u64) -> Self {
        let catalog = catalog();
        let prices = prices(&catalog);
        Self {
            catalog,
            clock: Scripted::at(unix_ms),
            log: CallLog::new(),
            prices,
            cancel: Cancel::new(),
        }
    }

    /// One primary call; its shadows returned undriven.
    fn primary<'w>(
        &'w self,
        models: &'w Fleet<'w>,
        shadows: &'w [Shadow],
        record_id: &str,
    ) -> ShadowCalls<'w> {
        let mut sink = VecSink::new(64, 64 * 1024);
        let mut admit = |_: &llm_routing::Selection<'_>| Ok(());
        let mut request = TurnRequest::new("code", vec![Item::user("hello")]);
        request.max_output_tokens = Some(128);
        let (_, calls) = block_on(record_turn_shadowed(
            &self.catalog,
            &request,
            Some(100),
            FallbackPolicy::default(),
            Ports {
                models,
                admit: &mut admit,
                sink: &mut sink,
                cancel: &self.cancel,
                pause: &no_wait,
            },
            CallIdentity::primary(id(record_id)),
            Recording {
                sink: &self.log,
                clock: &self.clock,
                prices: Some(&self.prices),
            },
            Shadowing {
                shadows,
                models,
                cancel: &self.cancel,
                pause: &no_wait,
            },
        ))
        .unwrap();
        calls
    }

    fn shadowed(&self, models: &Fleet<'_>, shadows: &[Shadow], record_id: &str) -> Vec<ShadowResult> {
        block_on(self.primary(models, shadows, record_id).run())
    }
}

/// The in-process lane refuses a day earlier than the latest one it admitted on, so a clock set
/// back across midnight cannot open a second fresh cap. A durable ledger is documented as never
/// starting a cap afresh; a restart with the clock set back across UTC midnight must not get a
/// fresh earlier day either, or one real day's spend reaches twice the cap.
#[test]
fn a_restart_with_the_clock_set_back_a_day_does_not_open_a_fresh_cap() {
    let dir = tempfile::tempdir().unwrap();
    let world = World::at(NEXT_DAY);
    let models = fleet(&world.catalog);

    // The in-process baseline: one ledger, day D then day D-1, refuses the earlier day.
    let memory = Arc::new(ShadowLedger::new(id("shadow-spend"), usd(), 4).unwrap());
    let in_memory = [shadow_named("compare-large", "0.3", &memory)];
    let sent = world.shadowed(&models, &in_memory, "mem-1");
    assert_eq!(sent[0].outcome, ShadowOutcome::Sent(Outcome::Succeeded));
    world.clock.set(LAST_MINUTE);
    let back = world.shadowed(&models, &in_memory, "mem-2");
    assert_eq!(
        back[0].outcome,
        ShadowOutcome::Skipped(BudgetError::ClockReversed)
    );
    let baseline_calls = models.shadow.calls();
    assert_eq!(baseline_calls, 1);

    // The durable lane: day D spent to its cap, then a restart with the clock back on D-1.
    world.clock.set(NEXT_DAY);
    let first = durable(dir.path(), 4);
    let shadows = [shadow_named("compare-large", "0.3", &first)];
    let sent = world.shadowed(&models, &shadows, "call-1");
    assert_eq!(sent[0].outcome, ShadowOutcome::Sent(Outcome::Succeeded));
    let full = world.shadowed(&models, &shadows, "call-2");
    assert_eq!(
        full[0].outcome,
        ShadowOutcome::Skipped(BudgetError::LimitExceeded)
    );
    drop(shadows);
    drop(first);

    world.clock.set(LAST_MINUTE);
    let second = durable(dir.path(), 4);
    let shadows = [shadow_named("compare-large", "0.3", &second)];
    let back = world.shadowed(&models, &shadows, "call-3");
    assert_eq!(
        models.shadow.calls(),
        baseline_calls + 1,
        "a shadow was sent on a fresh cap for day {} after day {} was spent to its cap",
        utc_day(LAST_MINUTE),
        utc_day(NEXT_DAY),
    );
    assert_eq!(
        back[0].outcome,
        ShadowOutcome::Skipped(BudgetError::ClockReversed)
    );
}

/// Within one day, a restart whose clock reads before the day's last write is refused.
#[test]
fn a_restart_with_the_clock_set_back_within_the_day_skips_the_shadow() {
    let dir = tempfile::tempdir().unwrap();
    let world = World::at(LAST_MINUTE + 30_000);
    let models = fleet(&world.catalog);
    let first = durable(dir.path(), 4);
    let shadows = [shadow_named("compare-large", "0.6", &first)];
    let sent = world.shadowed(&models, &shadows, "call-1");
    assert_eq!(sent[0].outcome, ShadowOutcome::Sent(Outcome::Succeeded));
    drop(shadows);
    drop(first);

    world.clock.set(LAST_MINUTE);
    let second = durable(dir.path(), 4);
    let shadows = [shadow_named("compare-large", "0.6", &second)];
    let back = world.shadowed(&models, &shadows, "call-2");
    assert_eq!(
        back[0].outcome,
        ShadowOutcome::Skipped(BudgetError::ClockReversed)
    );
    assert_eq!(models.shadow.calls(), 1);
}

/// A process that dies with a shadow in flight leaves the reservation started; the restart marks
/// it uncertain and admits no further shadow that day.
#[test]
fn a_crash_with_a_shadow_in_flight_admits_nothing_more_after_the_restart() {
    let dir = tempfile::tempdir().unwrap();
    let world = World::at(LAST_MINUTE);
    let gate = AtomicBool::new(false);
    let mut models = fleet(&world.catalog);
    models.shadow.gate = Some(&gate);
    let first = durable(dir.path(), 4);
    let shadows = [shadow_named("compare-large", "0.6", &first)];
    {
        let calls = world.primary(&models, &shadows, "call-1");
        let mut run = std::pin::pin!(calls.run());
        let mut context = Context::from_waker(Waker::noop());
        assert!(run.as_mut().poll(&mut context).is_pending());
        assert_eq!(models.shadow.calls(), 1);
    }
    drop(shadows);
    drop(first);

    gate.store(true, Ordering::SeqCst);
    world.clock.set(LAST_MINUTE + 1_000);
    let second = durable(dir.path(), 4);
    let shadows = [shadow_named("compare-large", "0.6", &second)];
    let after = world.shadowed(&models, &shadows, "call-2");
    assert_eq!(
        after[0].outcome,
        ShadowOutcome::Skipped(BudgetError::UncertainSpend)
    );
    assert_eq!(models.shadow.calls(), 1);
    let totals = second
        .totals(&id("compare-large"), utc_day(LAST_MINUTE))
        .unwrap();
    assert_eq!(totals.exposure_nanos, amount("0.25").nanos().to_string());
}

/// A cap or concurrency bound changed between runs does not start the day afresh.
#[test]
fn a_changed_cap_or_bound_between_runs_skips_the_shadow() {
    let dir = tempfile::tempdir().unwrap();
    let world = World::at(LAST_MINUTE);
    let models = fleet(&world.catalog);
    let first = durable(dir.path(), 4);
    let shadows = [shadow_named("compare-large", "0.3", &first)];
    let sent = world.shadowed(&models, &shadows, "call-1");
    assert_eq!(sent[0].outcome, ShadowOutcome::Sent(Outcome::Succeeded));
    drop(shadows);
    drop(first);

    world.clock.set(LAST_MINUTE + 1_000);
    let raised = durable(dir.path(), 4);
    let shadows = [shadow_named("compare-large", "0.9", &raised)];
    let refused = world.shadowed(&models, &shadows, "call-2");
    assert_eq!(
        refused[0].outcome,
        ShadowOutcome::Skipped(BudgetError::PolicyMismatch)
    );
    drop(shadows);
    drop(raised);

    let rebound = durable(dir.path(), 9);
    let shadows = [shadow_named("compare-large", "0.3", &rebound)];
    let refused = world.shadowed(&models, &shadows, "call-3");
    assert_eq!(
        refused[0].outcome,
        ShadowOutcome::Skipped(BudgetError::PolicyMismatch)
    );
    assert_eq!(models.shadow.calls(), 1);
}

/// Every escaped or digested id names one entry inside the directory, distinct per id: `.`,
/// `..`, a slash, a `%` that could imitate an escape, and two 256-byte ids differing only in
/// their last byte.
#[test]
fn escaped_and_digested_shadow_ids_stay_inside_and_never_share_a_cap() {
    let outer = tempfile::tempdir().unwrap();
    let dir = outer.path().join("ledgers");
    let world = World::at(LAST_MINUTE);
    let models = fleet(&world.catalog);
    let ledger = durable(&dir, 4);
    let long_a = format!("{}a", "/".repeat(255));
    let long_b = format!("{}b", "/".repeat(255));
    let names = [".", "..", "a/b", "a%2fb", "%2e%2e", long_a.as_str(), long_b.as_str()];
    let shadows: Vec<Shadow> = names
        .iter()
        .map(|name| shadow_named(name, "0.3", &ledger))
        .collect();
    let results = world.shadowed(&models, &shadows, "call-1");
    for result in &results {
        assert_eq!(
            result.outcome,
            ShadowOutcome::Sent(Outcome::Succeeded),
            "{}",
            result.shadow_id
        );
    }
    let mut entries: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    entries.sort();
    assert_eq!(entries.len(), names.len(), "{entries:?}");
    assert!(entries.iter().all(|name| !name.is_empty() && !name.contains('/') && name != "." && name != ".."));
    assert_eq!(
        entries.iter().filter(|name| name.starts_with("%sha256-")).count(),
        2
    );
    assert_eq!(std::fs::read_dir(outer.path()).unwrap().count(), 1);
}

/// A directory reached through a symlink is the same ledger as its target: the owner lock holds
/// across the two spellings, and the day's spend survives a reopen through the other one.
#[cfg(unix)]
#[test]
fn a_symlinked_directory_is_the_same_ledger_as_its_target() {
    let outer = tempfile::tempdir().unwrap();
    let real = outer.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let link = outer.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let world = World::at(LAST_MINUTE);
    let models = fleet(&world.catalog);

    let through_link = durable(&link, 4);
    let linked = [shadow_named("compare-large", "0.3", &through_link)];
    let sent = world.shadowed(&models, &linked, "call-1");
    assert_eq!(sent[0].outcome, ShadowOutcome::Sent(Outcome::Succeeded));

    let through_real = durable(&real, 4);
    let direct = [shadow_named("compare-large", "0.3", &through_real)];
    let busy = world.shadowed(&models, &direct, "call-2");
    assert_eq!(
        busy[0].outcome,
        ShadowOutcome::Skipped(BudgetError::OwnerBusy)
    );
    drop(linked);
    drop(through_link);

    world.clock.set(LAST_MINUTE + 1_000);
    let full = world.shadowed(&models, &direct, "call-3");
    assert_eq!(
        full[0].outcome,
        ShadowOutcome::Skipped(BudgetError::LimitExceeded)
    );
    assert_eq!(models.shadow.calls(), 1);
}
