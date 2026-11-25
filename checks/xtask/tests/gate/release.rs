//! `llm-xtask release-tag <version> <sha>` makes the release tag only when every run of `Gate`,
//! `Shared source gates` and `Documentation validation` on that commit ended `success`, and
//! refuses it, before any `b10x-gates bot` call, while one is absent, pending or red.
//!
//! No case reaches the network or the real `b10x-gates`: the command runs with an environment
//! that holds nothing but a `PATH` of one temporary directory, in which a stand-in `b10x-gates`
//! prints a recorded run list for `gh`, appends the arguments of every call to a log and exits
//! with the status the case chooses.

use super::read;
use std::{
    fs, io,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output},
};

const VERSION: &str = "0.31.0";
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const OTHER_SHA: &str = "89abcdef0123456789abcdef0123456789abcdef";

const GATE: &str = "Gate";
const SHARED: &str = "Shared source gates";
const DOCUMENTATION: &str = "Documentation validation";
const REQUIRED: [&str; 3] = [GATE, SHARED, DOCUMENTATION];
const SITE: &str = "Documentation site";

/// The members of one run as `gh run list --json` writes them.
const MEMBERS: [&str; 5] = ["conclusion", "event", "headSha", "name", "status"];

/// The exit status of the stand-in for each of the three calls.
struct Exits {
    gh: u8,
    tag: u8,
    push: u8,
}

const ALL_SUCCEED: Exits = Exits {
    gh: 0,
    tag: 0,
    push: 0,
};

/// What one start of the command left behind.
struct Outcome {
    code: Option<i32>,
    stdout: String,
    stderr: String,
    /// The arguments of each call of the stand-in, in order, every argument in brackets.
    calls: Vec<String>,
}

fn entry(name: &str, status: &str, conclusion: &str, event: &str, head: &str) -> String {
    format!(
        "{{\"conclusion\":\"{conclusion}\",\"event\":\"{event}\",\"headSha\":\"{head}\",\
         \"name\":\"{name}\",\"status\":\"{status}\"}}"
    )
}

/// A run on the commit under test.
fn run(name: &str, status: &str, conclusion: &str) -> String {
    entry(name, status, conclusion, "push", SHA)
}

fn green(name: &str) -> String {
    run(name, "completed", "success")
}

fn list(entries: &[String]) -> String {
    format!("[{}]", entries.join(","))
}

/// The run list recorded for a tagged commit: the site's run, `Shared source gates` twice, then
/// the other two, all green.
fn recorded() -> Vec<String> {
    vec![
        entry(SITE, "completed", "success", "workflow_run", SHA),
        green(SHARED),
        green(SHARED),
        green(DOCUMENTATION),
        green(GATE),
    ]
}

/// One green run of each required workflow, and `more` beside them.
fn green_with(more: &[String]) -> String {
    let mut entries = vec![green(SHARED), green(DOCUMENTATION), green(GATE)];
    entries.extend_from_slice(more);
    list(&entries)
}

/// One green run of each required workflow but `replaced`, and `instead` in its place.
fn green_but(replaced: &str, instead: &[String]) -> String {
    let mut entries: Vec<String> = REQUIRED
        .iter()
        .filter(|name| **name != replaced)
        .map(|name| green(name))
        .collect();
    entries.extend_from_slice(instead);
    list(&entries)
}

/// Writes the stand-in `b10x-gates` into `directory/bin`. It uses only what `sh` has built in,
/// because the command's `PATH` holds nothing else. A `sh` child writes the file, so no thread
/// of this process ever holds it open for writing while another starts a child (a file open for
/// writing cannot be executed).
fn stand_in(directory: &Path, printed: &str, exits: &Exits) {
    assert!(!printed.contains('\''), "{printed}");
    let bin = directory.join("bin");
    fs::create_dir(&bin).expect("the directory is made");
    let log = directory.join("calls.log");
    let log = log.to_str().expect("a UTF-8 path");
    assert!(!log.contains('\''), "{log}");
    let script = format!(
        "#!/bin/sh\nfor argument in \"$@\"; do printf '[%s]' \"$argument\"; done >> '{log}'\n\
         printf '\\n' >> '{log}'\ncase \"$1:$2\" in\n  gh:*) printf '%s' '{printed}'; exit {} ;;\n  \
         bot:--) exit {} ;;\n  bot:--repository) exit {} ;;\n  *) exit 97 ;;\nesac\n",
        exits.gh, exits.tag, exits.push
    );
    let path = bin.join("b10x-gates");
    let written = Command::new("sh")
        .args(["-c", "printf '%s' \"$1\" > \"$2\"", "sh"])
        .arg(&script)
        .arg(&path)
        .status()
        .expect("sh runs");
    assert!(written.success(), "the stand-in is written");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
        .expect("the stand-in is executable");
}

fn start(directory: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_llm-xtask"))
        .arg("release-tag")
        .args(arguments)
        .env_clear()
        .env("PATH", directory.join("bin"))
        .current_dir(directory)
        .output()
        .expect("llm-xtask runs")
}

fn calls(directory: &Path) -> Vec<String> {
    match fs::read_to_string(directory.join("calls.log")) {
        Ok(log) => log.lines().map(str::to_owned).collect(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => panic!("reading the log: {error}"),
    }
}

fn outcome(directory: &Path, output: &Output) -> Outcome {
    Outcome {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        calls: calls(directory),
    }
}

/// Starts `release-tag <version> <sha>` against a stand-in that prints `printed` for `gh`.
fn release(printed: &str, exits: &Exits, version: &str, sha: &str) -> Outcome {
    let directory = tempfile::tempdir().expect("a temporary directory");
    stand_in(directory.path(), printed, exits);
    let output = start(directory.path(), &[version, sha]);
    outcome(directory.path(), &output)
}

/// Starts it for the version and commit under test against a stand-in whose calls all succeed.
fn release_over(printed: &str) -> Outcome {
    release(printed, &ALL_SUCCEED, VERSION, SHA)
}

fn gh_call() -> String {
    format!(
        "[gh][--][run][list][--repo][beyond10x/llm][--commit][{SHA}][--limit][100][--json]\
         [name,status,conclusion,event,headSha]"
    )
}

fn tag_call() -> String {
    format!("[bot][--][tag][-a][{VERSION}][-m][LLM {VERSION}][{SHA}]")
}

fn push_call() -> String {
    format!("[bot][--repository][beyond10x/llm][--][push][origin][refs/tags/{VERSION}]")
}

impl Outcome {
    fn shown(&self) -> String {
        format!(
            "exit {:?}\nstdout: {}\nstderr: {}\ncalls: {:#?}",
            self.code, self.stdout, self.stderr, self.calls
        )
    }

    /// The runs were read, then the tag was made, then pushed, and the command succeeded.
    fn assert_tagged(&self, case: &str) {
        assert_eq!(
            self.calls,
            [gh_call(), tag_call(), push_call()],
            "{case}: {}",
            self.shown()
        );
        assert_eq!(self.code, Some(0), "{case}: {}", self.shown());
    }

    /// The runs were asked for, nothing else was called, and the command failed as a refusal.
    fn assert_refused(&self, case: &str) {
        assert_eq!(self.calls, [gh_call()], "{case}: {}", self.shown());
        assert_eq!(self.code, Some(1), "{case}: {}", self.shown());
        assert!(
            self.stderr.contains("release-tag: refused"),
            "{case}: {}",
            self.shown()
        );
    }

    fn assert_names(&self, line: &str, case: &str) {
        assert!(
            self.stderr.lines().any(|written| written.trim() == line),
            "{case}: no line {line:?}: {}",
            self.shown()
        );
    }

    /// The refusal names no run of `workflow`.
    fn assert_silent_about(&self, workflow: &str, case: &str) {
        let prefix = format!("{workflow}: ");
        assert!(
            !self
                .stderr
                .lines()
                .any(|written| written.trim().starts_with(&prefix)),
            "{case}: {workflow} is named: {}",
            self.shown()
        );
    }
}

#[test]
fn three_green_workflows_one_of_them_run_twice_are_tagged_then_pushed() {
    let outcome = release_over(&list(&recorded()));
    outcome.assert_tagged("the recorded list");
    assert!(!outcome.stderr.contains("refused"), "{}", outcome.shown());
}

#[test]
fn a_gate_run_in_progress_refuses_the_tag_and_is_named_with_its_status() {
    let outcome = release_over(&green_but(GATE, &[run(GATE, "in_progress", "")]));
    outcome.assert_refused("in_progress");
    outcome.assert_names("Gate: pending (in_progress)", "in_progress");
    outcome.assert_silent_about(SHARED, "in_progress");
    outcome.assert_silent_about(DOCUMENTATION, "in_progress");
}

#[test]
fn a_queued_gate_run_refuses_the_tag_and_is_named_with_its_status() {
    let outcome = release_over(&green_but(GATE, &[run(GATE, "queued", "")]));
    outcome.assert_refused("queued");
    outcome.assert_names("Gate: pending (queued)", "queued");
}

#[test]
fn every_status_other_than_completed_is_pending_whatever_the_conclusion_says() {
    for (status, conclusion) in [
        ("waiting", ""),
        ("requested", ""),
        ("pending", ""),
        ("in_progress", "success"),
        ("Completed", "success"),
        ("completed ", "success"),
        ("complete", "success"),
        ("completed_late", "success"),
    ] {
        let outcome = release_over(&green_but(GATE, &[run(GATE, status, conclusion)]));
        outcome.assert_refused(status);
        outcome.assert_names(&format!("Gate: pending ({status})"), status);
    }
    let outcome = release_over(&green_but(GATE, &[run(GATE, "", "success")]));
    outcome.assert_refused("an empty status");
    outcome.assert_names("Gate: pending (no status)", "an empty status");
}

fn a_red_documentation_run(conclusion: &str, named: &str) {
    let outcome = release_over(&green_but(
        DOCUMENTATION,
        &[run(DOCUMENTATION, "completed", conclusion)],
    ));
    outcome.assert_refused(conclusion);
    outcome.assert_names(
        &format!("Documentation validation: red ({named})"),
        conclusion,
    );
    outcome.assert_silent_about(GATE, conclusion);
    outcome.assert_silent_about(SHARED, conclusion);
}

#[test]
fn a_failed_documentation_validation_refuses_the_tag() {
    a_red_documentation_run("failure", "failure");
}

#[test]
fn a_cancelled_documentation_validation_refuses_the_tag() {
    a_red_documentation_run("cancelled", "cancelled");
}

#[test]
fn a_skipped_documentation_validation_refuses_the_tag() {
    a_red_documentation_run("skipped", "skipped");
}

#[test]
fn a_completed_documentation_validation_without_a_conclusion_refuses_the_tag() {
    a_red_documentation_run("", "no conclusion");
}

#[test]
fn every_conclusion_other_than_exactly_success_is_red() {
    for conclusion in [
        "timed_out",
        "neutral",
        "action_required",
        "stale",
        "startup_failure",
        "successful",
        "success_with_warnings",
        "succes",
        "Success",
        "SUCCESS",
        " success",
        "success ",
    ] {
        a_red_documentation_run(conclusion, conclusion);
    }
}

#[test]
fn a_required_workflow_without_a_run_refuses_the_tag_and_is_named_absent() {
    for absent in REQUIRED {
        let outcome = release_over(&green_but(absent, &[]));
        outcome.assert_refused(absent);
        outcome.assert_names(&format!("{absent}: absent"), absent);
        for present in REQUIRED.iter().filter(|name| **name != absent) {
            outcome.assert_silent_about(present, absent);
        }
    }
}

#[test]
fn an_empty_list_refuses_the_tag_and_names_all_three_absent() {
    let outcome = release_over("[]");
    outcome.assert_refused("an empty list");
    for absent in REQUIRED {
        outcome.assert_names(&format!("{absent}: absent"), "an empty list");
    }
}

#[test]
fn one_red_run_beside_a_green_one_of_the_same_workflow_refuses_the_tag() {
    let red = run(SHARED, "completed", "failure");
    for (case, pair) in [
        ("green then red", [green(SHARED), red.clone()]),
        ("red then green", [red.clone(), green(SHARED)]),
    ] {
        let outcome = release_over(&green_but(SHARED, &pair));
        outcome.assert_refused(case);
        outcome.assert_names("Shared source gates: red (failure)", case);
        assert!(!outcome.stderr.contains("absent"), "{}", outcome.shown());
    }
    let pending = run(SHARED, "in_progress", "");
    let outcome = release_over(&green_but(SHARED, &[green(SHARED), pending]));
    outcome.assert_refused("green then pending");
    outcome.assert_names("Shared source gates: pending (in_progress)", "pending");
}

#[test]
fn a_green_run_on_another_commit_does_not_stand_for_the_absent_one() {
    let elsewhere = entry(GATE, "completed", "success", "push", OTHER_SHA);
    let outcome = release_over(&green_but(GATE, &[elsewhere]));
    outcome.assert_refused("Gate green on another commit");
    outcome.assert_names("Gate: absent", "Gate green on another commit");
}

#[test]
fn a_red_run_on_another_commit_does_not_block_the_tag() {
    let elsewhere = entry(GATE, "completed", "failure", "push", OTHER_SHA);
    let pending = entry(SHARED, "in_progress", "", "push", OTHER_SHA);
    release_over(&green_with(&[elsewhere, pending])).assert_tagged("runs on another commit");
}

#[test]
fn a_documentation_site_run_in_progress_is_named_and_does_not_block_the_tag() {
    let site = entry(SITE, "in_progress", "", "workflow_run", SHA);
    let outcome = release_over(&green_with(&[site]));
    outcome.assert_tagged("the site in progress");
    assert!(
        outcome
            .stdout
            .lines()
            .any(|line| line == "release-tag: not required: Documentation site: in_progress"),
        "{}",
        outcome.shown()
    );
}

#[test]
fn a_failed_documentation_site_run_is_named_and_does_not_block_the_tag() {
    let site = entry(SITE, "completed", "failure", "workflow_run", SHA);
    let outcome = release_over(&green_with(&[site]));
    outcome.assert_tagged("the site failed");
    assert!(
        outcome
            .stdout
            .lines()
            .any(|line| line == "release-tag: not required: Documentation site: failure"),
        "{}",
        outcome.shown()
    );
}

#[test]
fn a_name_that_differs_by_case_or_has_more_text_is_not_the_gate() {
    for other in ["gate", "GATE", "Gate 2", "Gates", " Gate", "Gate "] {
        let outcome = release_over(&green_but(GATE, &[green(other)]));
        outcome.assert_refused(other);
        outcome.assert_names("Gate: absent", other);
        let outcome = release_over(&green_with(&[run(other, "completed", "failure")]));
        outcome.assert_tagged(other);
        assert!(
            outcome
                .stdout
                .lines()
                .any(|line| line == format!("release-tag: not required: {other}: failure")),
            "{other}: {}",
            outcome.shown()
        );
    }
}

#[test]
fn a_run_list_command_that_fails_refuses_the_tag_whatever_it_printed() {
    let exits = Exits {
        gh: 1,
        ..ALL_SUCCEED
    };
    release(&list(&recorded()), &exits, VERSION, SHA).assert_refused("gh exits 1");
}

#[test]
fn output_that_is_not_json_refuses_the_tag() {
    release_over("no runs found").assert_refused("text");
    release_over("").assert_refused("nothing");
    let cut = list(&recorded());
    release_over(&cut[..cut.len() - 1]).assert_refused("a list cut before its end");
}

#[test]
fn output_that_is_not_an_array_of_runs_refuses_the_tag() {
    let whole = list(&recorded());
    release_over(&format!("{{\"runs\":{whole}}}")).assert_refused("an object");
    release_over("null").assert_refused("null");
    release_over(&green_with(&["\"Gate\"".to_owned()])).assert_refused("a string entry");
    release_over(&green_with(&[format!("[{}]", green(GATE))])).assert_refused("a nested list");
}

#[test]
fn an_entry_without_one_of_its_members_refuses_the_tag() {
    for member in MEMBERS {
        let whole = green(GATE);
        let object: serde_json::Value = serde_json::from_str(&whole).expect("an entry is JSON");
        let mut lacking = object.clone();
        lacking
            .as_object_mut()
            .expect("an entry is an object")
            .remove(member);
        let case = format!("Gate without {member}");
        release_over(&green_but(GATE, &[whole.clone(), lacking.to_string()])).assert_refused(&case);
        let mut null = object;
        null[member] = serde_json::Value::Null;
        let case = format!("Gate with a null {member}");
        release_over(&green_but(GATE, &[whole, null.to_string()])).assert_refused(&case);
    }
    let site = "{\"event\":\"workflow_run\",\"headSha\":\"0123456789abcdef0123456789abcdef01234567\",\
                \"name\":\"Documentation site\",\"status\":\"completed\"}";
    release_over(&green_with(&[site.to_owned()])).assert_refused("the site without conclusion");
}

#[test]
fn a_list_as_long_as_its_limit_refuses_the_tag_because_it_may_be_cut() {
    let mut entries = recorded();
    while entries.len() < 99 {
        entries.push(green(GATE));
    }
    release_over(&list(&entries)).assert_tagged("99 runs");
    entries.push(green(GATE));
    release_over(&list(&entries)).assert_refused("100 runs");
}

#[test]
fn without_the_gates_command_the_tag_is_refused() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    fs::create_dir(directory.path().join("bin")).expect("the directory is made");
    let output = start(directory.path(), &[VERSION, SHA]);
    let outcome = outcome(directory.path(), &output);
    assert_eq!(outcome.code, Some(1), "{}", outcome.shown());
    assert!(
        outcome.stderr.contains("release-tag: refused") && outcome.stderr.contains("`b10x-gates`"),
        "{}",
        outcome.shown()
    );
}

#[test]
fn a_tag_call_that_fails_stops_before_the_push() {
    let exits = Exits {
        tag: 1,
        ..ALL_SUCCEED
    };
    let outcome = release(&list(&recorded()), &exits, VERSION, SHA);
    assert_eq!(
        outcome.calls,
        [gh_call(), tag_call()],
        "{}",
        outcome.shown()
    );
    assert_eq!(outcome.code, Some(1), "{}", outcome.shown());
}

#[test]
fn a_push_call_that_fails_fails_the_command() {
    let exits = Exits {
        push: 1,
        ..ALL_SUCCEED
    };
    let outcome = release(&list(&recorded()), &exits, VERSION, SHA);
    assert_eq!(
        outcome.calls,
        [gh_call(), tag_call(), push_call()],
        "{}",
        outcome.shown()
    );
    assert_eq!(outcome.code, Some(1), "{}", outcome.shown());
}

/// A value the command refuses before it runs anything: the log stays empty.
fn refused_before_any_call(version: &str, sha: &str, named: &str) {
    let outcome = release(&list(&recorded()), &ALL_SUCCEED, version, sha);
    let case = format!("{version:?} {sha:?}");
    assert_eq!(outcome.calls, [""; 0], "{case}: {}", outcome.shown());
    assert_eq!(outcome.code, Some(1), "{case}: {}", outcome.shown());
    assert!(
        outcome
            .stderr
            .contains(&format!("release-tag: refused: {named}")),
        "{case}: {}",
        outcome.shown()
    );
}

#[test]
fn a_sha_that_is_not_forty_lower_case_hexadecimal_characters_is_refused_before_any_call() {
    for sha in [
        &SHA[..39],
        "0123456789abcdef0123456789abcdef012345678",
        "0123456789ABCDEF0123456789abcdef01234567",
        "0123456789abcdef0123456789abcdef0123456g",
        "0123456789abcdef0123456789abcdef0123456 ",
        "-123456789abcdef0123456789abcdef01234567",
        "main",
        "",
    ] {
        refused_before_any_call(VERSION, sha, "the commit");
    }
}

#[test]
fn a_version_that_is_not_digits_and_dots_is_refused_before_any_call() {
    for version in [
        "v0.31.0",
        "0.31 .0",
        " 0.31.0",
        "-0.31.0",
        "--force",
        "0.31.0-rc1",
        "0..31",
        ".0.31",
        "0.31.",
        ".",
        "",
    ] {
        refused_before_any_call(version, SHA, "the version");
    }
    refused_before_any_call(SHA, VERSION, "the version");
}

#[test]
fn any_other_number_of_arguments_is_a_usage_error_and_calls_nothing() {
    for arguments in [&[][..], &[VERSION], &[VERSION, SHA, SHA]] {
        let directory = tempfile::tempdir().expect("a temporary directory");
        stand_in(directory.path(), &list(&recorded()), &ALL_SUCCEED);
        let output = start(directory.path(), arguments);
        let outcome = outcome(directory.path(), &output);
        assert_eq!(outcome.calls, [""; 0], "{}", outcome.shown());
        assert_eq!(outcome.code, Some(2), "{}", outcome.shown());
        assert!(
            outcome
                .stderr
                .contains("llm-xtask release-tag <version> <sha>"),
            "{}",
            outcome.shown()
        );
    }
}

#[test]
fn the_release_section_and_the_taskfile_name_the_one_command_that_tags() {
    let agents = read("AGENTS.md");
    let section = agents
        .split("\n## Release\n")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .expect("AGENTS.md has a section `Release`");
    let step = section
        .split("\n3. ")
        .nth(1)
        .and_then(|rest| rest.split("\n4. ").next())
        .expect("the section has a step 3 before a step 4");
    let step = step.split_whitespace().collect::<Vec<&str>>().join(" ");
    assert!(
        step.contains("`task release-tag -- <version> <sha>`"),
        "{step}"
    );
    for workflow in REQUIRED {
        assert!(
            step.contains(&format!("`{workflow}`")),
            "{workflow}: {step}"
        );
    }
    assert!(step.contains("absent, pending or red"), "{step}");
    assert!(step.contains("merge commit"), "{step}");

    let taskfile: serde_yaml::Value =
        serde_yaml::from_str(&read("Taskfile.yml")).expect("Taskfile.yml is YAML");
    let commands: Vec<&str> = taskfile["tasks"]["release-tag"]["cmds"]
        .as_sequence()
        .expect("Taskfile.yml has a task `release-tag` with commands")
        .iter()
        .filter_map(serde_yaml::Value::as_str)
        .collect();
    assert_eq!(commands, ["cargo xtask release-tag {{.CLI_ARGS}}"]);
}
