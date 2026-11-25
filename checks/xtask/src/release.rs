//! The release tag, made only on a commit whose required workflow runs ended `success`:
//! `cargo xtask release-tag <version> <sha>` (`task release-tag -- <version> <sha>`).
//!
//! It asks `b10x-gates gh` for the workflow runs of `<sha>`. `b10x-gates` reads its policy file
//! from the environment variable `B10X_GATES_POLICY`; this repository names the variable and no
//! path. The tag is made when each of `Gate`, `Shared source gates` and `Documentation
//! validation` has at least one run on `<sha>` and every such run is `completed` with the
//! conclusion `success`. Otherwise the command names each required workflow that is absent and
//! each of its runs that is pending (any other status) or red (any other conclusion), calls
//! `b10x-gates bot` not once and exits 1. It refuses the same way when the runs cannot be read.
//!
//! A run of any other workflow on the commit is printed and does not block: documentation
//! publication runs after a source release and its failure does not invalidate one.
//!
//! When the rule holds it runs `b10x-gates bot -- tag -a <version> -m "LLM <version>" <sha>` and
//! then `b10x-gates bot --repository beyond10x/llm -- push origin refs/tags/<version>`. A tag
//! call that fails stops before the push, and either failure exits 1.

use std::process::{Command, ExitCode, Stdio};

/// The command that reads the runs and, as the bot, makes and pushes the tag.
const GATES: &str = "b10x-gates";

const REPOSITORY: &str = "beyond10x/llm";

/// The workflows a release commit must have passed, by their exact names.
const REQUIRED: [&str; 3] = ["Gate", "Shared source gates", "Documentation validation"];

/// How many runs the list is asked for. A list this long may be cut, so it shows no rule.
const LIMIT: usize = 100;

/// The members asked of each run, each a string in the answer.
const MEMBERS: [&str; 5] = ["name", "status", "conclusion", "event", "headSha"];

/// One workflow run as the list names it.
struct Run {
    name: String,
    status: String,
    conclusion: String,
    head: String,
}

impl Run {
    fn pending(&self) -> bool {
        self.status != "completed"
    }

    /// Whether a run that has ended ended in anything but exactly `success`.
    fn red(&self) -> bool {
        self.conclusion != "success"
    }

    /// The status of a run that has not ended, the conclusion of one that has.
    fn state(&self) -> &str {
        match (
            self.pending(),
            self.status.as_str(),
            self.conclusion.as_str(),
        ) {
            (true, "", _) => "no status",
            (true, status, _) => status,
            (false, _, "") => "no conclusion",
            (false, _, conclusion) => conclusion,
        }
    }
}

/// Makes and pushes the tag `version` on `sha`, or says why not on stderr.
pub fn tag(version: &str, sha: &str) -> ExitCode {
    match release(version, sha) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("release-tag: {message}");
            ExitCode::FAILURE
        }
    }
}

fn release(version: &str, sha: &str) -> Result<(), String> {
    let groups_of_digits = version
        .split('.')
        .all(|group| !group.is_empty() && group.bytes().all(|byte| byte.is_ascii_digit()));
    if !groups_of_digits {
        return Err(format!(
            "refused: the version {version:?} is not digits and dots, as `0.31.0` is"
        ));
    }
    let lower_case_hexadecimal = sha
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if sha.len() != 40 || !lower_case_hexadecimal {
        return Err(format!(
            "refused: the commit {sha:?} is not 40 lower-case hexadecimal characters"
        ));
    }
    let runs = runs(sha).map_err(|reason| {
        format!("refused: the workflow runs of {sha} were not read: {reason}; no tag was made")
    })?;
    let blocking = blocking(&runs, sha);
    if !blocking.is_empty() {
        return Err(format!(
            "refused: no tag `{version}` on {sha}, whose required runs did not all end \
             `success`:\n  {}",
            blocking.join("\n  ")
        ));
    }
    bot(&[
        "--",
        "tag",
        "-a",
        version,
        "-m",
        &format!("LLM {version}"),
        sha,
    ])
    .map_err(|reason| format!("{reason}; the tag `{version}` was not pushed"))?;
    let reference = format!("refs/tags/{version}");
    bot(&[
        "--repository",
        REPOSITORY,
        "--",
        "push",
        "origin",
        &reference,
    ])
    .map_err(|reason| {
        format!("{reason}; the tag `{version}` was made here and is not on the remote")
    })?;
    println!("release-tag: tag `{version}` on {sha} is pushed");
    Ok(())
}

/// The workflow runs `b10x-gates gh` lists for `sha`, or why they cannot be relied on.
fn runs(sha: &str) -> Result<Vec<Run>, String> {
    let limit = LIMIT.to_string();
    let members = MEMBERS.join(",");
    let output = Command::new(GATES)
        .args(["gh", "--", "run", "list", "--repo", REPOSITORY])
        .args(["--commit", sha, "--limit", &limit, "--json", &members])
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("`{GATES}` does not run: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "`{GATES} gh -- run list` ended {} (it reads its policy file from `B10X_GATES_POLICY`)",
            output.status
        ));
    }
    let list: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("the run list is not JSON: {error}"))?;
    let entries = list
        .as_array()
        .ok_or_else(|| "the run list is not a JSON array".to_owned())?;
    if entries.len() >= LIMIT {
        return Err(format!(
            "the run list holds {} runs and may be cut at its limit of {LIMIT}",
            entries.len()
        ));
    }
    entries.iter().map(run).collect()
}

/// Reads one entry of the list, which names every member as a string.
fn run(entry: &serde_json::Value) -> Result<Run, String> {
    if let Some(missing) = MEMBERS.iter().find(|member| !entry[**member].is_string()) {
        return Err(format!("a run without a string `{missing}`: {entry}"));
    }
    let member = |name: &str| entry[name].as_str().unwrap_or_default().to_owned();
    Ok(Run {
        name: member("name"),
        status: member("status"),
        conclusion: member("conclusion"),
        head: member("headSha"),
    })
}

/// Prints each run on `sha` and returns what keeps the tag from it: each required workflow
/// without a run on `sha`, and each of their runs on it that is pending or red.
fn blocking(runs: &[Run], sha: &str) -> Vec<String> {
    let mut blocking = Vec::new();
    for run in runs {
        if run.head != sha {
            println!(
                "release-tag: not counted: {}: {} on {}, another commit",
                run.name,
                run.state(),
                run.head
            );
        } else if !REQUIRED.contains(&run.name.as_str()) {
            println!("release-tag: not required: {}: {}", run.name, run.state());
        } else {
            println!("release-tag: {}: {}", run.name, run.state());
            if run.pending() {
                blocking.push(format!("{}: pending ({})", run.name, run.state()));
            } else if run.red() {
                blocking.push(format!("{}: red ({})", run.name, run.state()));
            }
        }
    }
    for required in REQUIRED {
        if !runs
            .iter()
            .any(|run| run.head == sha && run.name == required)
        {
            blocking.push(format!("{required}: absent"));
        }
    }
    blocking
}

/// Runs `b10x-gates bot` with `arguments`.
fn bot(arguments: &[&str]) -> Result<(), String> {
    let shown = format!("{GATES} bot {}", arguments.join(" "));
    let status = Command::new(GATES)
        .arg("bot")
        .args(arguments)
        .status()
        .map_err(|error| format!("`{shown}` does not run: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`{shown}` ended {status}"))
    }
}
