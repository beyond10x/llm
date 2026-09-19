use super::{BudgetCommand, BudgetEngine, BudgetError, BudgetPolicy, BudgetReceipt, BudgetView};
use crate::MAX_DOCUMENT_BYTES;
use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    path::Path,
    sync::Mutex,
    time::Duration,
};

const FORMAT: &str = "llm.budget/1";
const APPLICATION_ID: i32 = 0x4c4c_4d42;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    format: String,
    at_ms: u64,
    command: Action,
    result: Result<(), BudgetError>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
enum Action {
    Apply(Box<BudgetCommand>),
    Recover,
}

struct Inner {
    connection: Connection,
    engine: BudgetEngine,
    sequence: i64,
    failed: bool,
}
struct OwnerLock(File);
impl Drop for OwnerLock {
    fn drop(&mut self) {
        // A concurrent process spawn can briefly inherit this open-file
        // description before exec closes it. Close alone leaves that lock alive.
        // On unlock failure, closing still releases our handle; any inherited
        // handle conservatively keeps competing owners out until it also closes.
        let _ = self.0.unlock();
    }
}
/// A single process owner, shareable by reference/Arc across concurrent callers.
/// Local trusted storage is required. Neither filesystem permissions nor a lock
/// protect against a privileged administrator replacing files or restoring backups.
pub struct SqliteLedger {
    inner: Mutex<Inner>,
    // Keep the lock until after the connection/engine are dropped. Never unlink it.
    _owner: OwnerLock,
}
impl SqliteLedger {
    /// Creates a new directory and ledger. Never opens or overwrites an existing directory.
    /// # Errors
    /// Refuses invalid policy, an existing path or any storage/commit failure.
    pub fn create(
        directory: &Path,
        policy: BudgetPolicy,
        now_ms: u64,
    ) -> Result<Self, BudgetError> {
        let engine = BudgetEngine::new(policy)?;
        #[cfg(unix)]
        let builder = {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            builder
        };
        #[cfg(not(unix))]
        let builder = fs::DirBuilder::new();
        builder
            .create(directory)
            .map_err(|_| BudgetError::Storage)?;
        let owner = new_file(&directory.join("owner.lock"))?;
        lock_owner(&owner)?;
        let owner = OwnerLock(owner);
        let database = directory.join("ledger.sqlite3");
        let created = new_file(&database)?;
        created.sync_all().map_err(|_| BudgetError::Storage)?;
        drop(created);
        let mut connection = connection(&database)?;
        configure(&connection)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        transaction.execute_batch("CREATE TABLE metadata (id INTEGER PRIMARY KEY CHECK(id=1), format TEXT NOT NULL, policy TEXT NOT NULL) STRICT;
            CREATE TABLE journal (seq INTEGER PRIMARY KEY, body TEXT NOT NULL) STRICT;
            PRAGMA application_id=1280068930; PRAGMA user_version=1;") .map_err(storage)?;
        transaction
            .execute(
                "INSERT INTO metadata VALUES (1, ?1, ?2)",
                params![
                    FORMAT,
                    serde_json::to_string(engine.policy()).map_err(storage)?
                ],
            )
            .map_err(storage)?;
        transaction.commit().map_err(storage)?;
        sync_directory(directory)?;
        if let Some(parent) = directory.parent() {
            sync_directory(parent)?;
        }
        let ledger = Self {
            inner: Mutex::new(Inner {
                connection,
                engine,
                sequence: 0,
                failed: false,
            }),
            _owner: owner,
        };
        ledger.apply(now_ms, BudgetCommand::Tick)?;
        Ok(ledger)
    }

    /// Opens only an existing journal, verifies its immutable policy and every replayed
    /// decision, then durably marks interrupted dispatches uncertain/stop-required.
    /// # Errors
    /// Refuses another owner, policy mismatch, malformed or unsupported data, time reversal,
    /// missing files or persistence failure. No reset/recreation or historical permit is returned.
    pub fn open(directory: &Path, policy: &BudgetPolicy, now_ms: u64) -> Result<Self, BudgetError> {
        policy.validate()?;
        if !fs::symlink_metadata(directory)
            .map_err(storage)?
            .file_type()
            .is_dir()
        {
            return Err(BudgetError::Storage);
        }
        let lock_path = directory.join("owner.lock");
        regular_file(&lock_path)?;
        let owner = OpenOptions::new()
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(storage)?;
        lock_owner(&owner)?;
        let owner = OwnerLock(owner);
        let database = directory.join("ledger.sqlite3");
        regular_file(&database)?;
        let connection = connection(&database)?;
        let application: i32 = connection
            .pragma_query_value(None, "application_id", |r| r.get(0))
            .map_err(storage)?;
        let version: i32 = connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(storage)?;
        if application != APPLICATION_ID || version != 1 {
            return Err(BudgetError::InvalidJournal);
        }
        let (format, bytes): (String, String) = connection
            .query_row("SELECT format, CASE WHEN length(CAST(policy AS BLOB)) <= 1048576 THEN policy ELSE NULL END FROM metadata WHERE id=1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(storage)?;
        if format != FORMAT || bytes.len() > MAX_DOCUMENT_BYTES {
            return Err(BudgetError::InvalidJournal);
        }
        let recorded: BudgetPolicy =
            serde_json::from_str(&bytes).map_err(|_| BudgetError::InvalidJournal)?;
        if &recorded != policy {
            return Err(BudgetError::PolicyMismatch);
        }
        let mut engine = BudgetEngine::new(recorded)?;
        let mut sequence = 0_i64;
        {
            let mut statement = connection
                .prepare("SELECT seq, CASE WHEN length(CAST(body AS BLOB)) <= 1048576 THEN body ELSE NULL END FROM journal ORDER BY seq")
                .map_err(storage)?;
            let mut rows = statement.query([]).map_err(storage)?;
            while let Some(row) = rows.next().map_err(storage)? {
                let seq: i64 = row.get(0).map_err(storage)?;
                let bytes: String = row.get(1).map_err(storage)?;
                if Some(seq) != sequence.checked_add(1) || bytes.len() > MAX_DOCUMENT_BYTES {
                    return Err(BudgetError::InvalidJournal);
                }
                let entry: Entry =
                    serde_json::from_str(&bytes).map_err(|_| BudgetError::InvalidJournal)?;
                if entry.format != FORMAT {
                    return Err(BudgetError::InvalidJournal);
                }
                let result = execute(&mut engine, entry.at_ms, entry.command).map(|_| ());
                if result != entry.result {
                    return Err(BudgetError::InvalidJournal);
                }
                sequence = seq;
            }
        }
        if sequence == 0 {
            return Err(BudgetError::InvalidJournal);
        }
        configure(&connection)?;
        let ledger = Self {
            inner: Mutex::new(Inner {
                connection,
                engine,
                sequence,
                failed: false,
            }),
            _owner: owner,
        };
        ledger.commit(now_ms, Action::Recover)?;
        Ok(ledger)
    }

    /// # Errors
    /// Refuses invalid admission/transition or storage failure. Even a refused command can
    /// durably observe elapsed obligations; a failed commit leaves no new dispatch permit.
    pub fn apply(&self, now_ms: u64, command: BudgetCommand) -> Result<BudgetReceipt, BudgetError> {
        self.commit(now_ms, Action::Apply(Box::new(command)))
    }
    fn commit(&self, now_ms: u64, action: Action) -> Result<BudgetReceipt, BudgetError> {
        if llm_core::exceeds(&action, MAX_DOCUMENT_BYTES / 2) {
            return Err(BudgetError::TooLarge);
        }
        let mut inner = self.inner.lock().map_err(|_| BudgetError::Storage)?;
        if inner.failed {
            return Err(BudgetError::Storage);
        }
        let mut candidate = inner.engine.clone();
        // Retain the command for its exact durable input without requiring permits to be Clone.
        let encoded = serde_json::to_string(&action).map_err(storage)?;
        if encoded.len() > MAX_DOCUMENT_BYTES / 2 {
            return Err(BudgetError::TooLarge);
        }
        let replay: Action = serde_json::from_str(&encoded).map_err(storage)?;
        let result = execute(&mut candidate, now_ms, replay);
        let entry = Entry {
            format: FORMAT.into(),
            at_ms: now_ms,
            command: action,
            result: result.as_ref().map(|_| ()).map_err(|e| *e),
        };
        let bytes = serde_json::to_string(&entry).map_err(storage)?;
        let next = inner
            .sequence
            .checked_add(1)
            .ok_or(BudgetError::Arithmetic)?;
        if let Err(error) = append(&mut inner.connection, next, &bytes) {
            inner.failed = true;
            inner.engine.recover();
            return Err(error);
        }
        inner.engine = candidate;
        inner.sequence = next;
        result
    }
    /// # Errors
    /// Refuses a poisoned owner or aggregate arithmetic overflow.
    pub fn view(&self) -> Result<BudgetView, BudgetError> {
        let inner = self.inner.lock().map_err(|_| BudgetError::Storage)?;
        let mut view = inner.engine.view()?;
        view.storage_failed = inner.failed;
        Ok(view)
    }
}
fn append(connection: &mut Connection, sequence: i64, body: &str) -> Result<(), BudgetError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(storage)?;
    transaction
        .execute(
            "INSERT INTO journal(seq, body) VALUES (?1, ?2)",
            params![sequence, body],
        )
        .map_err(storage)?;
    transaction.commit().map_err(storage)
}
fn execute(
    engine: &mut BudgetEngine,
    at_ms: u64,
    action: Action,
) -> Result<BudgetReceipt, BudgetError> {
    match action {
        Action::Apply(command) => engine.apply(at_ms, *command),
        Action::Recover => {
            engine.apply(at_ms, BudgetCommand::Tick)?;
            engine.recover();
            Ok(BudgetReceipt { permit: None })
        }
    }
}
fn connection(path: &Path) -> Result<Connection, BudgetError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(storage)?;
    connection
        .busy_timeout(Duration::from_secs(2))
        .map_err(storage)?;
    Ok(connection)
}
fn configure(connection: &Connection) -> Result<(), BudgetError> {
    let mode: String = connection
        .query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))
        .map_err(storage)?;
    connection
        .execute_batch("PRAGMA synchronous=EXTRA; PRAGMA trusted_schema=OFF; PRAGMA fullfsync=ON; PRAGMA foreign_keys=ON;")
        .map_err(storage)?;
    let sync: i32 = connection
        .pragma_query_value(None, "synchronous", |r| r.get(0))
        .map_err(storage)?;
    if mode != "delete" || sync != 3 {
        return Err(BudgetError::Storage);
    }
    Ok(())
}
fn new_file(path: &Path) -> Result<File, BudgetError> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map_err(storage)
}
fn regular_file(path: &Path) -> Result<(), BudgetError> {
    if !fs::symlink_metadata(path)
        .map_err(storage)?
        .file_type()
        .is_file()
    {
        return Err(BudgetError::Storage);
    }
    Ok(())
}
fn sync_directory(path: &Path) -> Result<(), BudgetError> {
    #[cfg(unix)]
    {
        File::open(path)
            .and_then(|f| f.sync_all())
            .map_err(storage)?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}
fn storage(_: impl std::fmt::Display) -> BudgetError {
    BudgetError::Storage
}
fn lock_owner(file: &File) -> Result<(), BudgetError> {
    file.try_lock().map_err(|error| match error {
        fs::TryLockError::WouldBlock => BudgetError::OwnerBusy,
        fs::TryLockError::Error(_) => BudgetError::Storage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Amount, Currency,
        budget::{Operation, Phase, ReservationRequest},
    };
    use llm_core::Id;

    #[cfg(unix)]
    #[test]
    fn closing_owner_releases_lock_even_with_an_inherited_descriptor() {
        let policy = BudgetPolicy {
            id: Id::new("scope").unwrap(),
            currency: Currency::new("USD").unwrap(),
            limit: Amount::from_nanos(100),
            max_active: 2,
        };
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("ledger");
        let ledger = SqliteLedger::create(&path, policy.clone(), 0).unwrap();
        // dup retains the same open-file description as a forked child between
        // fork and exec. Closing only the parent's descriptor does not unlock it.
        #[expect(
            clippy::used_underscore_binding,
            reason = "this regression deliberately duplicates the otherwise RAII-only guard"
        )]
        let inherited = ledger._owner.0.try_clone().unwrap();
        drop(ledger);
        assert_eq!(SqliteLedger::open(&path, &policy, 1).err(), None);
        drop(inherited);
    }

    #[test]
    fn failed_sqlite_commit_issues_no_permit_or_in_memory_start() {
        let id = |v| Id::new(v).unwrap();
        let policy = BudgetPolicy {
            id: id("scope"),
            currency: Currency::new("USD").unwrap(),
            limit: Amount::from_nanos(100),
            max_active: 2,
        };
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("ledger");
        let ledger = SqliteLedger::create(&path, policy.clone(), 0).unwrap();
        ledger
            .apply(
                0,
                BudgetCommand::Reserve {
                    request: ReservationRequest {
                        id: id("pod"),
                        operation: Operation::Compute {
                            deployment: id("pod"),
                            provider: id("host"),
                            account: id("owner"),
                        },
                        reserved: Amount::from_nanos(50),
                        assumption: id("rates"),
                        expires_at_ms: 100,
                    },
                },
            )
            .unwrap();
        // A deferred constraint fails COMMIT after INSERT succeeded. The real SQLite
        // error exercises the commit boundary, without a fake successful store.
        ledger.inner.lock().unwrap().connection.execute_batch(
            "PRAGMA foreign_keys=ON;
             CREATE TEMP TABLE parent(id INTEGER PRIMARY KEY);
             CREATE TEMP TABLE child(id INTEGER REFERENCES parent(id) DEFERRABLE INITIALLY DEFERRED);
             CREATE TEMP TRIGGER fail_commit AFTER INSERT ON main.journal BEGIN INSERT INTO child VALUES(1); END;"
        ).unwrap();
        assert_eq!(
            ledger
                .apply(1, BudgetCommand::Begin { id: id("pod") })
                .unwrap_err(),
            BudgetError::Storage
        );
        assert_eq!(
            ledger.view().unwrap().reservations[0].phase,
            Phase::Reserved
        );
        assert!(ledger.view().unwrap().storage_failed);
        ledger
            .inner
            .lock()
            .unwrap()
            .connection
            .execute_batch("DROP TRIGGER fail_commit;")
            .unwrap();
        assert_eq!(
            ledger
                .apply(1, BudgetCommand::Begin { id: id("pod") })
                .unwrap_err(),
            BudgetError::Storage
        );
        drop(ledger);
        let ledger = SqliteLedger::open(&path, &policy, 1).unwrap();
        assert!(
            ledger
                .apply(1, BudgetCommand::Begin { id: id("pod") })
                .unwrap()
                .permit
                .is_some()
        );
        assert_eq!(
            ledger
                .apply(1, BudgetCommand::Begin { id: id("pod") })
                .unwrap_err(),
            BudgetError::WrongPhase
        );
    }
}
