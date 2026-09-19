use llm_core::Id;
use llm_cost::{
    RecordedCharge,
    budget::{
        BudgetCommand, BudgetError, BudgetPolicy, BudgetReceipt, BudgetView, Operation,
        ReservationRequest, SqliteLedger,
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Barrier},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exercise {
    program_json: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Program {
    policy: BudgetPolicy,
    steps: Vec<Step>,
}
#[derive(Deserialize)]
#[serde(tag = "step", rename_all = "kebab-case", deny_unknown_fields)]
enum Step {
    Apply {
        at_ms: u64,
        command: BudgetCommand,
    },
    Reopen {
        at_ms: u64,
        policy: Option<BudgetPolicy>,
    },
    Contend {
        at_ms: u64,
    },
    Concurrent {
        at_ms: u64,
        requests: Vec<ReservationRequest>,
    },
    Fault {
        fault: Fault,
    },
}
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Fault {
    Commit,
    ClearCommit,
    Version,
    Entry,
    Gap,
    Decision,
}

pub fn observe(input: &Exercise) -> Result<Value, String> {
    let mut facts = json!({"valid_program": false, "storage_failed": false, "error_code": null, "results": [], "permits": [],
        "concurrent_admitted": [], "concurrent_errors": [], "policy_id": null, "currency": null,
        "limit": null, "max_active": null, "now_ms": null, "totals": null,
        "reservations": [], "charge_ids": [], "attribution_preserved": false});
    if input.program_json.len() > llm_cost::MAX_DOCUMENT_BYTES {
        return Ok(facts);
    }
    let Ok(program) = serde_json::from_str::<Program>(&input.program_json) else {
        return Ok(facts);
    };
    if program.steps.len() > 128
        || program
            .steps
            .iter()
            .any(|s| matches!(s, Step::Concurrent { requests, .. } if requests.len() > 32))
    {
        return Ok(facts);
    }
    facts["valid_program"] = json!(true);
    let temp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let path = temp.path().join("ledger");
    let ledger = match SqliteLedger::create(&path, program.policy.clone(), 0) {
        Ok(ledger) => ledger,
        Err(e) => {
            facts["error_code"] = json!(e);
            return Ok(facts);
        }
    };
    let mut run = Run {
        ledger: Some(Arc::new(ledger)),
        snapshot: None,
        operations: BTreeMap::new(),
        charges: BTreeMap::new(),
        results: Vec::new(),
        permits: Vec::new(),
        admitted: Vec::new(),
        errors: Vec::new(),
    };
    for step in program.steps {
        run.step(step, &path, &program.policy)?;
    }
    if let Some(ledger) = &run.ledger {
        run.snapshot = Some(ledger.view().map_err(|e| e.to_string())?);
    }
    facts["results"] = json!(run.results);
    facts["permits"] = json!(run.permits);
    facts["concurrent_admitted"] = json!(run.admitted);
    facts["concurrent_errors"] = json!(run.errors);
    if let Some(view) = &run.snapshot {
        snapshot(&mut facts, view, &run);
    }
    Ok(facts)
}
struct Run {
    ledger: Option<Arc<SqliteLedger>>,
    snapshot: Option<BudgetView>,
    operations: BTreeMap<Id, Operation>,
    charges: BTreeMap<Id, RecordedCharge>,
    results: Vec<String>,
    permits: Vec<Id>,
    admitted: Vec<usize>,
    errors: Vec<String>,
}
impl Run {
    fn step(&mut self, step: Step, path: &Path, policy: &BudgetPolicy) -> Result<(), String> {
        match step {
            Step::Apply { at_ms, command } => {
                let result = self
                    .ledger
                    .as_ref()
                    .ok_or(BudgetError::Storage)
                    .and_then(|l| l.apply(at_ms, command.clone()));
                if result.is_ok() {
                    match command {
                        BudgetCommand::Reserve { request } => {
                            self.operations.insert(request.id, request.operation);
                        }
                        BudgetCommand::PostCharge { charge } => {
                            self.charges.insert(charge.id.clone(), charge);
                        }
                        _ => (),
                    }
                }
                self.record(result);
            }
            Step::Reopen {
                at_ms,
                policy: override_policy,
            } => {
                if let Some(ledger) = self.ledger.take() {
                    self.snapshot = Some(ledger.view().map_err(|e| e.to_string())?);
                    drop(ledger);
                }
                match SqliteLedger::open(path, override_policy.as_ref().unwrap_or(policy), at_ms) {
                    Ok(ledger) => {
                        self.ledger = Some(Arc::new(ledger));
                        self.results.push("ok".into());
                    }
                    Err(e) => self.results.push(error(e)),
                }
            }
            Step::Contend { at_ms } => match SqliteLedger::open(path, policy, at_ms) {
                Ok(_) => self.results.push("ok".into()),
                Err(e) => self.results.push(error(e)),
            },
            Step::Concurrent { at_ms, requests } => self.concurrent(at_ms, requests)?,
            Step::Fault { fault } => {
                fault.apply(path)?;
                self.results.push("fixture-ready".into());
            }
        }
        Ok(())
    }
    fn record(&mut self, result: Result<BudgetReceipt, BudgetError>) {
        match result {
            Ok(receipt) => {
                self.results.push("ok".into());
                if let Some(permit) = receipt.permit {
                    self.permits.push(permit.reservation().clone());
                }
            }
            Err(e) => self.results.push(error(e)),
        }
    }
    fn concurrent(&mut self, at_ms: u64, requests: Vec<ReservationRequest>) -> Result<(), String> {
        let ledger = self.ledger.as_ref().ok_or("fixture has no open ledger")?;
        let barrier = Arc::new(Barrier::new(requests.len().max(1)));
        let outcomes = std::thread::scope(|scope| {
            let threads: Vec<_> = requests
                .into_iter()
                .map(|request| {
                    let barrier = barrier.clone();
                    scope.spawn(move || {
                        barrier.wait();
                        let result = ledger.apply(
                            at_ms,
                            BudgetCommand::Reserve {
                                request: request.clone(),
                            },
                        );
                        (request, result)
                    })
                })
                .collect();
            threads
                .into_iter()
                .map(|t| t.join().map_err(|_| "concurrent fixture panicked"))
                .collect::<Result<Vec<_>, _>>()
        })?;
        let mut admitted = 0;
        let mut errors = Vec::new();
        for (request, result) in outcomes {
            match result {
                Ok(_) => {
                    admitted += 1;
                    self.operations.insert(request.id, request.operation);
                }
                Err(e) => errors.push(error(e)),
            }
        }
        errors.sort();
        self.errors.extend(errors);
        self.admitted.push(admitted);
        self.results.push("concurrent-observed".into());
        Ok(())
    }
}
fn error(error: BudgetError) -> String {
    format!(
        "err:{}",
        serde_json::to_value(error)
            .expect("closed error enum")
            .as_str()
            .expect("string enum")
    )
}
fn snapshot(facts: &mut Value, view: &BudgetView, run: &Run) {
    facts["storage_failed"] = json!(view.storage_failed);
    facts["policy_id"] = json!(view.policy.id);
    facts["currency"] = json!(view.policy.currency);
    facts["limit"] = json!(view.policy.limit);
    facts["max_active"] = json!(view.policy.max_active);
    facts["now_ms"] = json!(view.now_ms.to_string());
    facts["totals"] = json!(view.totals);
    facts["reservations"] = json!(view.reservations.iter().map(|r| json!({
        "id": r.request.id, "phase": r.phase, "reserved": r.request.reserved,
        "assumption": r.request.assumption, "expires_at_ms": r.request.expires_at_ms.to_string(),
        "known": r.settlement.as_ref().map(|s| s.known), "complete": r.settlement.as_ref().map(|s| s.complete),
        "evidence": r.evidence, "settlement_evidence": r.settlement.as_ref().map(|s| &s.evidence),
    })).collect::<Vec<_>>());
    facts["charge_ids"] = json!(
        view.charges
            .iter()
            .map(|c| &c.observation.id)
            .collect::<Vec<_>>()
    );
    let operations_match = view
        .reservations
        .iter()
        .all(|r| run.operations.get(&r.request.id) == Some(&r.request.operation));
    let charges_match = view.charges.iter().all(|c| {
        run.charges.get(&c.observation.id).is_some_and(|original| {
            let mut attributed = c.observation.clone();
            attributed.amount = original.amount;
            &attributed == original
        })
    });
    facts["attribution_preserved"] = json!(operations_match && charges_match);
}
impl Fault {
    fn apply(self, path: &Path) -> Result<(), String> {
        let connection =
            rusqlite::Connection::open(path.join("ledger.sqlite3")).map_err(|e| e.to_string())?;
        let sql = match self {
            Self::Commit => "CREATE TABLE fault_parent(id INTEGER PRIMARY KEY);
                CREATE TABLE fault_child(id INTEGER REFERENCES fault_parent(id) DEFERRABLE INITIALLY DEFERRED);
                CREATE TRIGGER fault_commit AFTER INSERT ON journal BEGIN INSERT INTO fault_child VALUES(1); END;",
            Self::ClearCommit => "DROP TRIGGER fault_commit;",
            Self::Version => "UPDATE metadata SET format='llm.budget/999' WHERE id=1;",
            Self::Entry => "UPDATE journal SET body='{}' WHERE seq=1;",
            Self::Gap => "DELETE FROM journal WHERE seq=1;",
            Self::Decision => "UPDATE journal SET body=json_set(body, '$.result', json('{\"Err\":\"limit-exceeded\"}')) WHERE seq=1;",
        };
        connection.execute_batch(sql).map_err(|e| e.to_string())
    }
}
