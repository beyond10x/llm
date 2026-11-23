//! `llm-xtask list-check <binary>...` runs each test binary as cargo-nextest lists it,
//! `--list --format terse` with and without `--ignored`, and fails unless every line it writes is
//! `<name>: test` or `<name>: benchmark`. `llm-xtask test` runs the same check on every
//! `harness = false` test target before its runs, so a runner of its own that nextest cannot list
//! fails the local gate as it fails the forge, by name.

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

/// An executable shell script standing in for a test binary: it prints `listed` for
/// `--list --format terse` and `ignored` when `--ignored` is added.
fn binary(directory: &Path, name: &str, listed: &str, ignored: &str) -> PathBuf {
    let path = directory.join(name);
    let script = format!(
        "#!/bin/sh\ncase \" $* \" in\n  *' --ignored '*) printf '%s' '{ignored}' ;;\n  *' --list '*) printf '%s' '{listed}' ;;\n  *) exit 9 ;;\nesac\n"
    );
    fs::write(&path, script).expect("the script is written");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("the script is executable");
    path
}

fn list_check(binaries: &[PathBuf]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_llm-xtask"))
        .arg("list-check")
        .args(binaries)
        .output()
        .expect("llm-xtask runs")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn a_binary_listing_its_tests_as_libtest_does_passes() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let libtest = binary(
        directory.path(),
        "libtest",
        "first_case: test\nsecond_case: test\nbench_case: benchmark\n",
        "",
    );
    let empty = binary(directory.path(), "empty", "", "");
    let output = list_check(&[libtest, empty]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}{}",
        text(&output.stdout),
        text(&output.stderr)
    );
}

#[test]
fn a_runner_that_ignores_list_and_runs_its_cases_fails_by_name() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let libtest = binary(directory.path(), "libtest", "first_case: test\n", "");
    let own = binary(
        directory.path(),
        "process_credentials",
        "\nrunning 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 19 filtered out\n\n",
        "",
    );
    let output = list_check(&[libtest, own]);
    let stderr = text(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("process_credentials"), "{stderr}");
    assert!(stderr.contains("--list --format terse"), "{stderr}");
    assert!(stderr.contains("<name>: test"), "{stderr}");
}

#[test]
fn a_runner_listing_an_ignored_case_in_another_form_fails() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let own = binary(
        directory.path(),
        "own",
        "first_case: test\n",
        "first_case ... ignored\n",
    );
    let output = list_check(&[own]);
    let stderr = text(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("--ignored"), "{stderr}");
}
