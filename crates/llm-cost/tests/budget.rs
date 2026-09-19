use llm_core::{BillingKind, Id, Protocol, Provenance};
use llm_cost::{Amount, ChargeKind, Currency, RecordedCharge, budget::*};

fn id(v: &str) -> Id {
    Id::new(v).unwrap()
}
fn amount(v: u64) -> Amount {
    Amount::from_nanos(v)
}
fn policy() -> BudgetPolicy {
    BudgetPolicy {
        id: id("scope"),
        currency: Currency::new("USD").unwrap(),
        limit: amount(100),
        max_active: 4,
    }
}
fn request(name: &str, reserve: u64) -> ReservationRequest {
    ReservationRequest {
        id: id(name),
        operation: Operation::Inference {
            binding: Provenance {
                protocol: Protocol::ChatCompletions,
                provider: id("vendor"),
                account: id("account"),
                endpoint: id("endpoint"),
                model: id("model"),
                binding_revision: id("binding-v1"),
            },
            billing: BillingKind::Metered,
            request_id: id("request"),
        },
        reserved: amount(reserve),
        assumption: id("price-v1"),
        expires_at_ms: 100,
    }
}
fn compute(name: &str, reserve: u64) -> ReservationRequest {
    let mut r = request(name, reserve);
    r.operation = Operation::Compute {
        deployment: id("owned-pod"),
        provider: id("hosting"),
        account: id("owner"),
    };
    r
}
fn reserve(e: &mut BudgetEngine, r: ReservationRequest) {
    e.apply(1, BudgetCommand::Reserve { request: r }).unwrap();
}
fn start(e: &mut BudgetEngine, name: &str) {
    let permit = e
        .apply(2, BudgetCommand::Begin { id: id(name) })
        .unwrap()
        .permit
        .unwrap();
    assert_eq!(permit.ledger(), &id("scope"));
    assert_eq!(permit.reservation(), &id(name));
}
fn settlement(known: u64, complete: bool) -> Settlement {
    Settlement {
        currency: Currency::new("USD").unwrap(),
        known: amount(known),
        complete,
        evidence: id("observed"),
    }
}
fn charge(name: &str, known: Option<u64>) -> RecordedCharge {
    RecordedCharge {
        id: id(name),
        source: "operator invoice fixture".into(),
        provider: id("vendor"),
        account: id("account"),
        subject: id("subscription"),
        period_start_unix_ms: 0,
        period_end_unix_ms: 100,
        currency: Currency::new("USD").unwrap(),
        amount: known.map(amount),
        charge_kind: ChargeKind::Subscription,
    }
}

#[test]
fn reservations_hold_the_inclusive_cap_and_start_exactly_once() {
    let mut e = BudgetEngine::new(policy()).unwrap();
    reserve(&mut e, request("a", 60));
    reserve(&mut e, request("b", 40));
    assert_eq!(e.view().unwrap().totals.exposure_nanos, "100");
    assert_eq!(
        e.apply(
            1,
            BudgetCommand::Reserve {
                request: request("c", 1)
            }
        )
        .unwrap_err(),
        BudgetError::LimitExceeded
    );
    start(&mut e, "a");
    assert_eq!(
        e.apply(2, BudgetCommand::Begin { id: id("a") })
            .unwrap_err(),
        BudgetError::WrongPhase
    );
    assert_eq!(
        e.apply(2, BudgetCommand::Cancel { id: id("a") })
            .unwrap_err(),
        BudgetError::WrongPhase
    );
    e.apply(2, BudgetCommand::Cancel { id: id("b") }).unwrap();
    assert_eq!(
        e.apply(
            2,
            BudgetCommand::Reserve {
                request: request("b", 1)
            }
        )
        .unwrap_err(),
        BudgetError::Duplicate
    );
    assert_eq!(e.view().unwrap().totals.held_nanos, "60");
}
#[test]
fn unknown_settlement_retains_exposure_and_overrun_stops_admission() {
    let mut e = BudgetEngine::new(policy()).unwrap();
    reserve(&mut e, request("a", 90));
    start(&mut e, "a");
    e.apply(
        3,
        BudgetCommand::Settle {
            id: id("a"),
            settlement: settlement(20, false),
        },
    )
    .unwrap();
    let totals = e.view().unwrap().totals;
    assert_eq!(
        (
            totals.settled_nanos.as_str(),
            totals.held_nanos.as_str(),
            totals.exposure_nanos.as_str()
        ),
        ("20", "70", "90")
    );
    assert_eq!(
        e.apply(
            3,
            BudgetCommand::Reserve {
                request: request("b", 0)
            }
        )
        .unwrap_err(),
        BudgetError::UncertainSpend
    );
    assert_eq!(
        e.apply(
            3,
            BudgetCommand::Settle {
                id: id("a"),
                settlement: settlement(19, true)
            }
        )
        .unwrap_err(),
        BudgetError::InvalidCharge
    );
    e.apply(
        4,
        BudgetCommand::Settle {
            id: id("a"),
            settlement: settlement(120, true),
        },
    )
    .unwrap();
    assert!(e.view().unwrap().totals.above_limit);
    assert_eq!(
        e.apply(
            4,
            BudgetCommand::Reserve {
                request: request("b", 0)
            }
        )
        .unwrap_err(),
        BudgetError::LimitExceeded
    );
}
#[test]
fn compute_cannot_release_until_stop_and_complete_cost_are_observed() {
    let mut e = BudgetEngine::new(policy()).unwrap();
    reserve(&mut e, compute("pod", 80));
    assert_eq!(
        e.apply(
            1,
            BudgetCommand::Reserve {
                request: compute("other", 1)
            }
        )
        .unwrap_err(),
        BudgetError::ResourceBusy
    );
    start(&mut e, "pod");
    assert_eq!(
        e.apply(
            3,
            BudgetCommand::Settle {
                id: id("pod"),
                settlement: settlement(0, true)
            }
        )
        .unwrap_err(),
        BudgetError::WrongPhase
    );
    assert_eq!(
        e.apply(
            4,
            BudgetCommand::Renew {
                id: id("pod"),
                additional: amount(21),
                expires_at_ms: 200,
                assumption: id("renewal")
            }
        )
        .unwrap_err(),
        BudgetError::LimitExceeded
    );
    assert_eq!(e.view().unwrap().totals.stop_required, vec![id("pod")]);
    assert_eq!(e.view().unwrap().totals.held_nanos, "80");
    e.apply(200, BudgetCommand::Tick).unwrap();
    assert_eq!(e.view().unwrap().totals.held_nanos, "80");
    e.apply(
        200,
        BudgetCommand::ConfirmStopped {
            id: id("pod"),
            evidence: id("control-plane-stopped"),
        },
    )
    .unwrap();
    assert_eq!(e.reservation(&id("pod")).unwrap().phase, Phase::Stopped);
    assert_eq!(e.view().unwrap().totals.uncertain_count, 1);
    e.apply(
        200,
        BudgetCommand::Settle {
            id: id("pod"),
            settlement: settlement(85, true),
        },
    )
    .unwrap();
    assert_eq!(e.view().unwrap().totals.exposure_nanos, "85");
}
#[test]
fn expiry_and_clock_regression_do_not_manufacture_free_work() {
    let mut e = BudgetEngine::new(policy()).unwrap();
    reserve(&mut e, request("idle", 10));
    reserve(&mut e, request("live", 30));
    reserve(&mut e, compute("pod", 40));
    start(&mut e, "live");
    start(&mut e, "pod");
    e.apply(100, BudgetCommand::Tick).unwrap();
    assert_eq!(e.reservation(&id("idle")).unwrap().phase, Phase::Cancelled);
    assert_eq!(e.reservation(&id("live")).unwrap().phase, Phase::Uncertain);
    assert_eq!(
        e.reservation(&id("pod")).unwrap().phase,
        Phase::StopRequired
    );
    assert_eq!(e.view().unwrap().totals.exposure_nanos, "70");
    assert_eq!(
        e.apply(99, BudgetCommand::Tick).unwrap_err(),
        BudgetError::ClockReversed
    );
    assert_eq!(e.view().unwrap().now_ms, 100);
}
#[test]
fn subscriptions_are_posted_once_and_unknown_charges_block_until_reconciled() {
    let mut e = BudgetEngine::new(policy()).unwrap();
    e.apply(
        0,
        BudgetCommand::PostCharge {
            charge: charge("fee", None),
        },
    )
    .unwrap();
    assert_eq!(
        e.apply(
            0,
            BudgetCommand::PostCharge {
                charge: charge("fee", Some(50))
            }
        )
        .unwrap_err(),
        BudgetError::Duplicate
    );
    assert_eq!(
        e.apply(
            0,
            BudgetCommand::Reserve {
                request: request("a", 0)
            }
        )
        .unwrap_err(),
        BudgetError::UncertainSpend
    );
    e.apply(
        0,
        BudgetCommand::ReconcileCharge {
            id: id("fee"),
            amount: amount(50),
            evidence: id("invoice"),
        },
    )
    .unwrap();
    let mut call = request("a", 0);
    if let Operation::Inference { billing, .. } = &mut call.operation {
        *billing = BillingKind::Subscription;
    }
    reserve(&mut e, call);
    start(&mut e, "a");
    e.apply(
        2,
        BudgetCommand::Settle {
            id: id("a"),
            settlement: settlement(0, true),
        },
    )
    .unwrap();
    assert_eq!(e.view().unwrap().totals.settled_nanos, "50");
    let mut wrong_currency = charge("bad", Some(1));
    wrong_currency.currency = Currency::new("EUR").unwrap();
    assert_eq!(
        e.apply(
            2,
            BudgetCommand::PostCharge {
                charge: wrong_currency
            }
        )
        .unwrap_err(),
        BudgetError::InvalidCharge
    );
}
#[test]
fn aggregate_overruns_are_exact_above_the_single_amount_range() {
    let mut e = BudgetEngine::new(policy()).unwrap();
    e.apply(
        0,
        BudgetCommand::PostCharge {
            charge: charge("one", Some(u64::MAX)),
        },
    )
    .unwrap();
    e.apply(
        0,
        BudgetCommand::PostCharge {
            charge: charge("two", Some(u64::MAX)),
        },
    )
    .unwrap();
    assert_eq!(
        e.view().unwrap().totals.settled_nanos,
        "36893488147419103230"
    );
    assert!(e.view().unwrap().totals.above_limit);
}
#[test]
fn renew_reserves_the_added_amount_and_concurrency_counts_unresolved_work() {
    let mut p = policy();
    p.max_active = 1;
    let mut e = BudgetEngine::new(p).unwrap();
    reserve(&mut e, compute("pod", 60));
    start(&mut e, "pod");
    e.apply(
        3,
        BudgetCommand::Renew {
            id: id("pod"),
            additional: amount(40),
            expires_at_ms: 200,
            assumption: id("renewal-v2"),
        },
    )
    .unwrap();
    assert_eq!(e.view().unwrap().totals.exposure_nanos, "100");
    assert_eq!(
        e.apply(
            3,
            BudgetCommand::Reserve {
                request: request("b", 0)
            }
        )
        .unwrap_err(),
        BudgetError::ConcurrencyExceeded
    );
    e.apply(100, BudgetCommand::Tick).unwrap();
    assert_eq!(e.reservation(&id("pod")).unwrap().phase, Phase::Started);
    e.apply(200, BudgetCommand::Tick).unwrap();
    assert_eq!(
        e.reservation(&id("pod")).unwrap().phase,
        Phase::StopRequired
    );
}

#[cfg(feature = "sqlite")]
mod durable {
    use super::*;
    use std::{
        sync::{Arc, Barrier},
        thread,
    };

    #[test]
    fn one_owner_and_concurrent_callers_share_one_admission_boundary() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("ledger");
        let ledger = Arc::new(SqliteLedger::create(&path, policy(), 0).unwrap());
        assert!(matches!(
            SqliteLedger::open(&path, &policy(), 0),
            Err(BudgetError::OwnerBusy)
        ));
        let barrier = Arc::new(Barrier::new(10));
        let threads: Vec<_> = (0..10)
            .map(|n| {
                let ledger = ledger.clone();
                let barrier = barrier.clone();
                thread::spawn(move || {
                    barrier.wait();
                    ledger
                        .apply(
                            1,
                            BudgetCommand::Reserve {
                                request: request(&format!("r-{n}"), 30),
                            },
                        )
                        .is_ok()
                })
            })
            .collect();
        let admitted = threads
            .into_iter()
            .map(|t| usize::from(t.join().unwrap()))
            .sum::<usize>();
        assert_eq!(admitted, 3);
        assert_eq!(ledger.view().unwrap().totals.exposure_nanos, "90");
        drop(ledger);
        let opened = SqliteLedger::open(&path, &policy(), 2).unwrap();
        assert_eq!(opened.view().unwrap().totals.exposure_nanos, "90");
    }
    #[test]
    fn restart_preserves_uncertain_and_shutdown_obligations_and_refuses_new_policy() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("ledger");
        let ledger = SqliteLedger::create(&path, policy(), 0).unwrap();
        for r in [request("a", 20), compute("pod", 50)] {
            let identity = r.id.clone();
            ledger
                .apply(2, BudgetCommand::Reserve { request: r })
                .unwrap();
            ledger
                .apply(2, BudgetCommand::Begin { id: identity })
                .unwrap();
        }
        drop(ledger);
        let mut changed = policy();
        changed.limit = amount(999);
        assert_eq!(
            SqliteLedger::open(&path, &changed, 3).err(),
            Some(BudgetError::PolicyMismatch)
        );
        let ledger = SqliteLedger::open(&path, &policy(), 3).unwrap();
        let view = ledger.view().unwrap();
        assert_eq!(
            view.reservations
                .iter()
                .map(|r| r.phase)
                .collect::<Vec<_>>(),
            vec![Phase::Uncertain, Phase::StopRequired]
        );
        assert_eq!(view.totals.exposure_nanos, "70");
        assert_eq!(
            ledger
                .apply(
                    4,
                    BudgetCommand::Reserve {
                        request: request("b", 1)
                    }
                )
                .unwrap_err(),
            BudgetError::UncertainSpend
        );
        assert_eq!(
            ledger
                .apply(4, BudgetCommand::Begin { id: id("a") })
                .unwrap_err(),
            BudgetError::WrongPhase
        );
        drop(ledger);
        let again = SqliteLedger::open(&path, &policy(), 5).unwrap();
        assert_eq!(again.view().unwrap().totals.exposure_nanos, "70");
    }
    #[test]
    fn missing_and_corrupted_storage_never_reset_the_ledger() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("ledger");
        assert!(matches!(
            SqliteLedger::open(&path, &policy(), 0),
            Err(BudgetError::Storage)
        ));
        let ledger = SqliteLedger::create(&path, policy(), 0).unwrap();
        drop(ledger);
        assert!(SqliteLedger::create(&path, policy(), 0).is_err());
        let connection = rusqlite::Connection::open(path.join("ledger.sqlite3")).unwrap();
        connection
            .execute("UPDATE journal SET body='{}' WHERE seq=1", [])
            .unwrap();
        drop(connection);
        assert!(matches!(
            SqliteLedger::open(&path, &policy(), 1),
            Err(BudgetError::InvalidJournal)
        ));
    }
}

#[cfg(feature = "sqlite")]
#[test]
fn sqlite_process_fixture() {
    let Ok(mode) = std::env::var("LLM_BUDGET_FIXTURE_MODE") else {
        return;
    };
    let path = std::path::PathBuf::from(std::env::var_os("LLM_BUDGET_FIXTURE_PATH").unwrap());
    if mode == "contend" {
        assert!(matches!(
            SqliteLedger::open(&path, &policy(), 2),
            Err(BudgetError::OwnerBusy)
        ));
    } else {
        assert_eq!(mode, "crash");
        let ledger = SqliteLedger::create(&path, policy(), 0).unwrap();
        for r in [request("a", 20), compute("pod", 50)] {
            let name = r.id.clone();
            ledger
                .apply(1, BudgetCommand::Reserve { request: r })
                .unwrap();
            ledger.apply(1, BudgetCommand::Begin { id: name }).unwrap();
        }
        // Exit without running destructors, then the parent reopens the actual journal.
        std::process::exit(0);
    }
}
#[cfg(feature = "sqlite")]
#[test]
fn process_lock_and_abrupt_exit_are_observed_at_the_real_storage_edge() {
    fn child(mode: &str, path: &std::path::Path) {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "sqlite_process_fixture", "--nocapture"])
            .env("LLM_BUDGET_FIXTURE_MODE", mode)
            .env("LLM_BUDGET_FIXTURE_PATH", path)
            .status()
            .unwrap();
        assert!(status.success(), "child process fixture failed");
    }
    let temp = tempfile::tempdir().unwrap();
    let held = temp.path().join("held");
    let owner = SqliteLedger::create(&held, policy(), 0).unwrap();
    child("contend", &held);
    drop(owner);
    let crashed = temp.path().join("crashed");
    child("crash", &crashed);
    let recovered = SqliteLedger::open(&crashed, &policy(), 2).unwrap();
    let view = recovered.view().unwrap();
    assert_eq!(view.totals.exposure_nanos, "70");
    assert_eq!(view.totals.stop_required, vec![id("pod")]);
    assert_eq!(view.reservations[0].phase, Phase::Uncertain);
    assert_eq!(
        recovered
            .apply(
                2,
                BudgetCommand::Reserve {
                    request: request("b", 1)
                }
            )
            .unwrap_err(),
        BudgetError::UncertainSpend
    );
}
