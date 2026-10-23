//! `ProcessCredentials` against a command that is this test binary itself.
//!
//! The target has no libtest harness: libtest writes `running N tests` to stdout before any test
//! runs, and a `credential_process` command's stdout must be exactly its answer. Run with
//! `--credential-process-fixture <directory>`, the binary is the fixture command described at
//! [`fixture`]; run any other way, it runs every case below, or those whose name contains an
//! argument, and prints the summary libtest would.

use llm_bedrock::{CredentialSource, ProcessCredentials};
use llm_core::{Cancel, Dispatch, Error, ErrorCode};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const FIXTURE_FLAG: &str = "--credential-process-fixture";
const ACCESS_KEY_ID: &str = "ASIAPROCESSFIXTURE";
const RENEWED_KEY_ID: &str = "ASIAPROCESSRENEWED";
const SECRET_ACCESS_KEY: &str = "process/fixture+secret-key";
const SESSION_TOKEN: &str = "process-fixture-session-token";
const PRINTED: &str = "fixture-stdout-that-must-stay-private";

/// Every secret value a case uses: none may reach a message or a `Debug` output.
const SECRETS: [&str; 5] = [
    ACCESS_KEY_ID,
    RENEWED_KEY_ID,
    SECRET_ACCESS_KEY,
    SESSION_TOKEN,
    PRINTED,
];

type Case = (&'static str, fn());

const CASES: &[Case] = &[
    (
        "an_answer_without_expiration_is_run_once_and_kept_for_the_life_of_the_source",
        an_answer_without_expiration_is_run_once_and_kept_for_the_life_of_the_source,
    ),
    (
        "an_answer_outside_the_refresh_window_is_not_run_again",
        an_answer_outside_the_refresh_window_is_not_run_again,
    ),
    (
        "an_answer_inside_the_refresh_window_is_answered_at_once_and_refreshed_by_one_run",
        an_answer_inside_the_refresh_window_is_answered_at_once_and_refreshed_by_one_run,
    ),
    (
        "turns_inside_the_refresh_window_get_the_kept_answer_while_one_refresh_stalls",
        turns_inside_the_refresh_window_get_the_kept_answer_while_one_refresh_stalls,
    ),
    (
        "an_expired_answer_makes_every_caller_wait_for_the_one_run",
        an_expired_answer_makes_every_caller_wait_for_the_one_run,
    ),
    (
        "a_failed_refresh_answers_the_kept_credentials_until_they_expire",
        a_failed_refresh_answers_the_kept_credentials_until_they_expire,
    ),
    (
        "each_malformed_answer_is_unauthorized_names_the_program_and_carries_no_output",
        each_malformed_answer_is_unauthorized_names_the_program_and_carries_no_output,
    ),
    (
        "an_answer_over_64_kib_is_unauthorized",
        an_answer_over_64_kib_is_unauthorized,
    ),
    (
        "a_non_zero_exit_is_unauthorized_and_carries_no_output",
        a_non_zero_exit_is_unauthorized_and_carries_no_output,
    ),
    (
        "a_command_that_does_not_exit_within_the_limit_is_killed_and_unauthorized",
        a_command_that_does_not_exit_within_the_limit_is_killed_and_unauthorized,
    ),
    (
        "dropping_the_source_kills_the_command_it_is_running",
        dropping_the_source_kills_the_command_it_is_running,
    ),
    (
        "a_program_that_cannot_be_started_is_unauthorized_by_its_name",
        a_program_that_cannot_be_started_is_unauthorized_by_its_name,
    ),
    (
        "an_empty_program_is_refused_before_anything_runs",
        an_empty_program_is_refused_before_anything_runs,
    ),
    ("the_command_reads_no_input", the_command_reads_no_input),
    (
        "an_expiration_with_any_offset_is_read_as_its_instant",
        an_expiration_with_any_offset_is_read_as_its_instant,
    ),
    (
        "a_cancelled_caller_runs_nothing",
        a_cancelled_caller_runs_nothing,
    ),
    ("debug_output_shows_no_key", debug_output_shows_no_key),
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some(FIXTURE_FLAG) {
        return fixture(Path::new(&args[2]));
    }
    let filters: Vec<&str> = args[1..]
        .iter()
        .map(String::as_str)
        .filter(|arg| !arg.starts_with('-'))
        .collect();
    let chosen: Vec<&Case> = CASES
        .iter()
        .filter(|(name, _)| filters.is_empty() || filters.iter().any(|f| name.contains(f)))
        .collect();
    println!("\nrunning {} tests", chosen.len());
    let mut failed = Vec::new();
    for (name, case) in chosen.iter().copied() {
        let outcome = std::panic::catch_unwind(case);
        println!(
            "test {name} ... {}",
            if outcome.is_ok() { "ok" } else { "FAILED" }
        );
        if outcome.is_err() {
            failed.push(*name);
        }
    }
    let passed = chosen.len() - failed.len();
    println!(
        "\ntest result: {}. {passed} passed; {} failed; 0 ignored; 0 measured; {} filtered out\n",
        if failed.is_empty() { "ok" } else { "FAILED" },
        failed.len(),
        CASES.len() - chosen.len()
    );
    if failed.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The fixture command. In `directory` it appends one line to `runs`, then by the file `mode`:
/// `exit` prints [`PRINTED`] and exits 3; `sleep` sleeps two seconds and then writes `survived`;
/// `flood` prints 70 KiB; `stdin` answers only once its input is at an end. Otherwise it prints
/// `answer-<n>`, `n` the runs before it, or `answer` when there is none; a run after the first
/// first waits, at most ten seconds, while the file `gate` exists.
fn fixture(directory: &Path) -> ExitCode {
    use std::io::{Read, Write};
    let runs = directory.join("runs");
    let before = std::fs::read_to_string(&runs).map_or(0, |text| text.lines().count());
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&runs)
        .expect("the run log");
    log.write_all(b"run\n").expect("a run logged");
    drop(log);
    let mode = std::fs::read_to_string(directory.join("mode")).unwrap_or_default();
    let mut stdout = std::io::stdout().lock();
    match mode.trim() {
        "exit" => {
            let _ = stdout.write_all(PRINTED.as_bytes());
            let _ = stdout.flush();
            return ExitCode::from(3);
        }
        "sleep" => {
            std::thread::sleep(Duration::from_secs(2));
            std::fs::write(directory.join("survived"), b"").expect("a trace");
            return ExitCode::SUCCESS;
        }
        "flood" => {
            let _ = stdout.write_all(&vec![b' '; 70 * 1024]);
            return ExitCode::SUCCESS;
        }
        "stdin" => {
            let mut input = Vec::new();
            if std::io::stdin().read_to_end(&mut input).is_err() || !input.is_empty() {
                return ExitCode::from(4);
            }
        }
        _ => {}
    }
    if before > 0 {
        let gate = directory.join("gate");
        let until = std::time::Instant::now() + Duration::from_secs(10);
        while gate.exists() && std::time::Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    let numbered = directory.join(format!("answer-{before}"));
    let answer = std::fs::read(&numbered)
        .or_else(|_| std::fs::read(directory.join("answer")))
        .unwrap_or_default();
    let _ = stdout.write_all(&answer);
    let _ = stdout.flush();
    ExitCode::SUCCESS
}

/// One fixture command's directory.
struct Command {
    directory: tempfile::TempDir,
}

impl Command {
    fn new() -> Self {
        Self {
            directory: tempfile::tempdir().expect("a directory"),
        }
    }

    fn answering(answer: &Value) -> Self {
        let command = Self::new();
        command.answer(None, answer.to_string().as_bytes());
        command
    }

    fn path(&self, name: &str) -> PathBuf {
        self.directory.path().join(name)
    }

    /// Sets the answer of run `n`, or of every run without its own.
    fn answer(&self, run: Option<usize>, bytes: &[u8]) {
        let name = run.map_or_else(|| "answer".to_owned(), |run| format!("answer-{run}"));
        std::fs::write(self.path(&name), bytes).expect("an answer");
    }

    fn mode(&self, mode: &str) {
        std::fs::write(self.path("mode"), mode).expect("a mode");
    }

    fn close_gate(&self) {
        std::fs::write(self.path("gate"), b"").expect("a gate");
    }

    fn open_gate(&self) {
        std::fs::remove_file(self.path("gate")).expect("the gate removed");
    }

    fn runs(&self) -> usize {
        std::fs::read_to_string(self.path("runs")).map_or(0, |text| text.lines().count())
    }

    fn source(&self) -> ProcessCredentials {
        self.limited(Duration::from_secs(60))
    }

    fn limited(&self, limit: Duration) -> ProcessCredentials {
        ProcessCredentials::with_time_limit(
            program(),
            [
                FIXTURE_FLAG.to_owned(),
                self.directory.path().display().to_string(),
            ],
            limit,
        )
        .expect("a command")
    }

    /// Waits, at most five seconds, until the command has started `count` runs.
    async fn started(&self, count: usize) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while self.runs() < count {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("{count} runs started, {} seen", self.runs()));
    }
}

fn program() -> String {
    std::env::current_exe()
        .expect("this test binary")
        .display()
        .to_string()
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime")
}

fn answer(key: &str, expires: Option<SystemTime>) -> Value {
    let mut answer = json!({
        "Version": 1,
        "AccessKeyId": key,
        "SecretAccessKey": SECRET_ACCESS_KEY,
        "SessionToken": SESSION_TOKEN,
    });
    if let Some(expires) = expires {
        answer["Expiration"] = json!(rfc3339(expires));
    }
    answer
}

/// Waits, at most five seconds, until `source` has no refresh in flight.
async fn settled(source: &ProcessCredentials) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while source.refreshing() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the refresh settles");
}

/// A whole second `seconds` from now (before now when negative).
fn in_seconds(seconds: i64) -> SystemTime {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("a clock after 1970")
        .as_secs();
    let now = UNIX_EPOCH + Duration::from_secs(now);
    let offset = Duration::from_secs(seconds.unsigned_abs());
    if seconds >= 0 {
        now + offset
    } else {
        now - offset
    }
}

/// `at` as an RFC 3339 instant with the offset `zone` (`Z` or `+hh:mm`/`-hh:mm`), whole seconds.
fn rfc3339_in(at: SystemTime, zone: &str) -> String {
    let shift: i64 = match zone.as_bytes() {
        [sign @ (b'+' | b'-'), ..] => {
            let hours: i64 = zone[1..3].parse().expect("hours");
            let minutes: i64 = zone[4..6].parse().expect("minutes");
            let magnitude = hours * 3600 + minutes * 60;
            if *sign == b'+' { magnitude } else { -magnitude }
        }
        _ => 0,
    };
    let seconds = i64::try_from(at.duration_since(UNIX_EPOCH).expect("after 1970").as_secs())
        .expect("seconds")
        + shift;
    let days = seconds.div_euclid(86_400);
    let of_day = seconds.rem_euclid(86_400);
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * from_march + 2) / 5 + 1;
    let month = if from_march < 10 {
        from_march + 3
    } else {
        from_march - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}{zone}",
        hour = of_day / 3600,
        minute = of_day % 3600 / 60,
        second = of_day % 60
    )
}

fn rfc3339(at: SystemTime) -> String {
    rfc3339_in(at, "Z")
}

/// An `unauthorized`, final, not-sent failure that names the program and carries no secret and
/// none of the command's output.
fn assert_refused(error: &Error) {
    assert_eq!(error.code, ErrorCode::Unauthorized, "{error:?}");
    assert!(!error.retriable, "{error:?}");
    assert_eq!(error.dispatch, Dispatch::NotSent, "{error:?}");
    assert!(
        error.message.contains(&format!("`{}`", program())),
        "{}",
        error.message
    );
    for secret in SECRETS {
        assert!(!error.message.contains(secret), "{}", error.message);
    }
}

fn an_answer_without_expiration_is_run_once_and_kept_for_the_life_of_the_source() {
    runtime().block_on(async {
        let mut long_term = answer(ACCESS_KEY_ID, None);
        long_term
            .as_object_mut()
            .expect("an object")
            .remove("SessionToken");
        let command = Command::answering(&long_term);
        let source = command.source();
        let cancel = Cancel::new();
        let first = source.credentials(&cancel).await.expect("first");
        assert_eq!(first.access_key_id(), ACCESS_KEY_ID);
        assert_eq!(first.secret_access_key(), SECRET_ACCESS_KEY);
        assert_eq!(first.session_token(), None);
        assert_eq!(first.expires(), None);
        for _ in 0..3 {
            let again = source.credentials(&cancel).await.expect("kept");
            assert_eq!(again.access_key_id(), ACCESS_KEY_ID);
        }
        assert!(!source.refreshing());
        assert_eq!(command.runs(), 1);
    });
}

fn an_answer_outside_the_refresh_window_is_not_run_again() {
    runtime().block_on(async {
        for seconds in [3600, 360] {
            let expires = in_seconds(seconds);
            let command = Command::answering(&answer(ACCESS_KEY_ID, Some(expires)));
            let source = command.source();
            let cancel = Cancel::new();
            let first = source.credentials(&cancel).await.expect("first");
            assert_eq!(first.session_token(), Some(SESSION_TOKEN));
            assert_eq!(first.expires(), Some(expires));
            source.credentials(&cancel).await.expect("second");
            assert!(!source.refreshing());
            assert_eq!(command.runs(), 1, "expiring in {seconds} s");
        }
    });
}

fn an_answer_inside_the_refresh_window_is_answered_at_once_and_refreshed_by_one_run() {
    runtime().block_on(async {
        let command = Command::new();
        let first_expires = in_seconds(120);
        command.answer(
            Some(0),
            answer(ACCESS_KEY_ID, Some(first_expires))
                .to_string()
                .as_bytes(),
        );
        command.answer(
            None,
            answer(RENEWED_KEY_ID, Some(in_seconds(3600)))
                .to_string()
                .as_bytes(),
        );
        let source = command.source();
        let cancel = Cancel::new();
        source.credentials(&cancel).await.expect("first");
        let kept = source.credentials(&cancel).await.expect("kept at once");
        assert_eq!(kept.access_key_id(), ACCESS_KEY_ID);
        assert_eq!(kept.expires(), Some(first_expires));
        settled(&source).await;
        assert_eq!(command.runs(), 2);
        let renewed = source.credentials(&cancel).await.expect("renewed");
        assert_eq!(renewed.access_key_id(), RENEWED_KEY_ID);
        source.credentials(&cancel).await.expect("renewed, kept");
        assert_eq!(command.runs(), 2);
    });
}

fn turns_inside_the_refresh_window_get_the_kept_answer_while_one_refresh_stalls() {
    runtime().block_on(async {
        let command = Command::new();
        command.answer(
            Some(0),
            answer(ACCESS_KEY_ID, Some(in_seconds(120)))
                .to_string()
                .as_bytes(),
        );
        command.answer(
            None,
            answer(RENEWED_KEY_ID, Some(in_seconds(3600)))
                .to_string()
                .as_bytes(),
        );
        command.close_gate();
        let source = Arc::new(command.source());
        let first = source.credentials(&Cancel::new()).await.expect("first");
        let turns: Vec<_> = (0..16)
            .map(|_| {
                let source = Arc::clone(&source);
                tokio::spawn(async move {
                    tokio::time::timeout(
                        Duration::from_millis(500),
                        source.credentials(&Cancel::new()),
                    )
                    .await
                })
            })
            .collect();
        for turn in turns {
            let kept = turn
                .await
                .expect("the turn ran")
                .expect("answered without waiting for the refresh")
                .expect("the kept credentials");
            assert_eq!(kept.access_key_id(), ACCESS_KEY_ID);
            assert_eq!(kept.expires(), first.expires());
        }
        command.started(2).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(command.runs(), 2, "one refresh running, no other");
        assert!(source.refreshing());
        command.open_gate();
        settled(&source).await;
        let renewed = source.credentials(&Cancel::new()).await.expect("renewed");
        assert_eq!(renewed.access_key_id(), RENEWED_KEY_ID);
        assert_eq!(command.runs(), 2);
    });
}

fn an_expired_answer_makes_every_caller_wait_for_the_one_run() {
    runtime().block_on(async {
        let command = Command::new();
        let expires = in_seconds(2);
        command.answer(
            Some(0),
            answer(ACCESS_KEY_ID, Some(expires)).to_string().as_bytes(),
        );
        command.answer(
            None,
            answer(RENEWED_KEY_ID, Some(in_seconds(3600)))
                .to_string()
                .as_bytes(),
        );
        let source = Arc::new(command.source());
        source.credentials(&Cancel::new()).await.expect("first");
        // The first answer is inside the window: its refresh is stalled at the gate until it
        // has expired.
        command.close_gate();
        let left = expires
            .duration_since(SystemTime::now())
            .unwrap_or_default();
        tokio::time::sleep(left + Duration::from_millis(100)).await;
        let turns: Vec<_> = (0..8)
            .map(|_| {
                let source = Arc::clone(&source);
                tokio::spawn(async move { source.credentials(&Cancel::new()).await })
            })
            .collect();
        command.started(2).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(turns.iter().all(|turn| !turn.is_finished()), "they wait");
        assert_eq!(command.runs(), 2, "one run for every waiting caller");
        command.open_gate();
        for turn in turns {
            let renewed = turn.await.expect("the turn ran").expect("renewed");
            assert_eq!(renewed.access_key_id(), RENEWED_KEY_ID);
        }
        assert_eq!(command.runs(), 2);
    });
}

fn a_failed_refresh_answers_the_kept_credentials_until_they_expire() {
    runtime().block_on(async {
        let command = Command::new();
        command.answer(
            Some(0),
            answer(ACCESS_KEY_ID, Some(in_seconds(120)))
                .to_string()
                .as_bytes(),
        );
        command.answer(None, b"{\"Version\": 2}");
        let source = command.source();
        let cancel = Cancel::new();
        let first = source.credentials(&cancel).await.expect("first");
        let kept = source.credentials(&cancel).await.expect("kept");
        assert_eq!(kept.access_key_id(), first.access_key_id());
        settled(&source).await;
        assert_eq!(command.runs(), 2);
        let again = source.credentials(&cancel).await.expect("still kept");
        assert_eq!(again.access_key_id(), first.access_key_id());
        assert_eq!(again.expires(), first.expires());
    });
}

fn each_malformed_answer_is_unauthorized_names_the_program_and_carries_no_output() {
    let valid = answer(ACCESS_KEY_ID, Some(in_seconds(3600)));
    let mut answers: Vec<Vec<u8>> = Vec::new();
    for version in [json!(2), json!(0), json!("1"), json!(1.5), Value::Null] {
        let mut wrong = valid.clone();
        wrong["Version"] = version;
        answers.push(wrong.to_string().into_bytes());
    }
    for field in ["Version", "AccessKeyId", "SecretAccessKey"] {
        let mut missing = valid.clone();
        missing.as_object_mut().expect("an object").remove(field);
        answers.push(missing.to_string().into_bytes());
    }
    for field in ["AccessKeyId", "SecretAccessKey", "SessionToken"] {
        for unusable in [
            json!(""),
            json!(7),
            json!("fixture key"),
            json!("fixture\nkey"),
        ] {
            let mut wrong = valid.clone();
            wrong[field] = unusable;
            answers.push(wrong.to_string().into_bytes());
        }
    }
    for expiration in [
        json!("tomorrow"),
        json!(""),
        json!(7),
        json!("2026-13-01T00:00:00Z"),
        json!(rfc3339(in_seconds(-60))),
    ] {
        let mut wrong = valid.clone();
        wrong["Expiration"] = expiration;
        answers.push(wrong.to_string().into_bytes());
    }
    answers.push(
        format!(
            r#"{{"Version":1,"AccessKeyId":"{ACCESS_KEY_ID}","SecretAccessKey":"{PRINTED}","SecretAccessKey":"{SECRET_ACCESS_KEY}"}}"#
        )
        .into_bytes(),
    );
    answers.push(format!("{PRINTED} is not JSON").into_bytes());
    answers.push(format!("{valid} {PRINTED}").into_bytes());
    answers.push(json!([valid.clone()]).to_string().into_bytes());
    answers.push(Vec::new());
    let mut not_utf8 = valid.to_string().into_bytes();
    not_utf8.insert(1, 0xff);
    answers.push(not_utf8);
    runtime().block_on(async {
        for bytes in &answers {
            let command = Command::new();
            command.answer(None, bytes);
            let error = command
                .source()
                .credentials(&Cancel::new())
                .await
                .expect_err(&String::from_utf8_lossy(bytes));
            assert_refused(&error);
            assert_eq!(command.runs(), 1);
        }
    });
}

fn an_answer_over_64_kib_is_unauthorized() {
    runtime().block_on(async {
        let command = Command::new();
        command.mode("flood");
        let error = command
            .source()
            .credentials(&Cancel::new())
            .await
            .expect_err("too long");
        assert_refused(&error);
        assert!(error.message.contains("64 KiB"), "{}", error.message);

        // Padding within the bound is still a usable answer.
        let padded = format!("{}{}", " ".repeat(60 * 1024), answer(ACCESS_KEY_ID, None));
        let command = Command::new();
        command.answer(None, padded.as_bytes());
        command
            .source()
            .credentials(&Cancel::new())
            .await
            .expect("an answer within 64 KiB");
    });
}

fn a_non_zero_exit_is_unauthorized_and_carries_no_output() {
    runtime().block_on(async {
        let command = Command::new();
        command.mode("exit");
        let error = command
            .source()
            .credentials(&Cancel::new())
            .await
            .expect_err("exit 3");
        assert_refused(&error);
        assert!(error.message.contains('3'), "{}", error.message);
    });
}

fn a_command_that_does_not_exit_within_the_limit_is_killed_and_unauthorized() {
    runtime().block_on(async {
        let command = Command::new();
        command.mode("sleep");
        let started = std::time::Instant::now();
        let error = command
            .limited(Duration::from_millis(300))
            .credentials(&Cancel::new())
            .await
            .expect_err("too slow");
        assert!(
            started.elapsed() < Duration::from_millis(1500),
            "{:?}",
            started.elapsed()
        );
        assert_refused(&error);
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert!(!command.path("survived").exists(), "the command was killed");
    });
}

fn dropping_the_source_kills_the_command_it_is_running() {
    runtime().block_on(async {
        let command = Command::new();
        command.mode("sleep");
        let source = command.source();
        let cancel = Cancel::new();
        let waiting = source.credentials(&cancel);
        let canceller = cancel.clone();
        let (error, ()) = tokio::join!(async { waiting.await.expect_err("cancelled") }, async {
            command.started(1).await;
            canceller.cancel();
        });
        assert_eq!(error.code, ErrorCode::Cancelled);
        assert!(source.refreshing(), "the run goes on for the next caller");
        drop(source);
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert!(!command.path("survived").exists(), "the command was killed");
    });
}

fn a_program_that_cannot_be_started_is_unauthorized_by_its_name() {
    runtime().block_on(async {
        let directory = tempfile::tempdir().expect("a directory");
        let missing = directory.path().join("no-such-credential-program");
        let error = ProcessCredentials::new(missing.display().to_string(), Vec::<String>::new())
            .expect("a command")
            .credentials(&Cancel::new())
            .await
            .expect_err("nothing to start");
        assert_eq!(error.code, ErrorCode::Unauthorized, "{error:?}");
        assert_eq!(error.dispatch, Dispatch::NotSent);
        assert!(
            error.message.contains("no-such-credential-program"),
            "{}",
            error.message
        );
    });
}

fn an_empty_program_is_refused_before_anything_runs() {
    let error = ProcessCredentials::new("", ["--profile".to_owned()]).expect_err("no program");
    assert_eq!(error.code, ErrorCode::Unauthorized, "{error:?}");
}

fn the_command_reads_no_input() {
    runtime().block_on(async {
        let command = Command::answering(&answer(ACCESS_KEY_ID, None));
        command.mode("stdin");
        let credentials = command
            .limited(Duration::from_secs(5))
            .credentials(&Cancel::new())
            .await
            .expect("stdin is at its end at once");
        assert_eq!(credentials.access_key_id(), ACCESS_KEY_ID);
    });
}

fn an_expiration_with_any_offset_is_read_as_its_instant() {
    runtime().block_on(async {
        let expires = in_seconds(7200);
        for zone in ["Z", "+00:00", "+02:00", "-05:30"] {
            let mut offset = answer(ACCESS_KEY_ID, None);
            offset["Expiration"] = json!(rfc3339_in(expires, zone));
            let command = Command::answering(&offset);
            let credentials = command
                .source()
                .credentials(&Cancel::new())
                .await
                .unwrap_or_else(|error| panic!("{zone}: {error}"));
            assert_eq!(credentials.expires(), Some(expires), "{zone}");
        }
    });
}

fn a_cancelled_caller_runs_nothing() {
    runtime().block_on(async {
        let command = Command::answering(&answer(ACCESS_KEY_ID, None));
        let cancel = Cancel::new();
        cancel.cancel();
        let error = command
            .source()
            .credentials(&cancel)
            .await
            .expect_err("cancelled");
        assert_eq!(error.code, ErrorCode::Cancelled);
        assert_eq!(command.runs(), 0);
    });
}

fn debug_output_shows_no_key() {
    runtime().block_on(async {
        let command = Command::answering(&answer(ACCESS_KEY_ID, Some(in_seconds(3600))));
        let source = command.source();
        let credentials = source.credentials(&Cancel::new()).await.expect("kept");
        for output in [format!("{source:?}"), format!("{credentials:?}")] {
            for secret in SECRETS {
                assert!(!output.contains(secret), "{output}");
            }
        }
        assert!(format!("{source:?}").contains(&program()), "{source:?}");
    });
}
