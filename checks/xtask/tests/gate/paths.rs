//! A pull request whose changed files are all planning or prose passes every Gate job, under its
//! own name, without building or testing; any other change runs the whole gate. The path list is
//! the one `case` of the step "Planning or prose only" in `.github/workflows/gate.yml`, and these
//! cases run that step's own script with `sh`, as the runner does.

use super::workflow;
use std::{fs, process::Command};

const DECIDING_JOB: &str = "changes";
const DECIDING_STEP: &str = "Planning or prose only";
const GATED: &str = "needs.changes.outputs.decision != 'skip'";

fn decision_script() -> String {
    let workflow = workflow();
    let steps = workflow["jobs"][DECIDING_JOB]["steps"]
        .as_sequence()
        .unwrap_or_else(|| panic!("gate.yml has a job `{DECIDING_JOB}` with steps"));
    steps
        .iter()
        .find(|step| step["name"].as_str() == Some(DECIDING_STEP))
        .and_then(|step| step["run"].as_str())
        .unwrap_or_else(|| {
            panic!("job `{DECIDING_JOB}` has a step `{DECIDING_STEP}` with a script")
        })
        .to_owned()
}

/// Runs the deciding step over `changed` (no list at all when `None`) and returns what it wrote
/// to `GITHUB_OUTPUT` as `decision`.
fn decide(script: &str, changed: Option<&[&str]>) -> String {
    let runner = tempfile::tempdir().expect("a temporary directory");
    if let Some(paths) = changed {
        let mut list = paths.join("\n");
        list.push('\n');
        fs::write(runner.path().join("changed.txt"), list).expect("the list is written");
    }
    let output = runner.path().join("github-output");
    fs::write(&output, "").expect("the output file is written");
    let run = Command::new("sh")
        .arg("-c")
        .arg(script)
        .env("RUNNER_TEMP", runner.path())
        .env("GITHUB_OUTPUT", &output)
        .output()
        .expect("sh runs");
    assert!(
        run.status.success(),
        "{changed:?}: the step exits {:?}: {}",
        run.status.code(),
        String::from_utf8_lossy(&run.stderr)
    );
    let written = fs::read_to_string(&output).expect("the output file is read");
    let decisions: Vec<&str> = written
        .lines()
        .filter_map(|line| line.strip_prefix("decision="))
        .collect();
    assert_eq!(decisions.len(), 1, "{changed:?}: {written:?}");
    decisions[0].to_owned()
}

#[test]
fn a_change_of_only_planning_or_prose_files_skips_the_gate() {
    let script = decision_script();
    for (case, changed) in [
        (
            "one planning file",
            &[".engineering/planning/story/x.md"][..],
        ),
        (
            "an evidence record of the store, which is not Markdown",
            &[".engineering/evidence/story/x/1.json"],
        ),
        (
            "documents no gate step reads",
            &[
                "CHANGELOG.md",
                "AGENTS.md",
                "README.md",
                "docs/design.md",
                "docs/plan/wave-3.md",
                "crates/llm-bedrock/tests/fixtures/README.md",
            ],
        ),
        (
            "planning and prose together",
            &[".engineering/planning/story/x.md", "docs/design.md"],
        ),
    ] {
        assert_eq!(decide(&script, Some(changed)), "skip", "{case}");
    }
}

#[test]
fn any_other_change_runs_the_whole_gate() {
    let script = decision_script();
    for (case, changed) in [
        ("a source file", &["crates/llm-core/src/lib.rs"][..]),
        (
            "planning, prose and one source file",
            &[
                ".engineering/planning/story/x.md",
                "docs/design.md",
                "crates/llm-core/src/lib.rs",
            ],
        ),
        (
            "a page of the built site, which Documentation validation builds",
            &["website/docs/index.md"],
        ),
        (
            "the status source, which `llm-docs generate --check` reads",
            &["docs/implementation-status.md"],
        ),
        ("the workflow itself", &[".github/workflows/gate.yml"]),
        ("the manifest", &["Cargo.toml"]),
        ("a scenario", &["contracts/calls/scenarios/x.yaml"]),
        ("a path git quoted", &["\"docs/pl\\303\\244ne.md\""]),
        ("an empty list", &[]),
    ] {
        assert_eq!(decide(&script, Some(changed)), "run", "{case}");
    }
    assert_eq!(decide(&script, None), "run", "no list: a push to main");
}

/// A file moved out of the source into `.engineering/` is a change of both its paths: the list
/// names renames as a deletion and an addition, so the source path is seen.
#[test]
fn the_changed_files_of_a_pull_request_name_both_paths_of_a_rename() {
    let workflow = workflow();
    let steps = workflow["jobs"][DECIDING_JOB]["steps"]
        .as_sequence()
        .expect("steps");
    let listing = steps
        .iter()
        .find(|step| step["name"].as_str() == Some("Changed files"))
        .expect("a step lists the changed files");
    assert_eq!(
        listing["if"].as_str(),
        Some("github.event_name == 'pull_request'")
    );
    assert_eq!(
        listing["run"].as_str().map(str::trim),
        Some("git diff --no-renames --name-only HEAD^1 HEAD > \"$RUNNER_TEMP/changed.txt\"")
    );
}

/// Every step of every other job runs only when the decision is not `skip`, and the jobs run
/// even when the deciding job failed, so a failure there runs the whole gate rather than
/// skipping it.
#[test]
fn every_step_of_every_gate_job_waits_on_the_decision() {
    let workflow = workflow();
    let jobs = workflow["jobs"].as_mapping().expect("gate.yml has jobs");
    let mut gated = 0;
    for (name, job) in jobs {
        let name = name.as_str().expect("a job name");
        if name == DECIDING_JOB {
            continue;
        }
        assert_eq!(job["needs"].as_str(), Some(DECIDING_JOB), "{name}");
        assert_eq!(job["if"].as_str(), Some("${{ !cancelled() }}"), "{name}");
        for step in job["steps"].as_sequence().expect("steps") {
            let condition = step["if"].as_str().unwrap_or_default();
            assert!(
                condition.contains(GATED),
                "{name}: step {:?} runs on a planning-only pull request: if {condition:?}",
                step["name"].as_str().or_else(|| step["uses"].as_str())
            );
            gated += 1;
        }
    }
    assert!(gated > 10, "{gated} steps");
}
