//! The gate's test phase, one command locally and in CI: `cargo xtask test`.
//!
//! `test` builds the test binaries of every run first, outside the bound, then runs each with
//! `cargo nextest run` under `.config/nextest.toml`, which kills a test at 60 s and counts it
//! failed, and ends one run at 120 s. After each run it reads the JUnit report that run wrote.
//! It ends as `report` does: the ten slowest tests of all runs, slowest first, and the time the
//! runs took by their own reports, failing when that time is over 120 s. The time is read from
//! the reports, not from a clock around the commands, so a wait for a build slot is not counted.
//!
//! `report <JUnit report>...` does the last part alone, over reports already written.

use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

/// What every run of the test phase may take together, in seconds.
const BOUND_SECONDS: f64 = 120.0;

/// How many of the slowest tests the phase prints.
const SLOWEST: usize = 10;

/// The runs of the test phase, each the arguments of one `cargo nextest run --locked`. Workspace
/// feature unification turns `process` on for `llm-models` in the workspace run, so it runs
/// once more alone; `llm-credentials`, `llm-cost` and `llm-calls` run with all their features.
const RUNS: [&[&str]; 5] = [
    &["--workspace"],
    &["-p", "llm-credentials", "--all-features"],
    &["-p", "llm-cost", "--all-features"],
    &["-p", "llm-calls", "--all-features"],
    &["-p", "llm-models"],
];

/// Where cargo-nextest writes the JUnit report of its default profile, under the target
/// directory (`.config/nextest.toml`).
const JUNIT: &str = "nextest/default/junit.xml";

const USAGE: &str = "usage: llm-xtask test\n       llm-xtask report <JUnit report>...";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let result = match arguments.split_first() {
        Some((command, [])) if command == "test" => test(),
        Some((command, reports)) if command == "report" && !reports.is_empty() => reports
            .iter()
            .map(|report| read(Path::new(report)))
            .collect::<Result<Vec<Run>, Failure>>()
            .and_then(|runs| verdict(&runs)),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            eprintln!("tests: {failure}");
            ExitCode::FAILURE
        }
    }
}

/// Why the phase failed.
enum Failure {
    Command(String),
    Report(String),
    Failed(Vec<String>),
    OverBound(f64),
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Command(message) | Self::Report(message) => formatter.write_str(message),
            Self::Failed(runs) => write!(formatter, "failed: {}", runs.join("; ")),
            Self::OverBound(seconds) => write!(
                formatter,
                "the runs took {seconds:.1} s, longer than their bound of {BOUND_SECONDS:.0} s"
            ),
        }
    }
}

/// One test as a report names it.
struct Case {
    seconds: f64,
    suite: String,
    name: String,
}

/// What one report says: the run's own time and each test's.
struct Run {
    seconds: f64,
    cases: Vec<Case>,
}

fn test() -> Result<(), Failure> {
    let report = target_directory()?.join(JUNIT);
    for run in RUNS {
        if !nextest(run, true)? {
            return Err(Failure::Command(format!(
                "building the tests of `{}` failed",
                command_line(run)
            )));
        }
    }
    let mut runs = Vec::new();
    let mut failed = Vec::new();
    for run in RUNS {
        match fs::remove_file(&report) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                return Err(Failure::Report(format!("{}: {error}", report.display())));
            }
            _ => {}
        }
        let passed = nextest(run, false)?;
        let read = read(&report)?;
        println!(
            "tests: `{}`: {} tests in {:.1} s, {}",
            command_line(run),
            read.cases.len(),
            read.seconds,
            if passed { "passed" } else { "failed" }
        );
        if !passed {
            failed.push(command_line(run));
        }
        runs.push(read);
    }
    let bound = verdict(&runs);
    if failed.is_empty() {
        bound
    } else {
        Err(Failure::Failed(failed))
    }
}

fn command_line(run: &[&str]) -> String {
    format!("cargo nextest run --locked {}", run.join(" "))
}

/// Runs `cargo nextest run --locked` with `run`'s arguments; with `build_only`, only builds.
fn nextest(run: &[&str], build_only: bool) -> Result<bool, Failure> {
    let mut command = Command::new("cargo");
    command.args(["nextest", "run", "--locked"]);
    if build_only {
        command.arg("--no-run");
    }
    let status = command
        .args(run)
        .status()
        .map_err(|error| Failure::Command(format!("cargo does not run: {error}")))?;
    Ok(status.success())
}

/// The target directory cargo builds into, as `cargo metadata` names it.
fn target_directory() -> Result<PathBuf, Failure> {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
        .output()
        .map_err(|error| Failure::Command(format!("cargo metadata does not run: {error}")))?;
    if !output.status.success() {
        return Err(Failure::Command(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| Failure::Command(format!("cargo metadata wrote no JSON: {error}")))?;
    metadata["target_directory"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| Failure::Command("cargo metadata names no target directory".to_owned()))
}

/// Reads one JUnit report cargo-nextest wrote.
fn read(report: &Path) -> Result<Run, Failure> {
    let place = report.display();
    let xml =
        fs::read_to_string(report).map_err(|error| Failure::Report(format!("{place}: {error}")))?;
    let seconds = tags(&xml, "testsuites")
        .first()
        .and_then(|tag| attribute(tag, "time"))
        .and_then(|time| time.parse::<f64>().ok())
        .ok_or_else(|| Failure::Report(format!("{place}: no run time on <testsuites>")))?;
    let mut cases = Vec::new();
    for tag in tags(&xml, "testcase") {
        let time = attribute(tag, "time").and_then(|time| time.parse::<f64>().ok());
        let (Some(seconds), Some(suite), Some(name)) =
            (time, attribute(tag, "classname"), attribute(tag, "name"))
        else {
            return Err(Failure::Report(format!(
                "{place}: a <testcase> without a time, classname or name"
            )));
        };
        cases.push(Case {
            seconds,
            suite: unescape(suite),
            name: unescape(name),
        });
    }
    Ok(Run { seconds, cases })
}

/// The attribute text of every start tag `<name ...>` in `xml`.
fn tags<'a>(xml: &'a str, name: &str) -> Vec<&'a str> {
    let open = format!("<{name} ");
    xml.match_indices(open.as_str())
        .filter_map(|(at, _)| {
            let rest = &xml[at + open.len()..];
            rest.find('>').map(|end| &rest[..end])
        })
        .collect()
}

/// The value of attribute `name` in a tag's attribute text, as written. A value holds no `"`
/// (XML writes it `&quot;`), so the first `name="` after white space starts the attribute.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let mut from = 0;
    while let Some(found) = tag[from..].find(&key) {
        let at = from + found;
        let start = at + key.len();
        let quoted_before = tag[..at].matches('"').count();
        let separated = at == 0 || tag[..at].ends_with(char::is_whitespace);
        if separated && quoted_before % 2 == 0 {
            return tag[start..].find('"').map(|end| &tag[start..start + end]);
        }
        from = start;
    }
    None
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Prints the ten slowest tests of `runs` and the time the runs took, and fails over the bound.
fn verdict(runs: &[Run]) -> Result<(), Failure> {
    let mut cases: Vec<&Case> = runs.iter().flat_map(|run| &run.cases).collect();
    cases.sort_by(|a, b| b.seconds.total_cmp(&a.seconds));
    println!("tests: the {SLOWEST} slowest tests");
    for case in cases.iter().take(SLOWEST) {
        println!("{:>10.3} s  {} {}", case.seconds, case.suite, case.name);
    }
    let seconds: f64 = runs.iter().map(|run| run.seconds).sum();
    println!("tests: the runs took {seconds:.1} s of their bound of {BOUND_SECONDS:.0} s");
    if seconds > BOUND_SECONDS {
        return Err(Failure::OverBound(seconds));
    }
    Ok(())
}
