//! The test phase is one step and one task, the same locally and in CI: `cargo xtask test`, which
//! runs cargo-nextest under `.config/nextest.toml`.

use super::{read, workflow};
use serde_yaml::Value;

const TEST_PHASE: &str = "cargo xtask test";

fn runs(steps: &Value) -> Vec<(String, String)> {
    steps
        .as_sequence()
        .expect("steps")
        .iter()
        .filter_map(|step| {
            Some((
                step["name"].as_str()?.to_owned(),
                step["run"].as_str()?.to_owned(),
            ))
        })
        .collect()
}

fn commands(task: &Value) -> Vec<String> {
    task["cmds"]
        .as_sequence()
        .expect("cmds")
        .iter()
        .map(|command| match command {
            Value::String(line) => line.clone(),
            other => format!("task: {}", other["task"].as_str().expect("a task")),
        })
        .collect()
}

#[test]
fn the_test_phase_is_its_own_step_and_task_and_runs_alike_locally_and_in_ci() {
    let workflow = workflow();
    let steps = runs(&workflow["jobs"]["libraries-and-plan"]["steps"]);
    let tests: Vec<&(String, String)> = steps
        .iter()
        .filter(|(_, run)| {
            run.contains("cargo test") || run.contains("cargo nextest") || run.contains(TEST_PHASE)
        })
        .collect();
    assert_eq!(tests.len(), 1, "{tests:?}");
    assert_eq!(tests[0].0, "Rust tests");
    assert_eq!(tests[0].1.trim(), TEST_PHASE);

    let taskfile: Value = serde_yaml::from_str(&read("Taskfile.yml")).expect("Taskfile.yml");
    assert_eq!(commands(&taskfile["tasks"]["test"]), [TEST_PHASE]);
    let check = commands(&taskfile["tasks"]["check"]);
    assert!(check.contains(&"task: test".to_owned()), "{check:?}");
    for (name, task) in taskfile["tasks"].as_mapping().expect("tasks") {
        if name.as_str() == Some("test") {
            continue;
        }
        for command in commands(task) {
            assert!(
                !command.contains("cargo test") && !command.contains("cargo nextest"),
                "task {name:?} runs tests outside the test phase: {command}"
            );
        }
    }
}

#[test]
fn a_test_over_60_s_is_killed_and_fails_and_a_run_ends_at_120_s() {
    let config: toml::Table = toml::from_str(&read(".config/nextest.toml")).expect("nextest.toml");
    let profile = &config["profile"]["default"];
    let slow = &profile["slow-timeout"];
    assert_eq!(slow["period"].as_str(), Some("60s"));
    assert_eq!(slow["terminate-after"].as_integer(), Some(1));
    assert_eq!(profile["global-timeout"].as_str(), Some("120s"));
    assert_eq!(profile["junit"]["path"].as_str(), Some("junit.xml"));
}

#[test]
fn ci_installs_cargo_nextest_pinned_by_version_and_digest() {
    let workflow = workflow();
    let steps = workflow["jobs"]["libraries-and-plan"]["steps"]
        .as_sequence()
        .expect("steps");
    let install = steps
        .iter()
        .find(|step| step["name"].as_str() == Some("Install cargo-nextest"))
        .expect("a step installs cargo-nextest");
    let version = install["env"]["NEXTEST_VERSION"]
        .as_str()
        .expect("a version");
    assert_eq!(version.split('.').count(), 3, "{version}");
    assert!(
        version.split('.').all(|part| part.parse::<u32>().is_ok()),
        "{version}"
    );
    let digest = install["env"]["NEXTEST_SHA256"].as_str().expect("a digest");
    assert!(
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "{digest}"
    );
    let run = install["run"].as_str().expect("a script");
    assert!(run.contains("sha256sum --check --strict"), "{run}");
    assert!(
        run.contains("--version | grep -F \"cargo-nextest $NEXTEST_VERSION \""),
        "{run}"
    );
}
