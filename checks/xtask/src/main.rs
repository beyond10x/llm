//! The gate's test phase, one command locally and in CI: `cargo xtask test`.
//!
//! `test` builds the test binaries of every run first, outside the bound, then runs each with
//! `cargo nextest run` under `.config/nextest.toml`, which kills a test at 60 s and counts it
//! failed, and ends one run at 120 s. After each run it reads the `JUnit` report that run wrote.
//! It ends as `report` does: the ten slowest tests of all runs, slowest first, and the time the
//! runs took by their own reports, failing when that time is over 120 s. The time is read from
//! the reports, not from a clock around the commands, so a wait for a build slot is not counted.
//!
//! Before the runs, `test` checks that every `harness = false` test target answers the list
//! cargo-nextest asks each binary for, as `list-check` does, so a runner of its own that nextest
//! cannot list fails here by name, locally as in CI.
//!
//! `report <JUnit report>...` does the last part alone, over reports already written.
//! `list-check <test binary>...` runs each binary with `--list --format terse`, with and without
//! `--ignored`, and fails unless every line it writes is `<name>: test` or `<name>: benchmark`.

use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
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

/// Where cargo-nextest writes the `JUnit` report of its default profile, under the target
/// directory (`.config/nextest.toml`).
const JUNIT: &str = "nextest/default/junit.xml";

/// What cargo-nextest asks a test binary for to list its tests; it adds `--ignored` for a second
/// list.
const LIST: [&str; 3] = ["--list", "--format", "terse"];

const USAGE: &str = "usage: llm-xtask test\n       llm-xtask report <JUnit report>...\n       \
                     llm-xtask list-check <test binary>...";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let result = match arguments.split_first() {
        Some((command, [])) if command == "test" => test(),
        Some((command, reports)) if command == "report" && !reports.is_empty() => reports
            .iter()
            .map(|report| read(Path::new(report)))
            .collect::<Result<Vec<Run>, Failure>>()
            .and_then(|runs| verdict(&runs)),
        Some((command, binaries)) if command == "list-check" && !binaries.is_empty() => binaries
            .iter()
            .try_for_each(|binary| list_check(Path::new(binary))),
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
    List(String),
    Failed(Vec<String>),
    OverBound(f64),
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Command(message) | Self::Report(message) | Self::List(message) => {
                formatter.write_str(message)
            }
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
    let metadata = metadata()?;
    let report = metadata["target_directory"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| Failure::Command("cargo metadata names no target directory".to_owned()))?
        .join(JUNIT);
    let own = custom_harness_targets(&metadata)?;
    let mut checked: Vec<PathBuf> = Vec::new();
    for run in RUNS {
        for (id, path) in binaries(run)? {
            if own.contains(&id) && !checked.contains(&path) {
                list_check(&path)?;
                checked.push(path);
            }
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
        let passed = nextest(run)?;
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

/// Runs `cargo nextest run --locked` with `run`'s arguments.
fn nextest(run: &[&str]) -> Result<bool, Failure> {
    let status = Command::new("cargo")
        .args(["nextest", "run", "--locked"])
        .args(run)
        .status()
        .map_err(|error| Failure::Command(format!("cargo does not run: {error}")))?;
    Ok(status.success())
}

/// Builds the test binaries of `run` and names each, by its cargo-nextest binary id
/// (`<package>::<target>` for an integration test) and its path. Listing only the binaries runs
/// none of them, so a binary nextest could not list is still built and named here.
fn binaries(run: &[&str]) -> Result<Vec<(String, PathBuf)>, Failure> {
    let output = Command::new("cargo")
        .args([
            "nextest",
            "list",
            "--locked",
            "--list-type",
            "binaries-only",
        ])
        .args(["--message-format", "json"])
        .args(run)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| Failure::Command(format!("cargo does not run: {error}")))?;
    if !output.status.success() {
        return Err(Failure::Command(format!(
            "building the tests of `{}` failed",
            command_line(run)
        )));
    }
    let list: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| Failure::Command(format!("cargo nextest list wrote no JSON: {error}")))?;
    Ok(list["rust-binaries"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(id, binary)| {
            binary["binary-path"]
                .as_str()
                .map(|path| (id.clone(), PathBuf::from(path)))
        })
        .collect())
}

/// The workspace as `cargo metadata --no-deps` describes it.
fn metadata() -> Result<serde_json::Value, Failure> {
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
    serde_json::from_slice(&output.stdout)
        .map_err(|error| Failure::Command(format!("cargo metadata wrote no JSON: {error}")))
}

/// The cargo-nextest binary id of every `[[test]]` target with `harness = false` in a workspace
/// manifest. `cargo metadata` does not say whether a target has a harness, so each manifest is read.
fn custom_harness_targets(metadata: &serde_json::Value) -> Result<Vec<String>, Failure> {
    let mut targets = Vec::new();
    for package in metadata["packages"].as_array().into_iter().flatten() {
        let (Some(package), Some(manifest)) =
            (package["name"].as_str(), package["manifest_path"].as_str())
        else {
            return Err(Failure::Command(
                "cargo metadata names a package without a name or manifest".to_owned(),
            ));
        };
        let text = fs::read_to_string(manifest)
            .map_err(|error| Failure::Command(format!("{manifest}: {error}")))?;
        let table: toml::Table = toml::from_str(&text)
            .map_err(|error| Failure::Command(format!("{manifest}: {error}")))?;
        let names = table
            .get("test")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
            .filter(|test| test.get("harness").and_then(toml::Value::as_bool) == Some(false))
            .filter_map(|test| test.get("name").and_then(toml::Value::as_str));
        targets.extend(names.map(|name| format!("{package}::{name}")));
    }
    Ok(targets)
}

/// Runs `binary` as cargo-nextest does to list its tests, with and without `--ignored`, and fails
/// unless every line it writes is `<name>: test` or `<name>: benchmark`, the only lines nextest
/// reads. libtest writes them; a `harness = false` runner has to answer the list itself.
fn list_check(binary: &Path) -> Result<(), Failure> {
    for ignored in [false, true] {
        let mut arguments = LIST.to_vec();
        if ignored {
            arguments.push("--ignored");
        }
        let shown = format!("{} {}", binary.display(), arguments.join(" "));
        let output = Command::new(binary)
            .args(&arguments)
            .output()
            .map_err(|error| Failure::List(format!("`{shown}` does not run: {error}")))?;
        if !output.status.success() {
            return Err(Failure::List(format!("`{shown}` ended {}", output.status)));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(line) = stdout
            .lines()
            .find(|line| !line.ends_with(": test") && !line.ends_with(": benchmark"))
        {
            return Err(Failure::List(format!(
                "`{shown}` wrote {line:?}: cargo-nextest lists a test binary only when every line \
                 is `<name>: test` or `<name>: benchmark`, so a `harness = false` runner answers \
                 libtest's `--list --format terse` and `--ignored` itself"
            )));
        }
    }
    Ok(())
}

/// Reads one `JUnit` report cargo-nextest wrote.
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
        if separated && quoted_before.is_multiple_of(2) {
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
