//! The ESS pin has one source, the `ESS_VERSION` and `ESS_SHA256` of the "Install ess" step in
//! `gate.yml`. The conformance runner reads it there to find or fetch the `ess` it runs
//! (`checks/conformance/src/ess.rs`); the manifest, the lockfile and the documents are held equal
//! to it here, and no task and no workflow step runs an `ess` the runner did not resolve.

use super::{read, repository, workflow};
use serde_yaml::Value;

/// Where the ESS crates and the release assets come from.
const SOURCE: &str = "https://github.com/beyond10x/ess.git";

/// The step of `gate.yml` that puts the pinned release on `PATH`.
const INSTALL: &str = "Install ess";

/// How `task check` and CI validate the specification: through the runner, which resolves the pin.
const VALIDATE: &str = "cargo run --locked -p llm-conformance -- ess specify validate --path spec";

/// The pinned version and digest as the "Install ess" step names them.
fn pin() -> (String, String) {
    let workflow = workflow();
    let install = workflow["jobs"]["libraries-and-plan"]["steps"]
        .as_sequence()
        .expect("steps")
        .iter()
        .find(|step| step["name"].as_str() == Some(INSTALL))
        .expect("a step installs ess")
        .clone();
    (
        install["env"]["ESS_VERSION"]
            .as_str()
            .expect("a version")
            .to_owned(),
        install["env"]["ESS_SHA256"]
            .as_str()
            .expect("a digest")
            .to_owned(),
    )
}

/// A word as a reader takes it: without the punctuation and the code marks around it.
fn bare(word: &str) -> &str {
    word.trim_matches(|character: char| "`,;:.()[]*\"'".contains(character))
}

/// Whether `word` names ESS: `ess`, `ESS`, in code marks or as the text of a link.
fn names_ess(word: &str) -> bool {
    let text = word
        .strip_prefix('[')
        .and_then(|link| link.split_once("](").map(|(text, _)| text))
        .unwrap_or(word);
    bare(text).eq_ignore_ascii_case("ess")
}

/// The words that may stand between the name and its version, as in "ESS version 0.1.0".
const BETWEEN: [&str; 3] = ["version", "release", "tag"];

/// Whether `word` says nothing of its own between a name and its version: a whole remark in
/// parentheses, as `([GitHub](...))` is, the bar between two cells of a table, or one of
/// [`BETWEEN`].
fn passed_over(word: &str) -> bool {
    word == "|"
        || (word.starts_with('(') && word.trim_end_matches([',', '.', ';', ':']).ends_with(')'))
        || BETWEEN.contains(&bare(word).to_ascii_lowercase().as_str())
}

/// The version `word` is, if it is one: it starts with a digit, after a `v` or not, and holds a
/// dot. Two numbers are a version too, so that `0.1` beside a pin of `0.1.0` is another version.
fn version_of(word: &str) -> Option<&str> {
    let text = bare(word);
    let text = text.strip_prefix(['v', 'V']).unwrap_or(text);
    (text.starts_with(|first: char| first.is_ascii_digit()) && text.contains('.')).then_some(text)
}

/// The version `word` carries right behind `ess-`, as the name of an asset does:
/// `ess-0.1.0-x86_64-unknown-linux-gnu.tar.gz` gives `0.1.0`. `ess-conformance` gives none.
fn joined_version(word: &str) -> Option<&str> {
    let lower = word.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find("ess-") {
        let at = from + found;
        from = at + "ess-".len();
        let starts_a_name = word[..at]
            .chars()
            .next_back()
            .is_none_or(|before| !before.is_ascii_alphanumeric());
        let rest = &word[from..];
        let length = rest
            .find(|character: char| !character.is_ascii_digit() && character != '.')
            .unwrap_or(rest.len());
        let version = rest[..length].trim_end_matches('.');
        if starts_a_name
            && version.starts_with(|first: char| first.is_ascii_digit())
            && version.contains('.')
        {
            return Some(version);
        }
    }
    None
}

/// Every version `text` gives for ESS, so that a reader of the document cannot be told another
/// version than the pin in any of these wordings:
///
/// - a version word ([`version_of`]) where the word before it names ESS: `ESS 0.1.0`,
///   `` `ess` 0.1.0 ``, `ESS v0.1.0`, `ESS 0.1`;
/// - the same with words between the two that are passed over ([`passed_over`]):
///   `[ESS](...) ([GitHub](...)) 0.1.0`, `| ESS | 0.1.0 |`, `ESS version 0.1.0`;
/// - a version joined to the name ([`joined_version`]): `ess-0.1.0`.
///
/// It errs on the side of reading too much: a version that follows the name by accident is read
/// as one of ESS and fails the test, which a rewording mends.
fn ess_versions(text: &str) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut versions = Vec::new();
    for (at, word) in words.iter().enumerate() {
        if let Some(version) = joined_version(word) {
            versions.push(version.to_owned());
        }
        let Some(version) = version_of(word) else {
            continue;
        };
        let subject = words[..at].iter().rev().find(|word| !passed_over(word));
        if subject.is_some_and(|word| names_ess(word)) {
            versions.push(version.to_owned());
        }
    }
    versions
}

#[test]
fn ci_installs_ess_pinned_by_version_and_digest() {
    let (version, digest) = pin();
    assert_eq!(version.split('.').count(), 3, "{version}");
    assert!(
        version.split('.').all(|part| part.parse::<u32>().is_ok()),
        "{version}"
    );
    assert!(
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "{digest}"
    );
    let text = read(".github/workflows/gate.yml");
    for name in ["ESS_VERSION:", "ESS_SHA256:"] {
        assert_eq!(text.matches(name).count(), 1, "{name} is set once");
    }
    assert!(
        text.contains("grep -Fx \"$ESS_SHA256  $asset\" SHA256SUMS"),
        "the release's digest line equals the pinned digest"
    );
    assert!(
        text.contains("grep -F \"  $asset\" SHA256SUMS | sha256sum --check --strict"),
        "the downloaded file has that digest"
    );
}

#[test]
fn the_manifest_and_the_lockfile_are_on_the_version_ci_installs() {
    let (version, _) = pin();
    let manifest: toml::Table =
        toml::from_str(&read("checks/conformance/Cargo.toml")).expect("the manifest is TOML");
    for name in ["ess-conformance", "ess-primitives"] {
        let dependency = manifest["dependencies"][name]
            .as_table()
            .unwrap_or_else(|| panic!("{name} is a table"));
        assert_eq!(dependency["git"].as_str(), Some(SOURCE), "{name}");
        assert_eq!(
            dependency["tag"].as_str(),
            Some(version.as_str()),
            "{name} is on the tag of the version gate.yml installs"
        );
        assert_eq!(dependency.len(), 2, "{name} has a git and a tag only");
    }

    let lock: toml::Table = toml::from_str(&read("Cargo.lock")).expect("the lockfile is TOML");
    let tagged = format!("git+{SOURCE}?tag={version}#");
    let mut names = Vec::new();
    for package in lock["package"].as_array().expect("packages") {
        let name = package["name"].as_str().expect("a name");
        let source = package.get("source").and_then(toml::Value::as_str);
        if !name.starts_with("ess-") && !source.is_some_and(|source| source.contains(SOURCE)) {
            continue;
        }
        assert_eq!(
            package["version"].as_str(),
            Some(version.as_str()),
            "{name} in Cargo.lock"
        );
        assert!(
            source.is_some_and(|source| source.starts_with(&tagged)),
            "{name} in Cargo.lock comes from {source:?}, not from tag {version}"
        );
        names.push(name);
    }
    for name in ["ess-conformance", "ess-primitives"] {
        assert!(names.contains(&name), "{name} is locked: {names:?}");
    }
}

#[test]
fn the_documents_name_the_version_ci_installs() {
    let (version, _) = pin();
    for document in ["AGENTS.md", "README.md", "website/docs/getting-started.md"] {
        let named = ess_versions(&read(document));
        assert!(!named.is_empty(), "{document} names an ESS version");
        for found in named {
            assert_eq!(found, version, "{document} names ESS {found}");
        }
    }
}

#[test]
fn a_version_is_read_as_an_ess_version_by_the_word_before_it() {
    let text = "CI installs `ess` 0.1.0 and `aep` 0.2.0. [ESS](https://e/ess/) \
                ([GitHub](https://g/ess)) 0.3.0-rc1 and ESS 0.4.01, (ess 0.5.0). Rust 1.98.0.\n\
                | [ESS](https://e/ess/) ([GitHub](https://g/ess)) | 0.6.0 in CI | stages |\n\
                | [AEP](https://e/aep/) | 0.7.0 in CI | the planning stage |";
    assert_eq!(
        ess_versions(text),
        ["0.1.0", "0.3.0-rc1", "0.4.01", "0.5.0", "0.6.0"]
    );
}

/// Each other wording a document may give the version in is read as a version of ESS.
#[test]
fn a_version_worded_another_way_is_read_as_an_ess_version_too() {
    for (text, versions) in [
        ("it needs ESS version 0.87.0 here", &["0.87.0"][..]),
        ("the `ess` release 0.87.0, then", &["0.87.0"]),
        ("ESS v0.87.0.", &["0.87.0"]),
        ("on ESS V0.87.0", &["0.87.0"]),
        ("ESS 0.87 is enough", &["0.87"]),
        ("unpack ess-0.87.0 first", &["0.87.0"]),
        ("`ess-0.87.0-x86_64-unknown-linux-gnu.tar.gz`", &["0.87.0"]),
        ("target/ESS-0.87.0/ess and ess-0.88.", &["0.87.0", "0.88"]),
        (
            "`ess-conformance` 0.87.0, ess-<version>, process-0.87.0 and Rust version 1.98.0",
            &[],
        ),
    ] {
        assert_eq!(ess_versions(text), versions, "{text}");
    }
}

/// The runner's command up to the `ess` it is handed: the one way a task or a workflow step may
/// name `ess`.
const RUNNER: [&str; 6] = ["cargo", "run", "--locked", "-p", "llm-conformance", "--"];

/// How many words of a script name an `ess` the runner did not resolve.
///
/// A script is read as text, not as a shell reads it. A word is bare when it holds `ess` or
/// `ess.exe`, in either case, as a whole name: what stands around the name in the word is no
/// letter, no digit and none of `_`, `.` and `-`. So `ess`, `"ess"`, `'ess'`, `$(ess`, `./ess`,
/// `/usr/bin/ess` and `X=ess` are bare wherever they stand in a line, after `sh -c`, `env --`,
/// `command`, `exec` or `time` as well, and `ess-conformance`, `ess_pin.rs` and `ess-$V.tar.gz`
/// are not. The one word `ess` that is not bare is the argument right after [`RUNNER`].
///
/// It errs on the side of counting: a harmless mention of `ess` in a script (`echo ess`, a path
/// through `target/ess/`) is counted and has to be worded another way. What it cannot count is a
/// name that is not written in the script: one put together when the script runs (`e"s"s`,
/// `$(printf ess)`, a variable set elsewhere) and one a program the script calls starts.
fn bare_ess(script: &str) -> usize {
    let words: Vec<&str> = script.split_whitespace().collect();
    words
        .iter()
        .enumerate()
        .filter(|&(at, word)| {
            let named = word
                .split(|character: char| {
                    !(character.is_ascii_alphanumeric() || "_.-".contains(character))
                })
                .any(|name| {
                    name.eq_ignore_ascii_case("ess") || name.eq_ignore_ascii_case("ess.exe")
                });
            let handed =
                *word == "ess" && at >= RUNNER.len() && words[at - RUNNER.len()..at] == RUNNER;
            named && !handed
        })
        .count()
}

#[test]
fn an_ess_is_bare_unless_the_runner_is_handed_it_as_an_argument() {
    for (script, bare) in [
        (
            "cargo run --locked -p llm-conformance -- ess specify validate",
            0,
        ),
        (
            "asset=\"ess-$V.tar.gz\"\ncargo test -p ess-conformance\nls tests/gate/ess_pin.rs",
            0,
        ),
        (
            "git clone https://github.com/beyond10x/ess.git \"$dir/ess-src\"",
            0,
        ),
        ("ess specify validate --path spec", 1),
        (
            "set -e\n  ess a && cargo run -- ess b\nA=1 ess c | tee log",
            3,
        ),
        (
            "if ! ess --version; then v=$(ess --version); w=`ess`; fi",
            3,
        ),
        (
            "'ess' x\ncommand ess x\nexec ess x\ntime ess x\nsudo ESS x\nxargs ess.exe",
            6,
        ),
        (
            "./ess z\n/usr/bin/ess z\n\"$dir/ess\" z\ntarget/ess/ess-0.1.0-x86_64/ess z",
            4,
        ),
        ("X=ess; $X y\nenv -- ess y\ncargo run -p other -- ess y", 3),
        ("cargo run --locked -p llm-conformance -- \"ess\" y", 1),
        ("cargo run --locked -p llm-conformance -- ess y && ess y", 1),
    ] {
        assert_eq!(bare_ess(script), bare, "{script}");
    }
}

/// Every string in `value`, at any depth, in the order of the document.
fn strings(value: &Value, texts: &mut Vec<String>) {
    match value {
        Value::String(text) => texts.push(text.clone()),
        Value::Sequence(items) => {
            for item in items {
                strings(item, texts);
            }
        }
        Value::Mapping(entries) => {
            for entry in entries.values() {
                strings(entry, texts);
            }
        }
        Value::Tagged(tagged) => strings(&tagged.value, texts),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// What in a Taskfile names an `ess` the runner did not resolve ([`bare_ess`]).
///
/// No form of a command is passed over, because every string of the document is read, wherever
/// it stands: a command written as a string, the `cmd:` of a map, the `cmd:` beside a `for:`, a
/// `defer:`, the `sh:` of a variable or a precondition, a `status:` line, a task written as one
/// string or as a list. A `task:` reference is read too and does no harm. Two things are not
/// commands and are left out: the `desc` and the `summary` of a task, which are shown and never
/// run. An `includes:` is refused, because the file it names is not read here.
fn taskfile_bare(text: &str) -> Vec<String> {
    let mut taskfile: Value = serde_yaml::from_str(text).expect("a Taskfile is YAML");
    let mut found = Vec::new();
    if taskfile.get("includes").is_some() {
        found.push("includes: an included Taskfile is not read by this test".to_owned());
    }
    if let Some(tasks) = taskfile.get_mut("tasks").and_then(Value::as_mapping_mut) {
        for task in tasks.values_mut() {
            if let Some(task) = task.as_mapping_mut() {
                task.remove("desc");
                task.remove("summary");
            }
        }
    }
    let mut texts = Vec::new();
    strings(&taskfile, &mut texts);
    found.extend(texts.into_iter().filter(|text| bare_ess(text) > 0));
    found
}

#[test]
fn every_form_a_taskfile_command_can_take_is_read() {
    let taskfile = "\
version: '3'
vars:
  FOUND:
    sh: ess --version
tasks:
  plain:
    desc: Runs ESS; ess is not started by a description
    summary: ess
    cmds:
      - ess plain
      - task: other
  map:
    cmds:
      - cmd: ess map
        silent: true
  loop:
    cmds:
      - for: [a, b]
        cmd: ess loop {{.ITEM}}
  deferred:
    cmds:
      - defer: ess deferred
      - defer: {task: other}
  guarded:
    preconditions:
      - sh: ess guarded
    status:
      - ess status
    cmds:
      - cargo run --locked -p llm-conformance -- ess specify validate --path spec
  short: ess short
  list: [ess list]
  desc: ess in a task that is named as a description is
";
    assert_eq!(
        taskfile_bare(taskfile),
        [
            "ess --version",
            "ess plain",
            "ess map",
            "ess loop {{.ITEM}}",
            "ess deferred",
            "ess guarded",
            "ess status",
            "ess short",
            "ess list",
            "ess in a task that is named as a description is",
        ]
    );
    assert_eq!(
        taskfile_bare("version: '3'\nincludes:\n  other: ./other\ntasks: {}\n"),
        ["includes: an included Taskfile is not read by this test"]
    );
}

#[test]
fn no_task_runs_an_ess_the_runner_did_not_resolve() {
    let text = read("Taskfile.yml");
    let bare = taskfile_bare(&text);
    assert!(
        bare.is_empty(),
        "Taskfile.yml names an `ess` the runner did not resolve: {bare:#?}"
    );
    let taskfile: Value = serde_yaml::from_str(&text).expect("Taskfile.yml");
    let check: Vec<&str> = taskfile["tasks"]["check"]["cmds"]
        .as_sequence()
        .expect("cmds")
        .iter()
        .filter_map(|command| command.as_str().or_else(|| command["cmd"].as_str()))
        .collect();
    assert!(check.contains(&VALIDATE), "{check:?}");
}

/// What the steps of one workflow hold.
struct Steps {
    /// How many steps have the name of the step that installs `ess`.
    installs: usize,
    /// How many `run:` scripts the steps have.
    scripts: usize,
    /// How many `run:` scripts stand elsewhere in the file, where they are not read.
    unread: usize,
    /// Each step whose script names an `ess` the runner did not resolve.
    bare: Vec<String>,
}

/// How many `run:` keys with a script `value` holds, at any depth.
fn run_keys(value: &Value) -> usize {
    match value {
        Value::Sequence(items) => items.iter().map(run_keys).sum(),
        Value::Mapping(entries) => entries
            .iter()
            .map(|(key, entry)| {
                usize::from(key.as_str() == Some("run") && entry.is_string()) + run_keys(entry)
            })
            .sum(),
        Value::Tagged(tagged) => run_keys(&tagged.value),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => 0,
    }
}

/// Reads the steps of the workflow `file`. One step may start `ess` itself: the step named
/// [`INSTALL`] in job `libraries-and-plan` of `gate.yml`, which puts the pinned release on
/// `PATH`. A step of that name in another job or another file is a step like any other, and
/// every step of that name is counted, so that the caller can hold their number to one.
fn steps(file: &str, text: &str) -> Steps {
    let workflow: Value =
        serde_yaml::from_str(text).unwrap_or_else(|error| panic!("{file}: {error}"));
    let mut found = Steps {
        installs: 0,
        scripts: 0,
        unread: 0,
        bare: Vec::new(),
    };
    for (job, body) in workflow["jobs"].as_mapping().into_iter().flatten() {
        for step in body["steps"].as_sequence().into_iter().flatten() {
            let name = step["name"].as_str();
            if name == Some(INSTALL) {
                found.installs += 1;
            }
            let Some(script) = step["run"].as_str() else {
                continue;
            };
            found.scripts += 1;
            let exempt = file == "gate.yml"
                && job.as_str() == Some("libraries-and-plan")
                && name == Some(INSTALL);
            if !exempt && bare_ess(script) > 0 {
                found
                    .bare
                    .push(format!("{file}: step {name:?} of job {job:?}: {script}"));
            }
        }
    }
    found.unread = run_keys(&workflow) - found.scripts;
    found
}

#[test]
fn the_step_that_installs_ess_is_exempt_once_and_only_where_it_is() {
    let gate = "\
jobs:
  libraries-and-plan:
    steps:
      - name: Install ess
        run: |
          tar -xzf ess.tar.gz
          ess --version
      - name: Specification and plan
        run: cargo run --locked -p llm-conformance -- ess specify validate --path spec
";
    let found = steps("gate.yml", gate);
    assert_eq!((found.installs, found.scripts, found.unread), (1, 2, 0));
    assert!(found.bare.is_empty(), "{:?}", found.bare);

    // The same step in another file or in another job is a step like any other.
    assert_eq!(steps("other.yml", gate).bare.len(), 1);
    let second = format!(
        "{gate}  other:\n    steps:\n      - name: Install ess\n        run: ess specify validate\n"
    );
    let found = steps("gate.yml", &second);
    assert_eq!(found.installs, 2, "a second step of the name is counted");
    assert_eq!(found.bare.len(), 1, "{:?}", found.bare);

    // A second step of the name in the same job is exempt as the first is, and counted.
    let twice = format!("{gate}      - name: Install ess\n        run: ess specify validate\n");
    let found = steps("gate.yml", &twice);
    assert_eq!((found.installs, found.bare.len()), (2, 0));

    // A script that is not a step's is not read, and is counted as unread.
    let outside = format!(
        "{gate}  composite:\n    runs:\n      nested:\n        - run: ess specify validate\n"
    );
    assert_eq!(steps("gate.yml", &outside).unread, 1);
}

/// CI validates the specification with the command `task check` has, so the forge and the local
/// gate go through one resolution, and no step of any workflow starts an `ess` the runner did
/// not resolve. The step that installs it is the one place that may, and there is one such step.
#[test]
fn no_workflow_step_runs_an_ess_the_runner_did_not_resolve() {
    let directory = repository().join(".github/workflows");
    let (mut installs, mut scripts) = (0, 0);
    for entry in std::fs::read_dir(&directory).expect("the workflows are listed") {
        let file = entry.expect("an entry").file_name();
        let file = file.to_str().expect("a workflow file name");
        let found = steps(file, &read(&format!(".github/workflows/{file}")));
        assert!(
            found.bare.is_empty(),
            "an `ess` the runner did not resolve: {:#?}",
            found.bare
        );
        assert_eq!(
            found.unread, 0,
            "{file} has a `run:` outside the steps of its jobs"
        );
        installs += found.installs;
        scripts += found.scripts;
    }
    assert_eq!(installs, 1, "one step of one workflow is named {INSTALL:?}");
    assert!(scripts > 10, "{scripts} scripts");

    let workflow = workflow();
    let validating = workflow["jobs"]["libraries-and-plan"]["steps"]
        .as_sequence()
        .expect("steps")
        .iter()
        .find(|step| step["name"].as_str() == Some("Specification and plan"))
        .and_then(|step| step["run"].as_str())
        .expect("a step validates the specification");
    let lines: Vec<&str> = validating.lines().map(str::trim).collect();
    assert!(lines.contains(&VALIDATE), "{lines:?}");
}

/// A line that starts the `ess` on `PATH` in quotes, as the script of another shell, or after
/// the `--` of another command than the runner's is bare too.
#[test]
fn adversary_w32_an_ess_in_quotes_or_after_another_commands_dashes_is_bare() {
    let missed: Vec<&str> = [
        "sh -c \"ess specify validate --path spec\"",
        "bash -c 'ess specify validate --path spec'",
        "\"ess\" specify validate --path spec",
        "env -- ess specify validate --path spec",
    ]
    .into_iter()
    .filter(|script| bare_ess(script) == 0)
    .collect();
    assert!(missed.is_empty(), "not counted as bare: {missed:#?}");
}
