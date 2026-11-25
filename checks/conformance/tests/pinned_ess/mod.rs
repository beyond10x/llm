//! `llm-conformance check` and `llm-conformance ess` run only the `ess` `gate.yml` pins
//! (`story:ess-pin-current-and-local-gate-finds-it`). Each case starts the built runner at the
//! repository root with a stand-in `ess` as the only program on its `PATH`, a store of its own
//! for pinned binaries and a release address on this disk, so no case needs a real `ess` or a
//! network.

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

fn repository() -> PathBuf {
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    Path::new(&manifest)
        .join("../..")
        .canonicalize()
        .expect("the repository root exists")
}

/// The version the "Install ess" step of `gate.yml` names.
fn pinned() -> String {
    let text = fs::read_to_string(repository().join(".github/workflows/gate.yml"))
        .expect("gate.yml is read");
    let workflow: serde_yaml::Value = serde_yaml::from_str(&text).expect("gate.yml is YAML");
    workflow["jobs"]["libraries-and-plan"]["steps"]
        .as_sequence()
        .expect("steps")
        .iter()
        .find(|step| step["name"].as_str() == Some("Install ess"))
        .and_then(|step| step["env"]["ESS_VERSION"].as_str())
        .expect("the step names a version")
        .to_owned()
}

/// What one run of the runner left behind.
struct Run {
    output: Output,
    /// The runner's process id; `check` names its projection directory by it.
    process: u32,
    /// Every call a stand-in `ess` received, one line each: which one, then its arguments.
    calls: Vec<String>,
    /// What the store holds afterwards.
    stored: Vec<PathBuf>,
}

impl Run {
    fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.output.stderr).into_owned()
    }

    /// The directory `check` projects into, under the repository's `target/`.
    fn projection(&self) -> PathBuf {
        repository().join(format!("target/conformance/projection-{}", self.process))
    }
}

/// Writes an `ess` at `place` that notes `<name> <arguments>` in `log` for each call, writes
/// `reply` for `--version` and ends 7 for every other call.
fn stand_in(place: &Path, name: &str, reply: &str, log: &Path) {
    fs::create_dir_all(place.parent().expect("a parent")).expect("a directory for the stand-in");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"{name} $*\" >> '{}'\nif [ \"$*\" = '--version' ]; then\n  printf '%s\\n' '{reply}'\n  exit 0\nfi\nexit 7\n",
        log.display()
    );
    fs::write(place, script).expect("the stand-in is written");
    fs::set_permissions(place, fs::Permissions::from_mode(0o755))
        .expect("the stand-in is executable");
}

/// Runs `llm-conformance <arguments>` at the repository root. The only program on its `PATH` is
/// an `ess` that reports `installed`; its store holds an `ess` that reports `stored`, or nothing.
fn run(arguments: &[&str], installed: &str, stored: Option<&str>) -> Run {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let bin = directory.path().join("bin");
    let store = directory.path().join("store");
    let log = directory.path().join("calls");
    stand_in(&bin.join("ess"), "path", installed, &log);
    if let Some(reply) = stored {
        let name = format!("ess-{}-x86_64-unknown-linux-gnu", pinned());
        stand_in(&store.join(name).join("ess"), "stored", reply, &log);
    }
    let child = Command::new(env!("CARGO_BIN_EXE_llm-conformance"))
        .args(arguments)
        .current_dir(repository())
        .env("PATH", &bin)
        .env("LLM_ESS_STORE", &store)
        .env(
            "LLM_ESS_RELEASES",
            format!("file://{}/no-release", directory.path().display()),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("llm-conformance starts");
    let process = child.id();
    let output = child.wait_with_output().expect("llm-conformance ends");
    let calls = fs::read_to_string(&log)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect();
    let stored = fs::read_dir(&store)
        .map(|entries| {
            entries
                .map(|entry| entry.expect("an entry").path())
                .collect()
        })
        .unwrap_or_default();
    Run {
        output,
        process,
        calls,
        stored,
    }
}

#[test]
fn another_ess_on_path_is_refused_with_both_versions_before_a_projection_is_made() {
    let pinned = pinned();
    assert_ne!(pinned, "0.0.1");
    let run = run(&["check"], "ess 0.0.1", None);
    let stderr = run.stderr();
    assert_eq!(run.output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("ess 0.0.1"), "the version found: {stderr}");
    assert!(
        stderr.contains(&format!("ess {pinned} is needed")),
        "the version needed: {stderr}"
    );
    assert_eq!(
        run.calls,
        ["path --version"],
        "the other ess projected nothing: {stderr}"
    );
    assert!(!run.projection().exists(), "{}", run.projection().display());
    assert!(run.stored.is_empty(), "{:?}", run.stored);
}

#[test]
fn validating_the_specification_refuses_another_ess_the_same_way() {
    let pinned = pinned();
    let run = run(
        &["ess", "specify", "validate", "--path", "spec"],
        "ess 0.0.1",
        None,
    );
    let stderr = run.stderr();
    assert_eq!(run.output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("ess 0.0.1"), "the version found: {stderr}");
    assert!(
        stderr.contains(&format!("ess {pinned} is needed")),
        "the version needed: {stderr}"
    );
    assert_eq!(run.calls, ["path --version"], "{stderr}");
}

#[test]
fn the_pinned_ess_on_path_gets_the_arguments_and_its_exit_status_is_passed_on() {
    let pinned = pinned();
    let run = run(
        &["ess", "specify", "validate", "--path", "spec"],
        &format!("ess {pinned}"),
        None,
    );
    let stderr = run.stderr();
    assert_eq!(
        run.calls,
        ["path --version", "path specify validate --path spec"],
        "{stderr}"
    );
    assert_eq!(run.output.status.code(), Some(7), "{stderr}");
    assert!(
        run.stored.is_empty(),
        "nothing is fetched: {:?}",
        run.stored
    );
}

/// The `ess` that was found is the one that runs: with the pinned release stored and another
/// version on `PATH`, every call goes to the stored one.
#[test]
fn with_another_ess_on_path_the_stored_one_validates() {
    let stored = format!("ess {}", pinned());
    let validated = run(
        &["ess", "specify", "validate", "--path", "spec"],
        "ess 0.0.1",
        Some(&stored),
    );
    let stderr = validated.stderr();
    assert_eq!(
        validated.calls,
        ["stored --version", "stored specify validate --path spec"],
        "{stderr}"
    );
    assert_eq!(validated.output.status.code(), Some(7), "{stderr}");
}

#[test]
fn with_another_ess_on_path_the_stored_one_projects() {
    let stored = format!("ess {}", pinned());
    let checked = run(&["check"], "ess 0.0.1", Some(&stored));
    let stderr = checked.stderr();
    // The stand-in ends 7 for the first projection, so the check ends there; the directory it
    // made for that projection is this case's to remove.
    let _ = fs::remove_dir_all(checked.projection());
    assert_eq!(
        checked.calls,
        [
            "stored --version".to_owned(),
            format!(
                "stored verify conform synthesize --path spec --suite-format 5 --target ir \
                 --scenarios contracts --out target/conformance/projection-{}/suite.json",
                checked.process
            ),
        ],
        "{stderr}"
    );
    assert_eq!(checked.output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("ESS projection refused"), "{stderr}");
}

/// `llm-conformance ess <arguments>` hands every argument to the pinned `ess`, the help flags as
/// the first argument among them, and passes its exit status on.
#[test]
fn adversary_w32_a_help_flag_as_the_first_argument_reaches_the_pinned_ess() {
    let pinned = pinned();
    for flag in ["--help", "-h"] {
        let run = run(&["ess", flag], &format!("ess {pinned}"), None);
        let stderr = run.stderr();
        assert_eq!(
            run.calls,
            ["path --version".to_owned(), format!("path {flag}")],
            "{flag}: {stderr}"
        );
        assert_eq!(run.output.status.code(), Some(7), "{flag}: {stderr}");
    }
}

/// What an interrupted fetch leaves in the store is never used. The runner's `curl` is a
/// stand-in that ends the runner with the signal an interrupt sends, so the fetch stops in its
/// first download, does not return and may leave its `.fetch-*` directory. The next run reads
/// none of it: with an `ess` of the pinned version wherever a release has one, in every
/// directory the first run left and in one more, it starts none of them, stores nothing from
/// them and goes to fetch the release again.
///
/// The runner reads the pin of `gate.yml`, and no asset made here has that digest, so the next
/// run's own fetch ends in a refusal. That a fetch which succeeds beside a leftover is stored and
/// used is held where a release can be made for the pin: `src/ess.rs`,
/// `a_leftover_of_an_interrupted_fetch_is_never_read_and_the_release_is_fetched`.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn adversary_w32_an_interrupted_fetch_is_never_used() {
    use std::os::unix::process::ExitStatusExt;

    /// Writes a `curl` at `place` that runs `script`.
    fn curl(place: &Path, script: &str) {
        fs::write(place, format!("#!/bin/sh\n{script}\n")).expect("the curl stand-in");
        fs::set_permissions(place, fs::Permissions::from_mode(0o755)).expect("it is executable");
    }

    let directory = tempfile::tempdir().expect("a temporary directory");
    let first = directory.path().join("first");
    let next = directory.path().join("next");
    let store = directory.path().join("store");
    let log = directory.path().join("calls");
    let pinned = pinned();
    stand_in(&first.join("ess"), "path", "ess 0.0.1", &log);
    stand_in(&next.join("ess"), "path", "ess 0.0.1", &log);
    curl(&first.join("curl"), "kill -TERM \"$PPID\"\nexit 1");
    curl(
        &next.join("curl"),
        "echo 'curl: no release here' >&2\nexit 22",
    );
    let validate = |bin: &Path| {
        Command::new(env!("CARGO_BIN_EXE_llm-conformance"))
            .args(["ess", "specify", "validate", "--path", "spec"])
            .current_dir(repository())
            .env("PATH", bin)
            .env("LLM_ESS_STORE", &store)
            .env(
                "LLM_ESS_RELEASES",
                format!("file://{}/no-release", directory.path().display()),
            )
            .stdin(Stdio::null())
            .output()
            .expect("llm-conformance runs")
    };

    let interrupted = validate(first.as_path());
    let stderr = String::from_utf8_lossy(&interrupted.stderr).into_owned();
    assert_eq!(
        interrupted.status.signal(),
        Some(15),
        "the runner was interrupted in its fetch: {stderr}"
    );
    let left: Vec<PathBuf> = fs::read_dir(&store)
        .map(|entries| {
            entries
                .map(|entry| entry.expect("an entry").path())
                .collect()
        })
        .unwrap_or_default();
    for leftover in &left {
        let name = leftover.file_name().expect("a name").to_string_lossy();
        assert!(
            name.starts_with(".fetch-"),
            "only a `.fetch-*` directory is left in the store: {left:?}"
        );
    }

    let release = format!("ess-{pinned}-x86_64-unknown-linux-gnu");
    let planted = store.join(".fetch-planted");
    for leftover in left.iter().chain([&planted]) {
        let reply = format!("ess {pinned}");
        stand_in(&leftover.join("ess"), "leftover", &reply, &log);
        stand_in(
            &leftover.join(&release).join("ess"),
            "leftover",
            &reply,
            &log,
        );
    }

    let refused = validate(next.as_path());
    let stderr = String::from_utf8_lossy(&refused.stderr).into_owned();
    assert_eq!(refused.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains(&format!("ess {pinned} is needed")),
        "the version needed: {stderr}"
    );
    assert!(
        stderr.contains("fetch: curl ended with"),
        "the next run went to fetch the release again: {stderr}"
    );
    let calls = fs::read_to_string(&log).expect("the calls");
    assert_eq!(
        calls.lines().collect::<Vec<_>>(),
        ["path --version", "path --version"],
        "one call in each run, and none to an `ess` of the leftover: {stderr}"
    );
    assert!(
        !store.join(&release).exists(),
        "nothing of the leftover was stored: {stderr}"
    );
}

/// Every argument after `ess` reaches the pinned `ess` as written: the runner answers no flag
/// itself and takes no `--` away.
#[test]
fn every_argument_after_ess_reaches_the_pinned_ess_as_written() {
    let pinned = pinned();
    let reply = format!("ess {pinned}");
    for arguments in [
        &["-V"][..],
        &["--"],
        &["--", "--help"],
        &["--", "specify", "validate"],
        &["specify", "--help"],
        &["specify", "-h", "--", "--version"],
        &["help"],
        &["--unknown=1", "-x"],
    ] {
        let mut whole = vec!["ess"];
        whole.extend_from_slice(arguments);
        let ran = run(&whole, &reply, None);
        let stderr = ran.stderr();
        assert_eq!(
            ran.calls,
            [
                "path --version".to_owned(),
                format!("path {}", arguments.join(" "))
            ],
            "{arguments:?}: {stderr}"
        );
        assert_eq!(ran.output.status.code(), Some(7), "{arguments:?}: {stderr}");
    }

    // A version flag is the pinned ess's to answer; its answer and its status come back.
    let ran = run(&["ess", "--version"], &reply, None);
    assert_eq!(ran.calls, ["path --version", "path --version"]);
    assert_eq!(ran.output.status.code(), Some(0), "{}", ran.stderr());
    assert_eq!(
        String::from_utf8_lossy(&ran.output.stdout),
        format!("{reply}\n")
    );

    // With no argument the pinned ess is started with none.
    let ran = run(&["ess"], &reply, None);
    assert_eq!(ran.calls, ["path --version", "path "], "{}", ran.stderr());
    assert_eq!(ran.output.status.code(), Some(7), "{}", ran.stderr());
}
