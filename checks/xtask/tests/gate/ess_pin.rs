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

/// Whether `word` says nothing of its own between a name and its version: a whole remark in
/// parentheses, as `([GitHub](...))` is, or the bar between two cells of a table.
fn passed_over(word: &str) -> bool {
    word == "|"
        || (word.starts_with('(') && word.trim_end_matches([',', '.', ';', ':']).ends_with(')'))
}

/// Every version `text` gives for ESS: each word that starts with a digit and holds a dot, where
/// the word before it names ESS. A remark in parentheses or a table bar between the two is passed
/// over, as in `[ESS](...) ([GitHub](...)) 0.1.0` and `| ESS | 0.1.0 |`.
fn ess_versions(text: &str) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut versions = Vec::new();
    for (at, word) in words.iter().enumerate() {
        let version = bare(word);
        if !version.starts_with(|first: char| first.is_ascii_digit()) || !version.contains('.') {
            continue;
        }
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

/// How often a shell script starts the `ess` on `PATH`: each word `ess`, alone or opening a
/// command substitution, that is not the argument right after the `--` of the runner's command.
fn bare_ess(script: &str) -> usize {
    let words: Vec<&str> = script.split_whitespace().collect();
    words
        .iter()
        .enumerate()
        .filter(|&(at, word)| {
            let ended = word.trim_end_matches([')', ';', '&', '|', '`']);
            ended.rsplit(['(', '`']).next() == Some("ess") && (at == 0 || words[at - 1] != "--")
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
            "asset=\"ess-$V.tar.gz\"\necho \"$dir/ess\" >> \"$GITHUB_PATH\"\n./ess z",
            0,
        ),
        ("ess specify validate --path spec", 1),
        (
            "set -e\n  ess a && cargo run -- ess b\nA=1 ess c | tee log",
            2,
        ),
        (
            "if ! ess --version; then v=$(ess --version); w=`ess`; fi",
            3,
        ),
    ] {
        assert_eq!(bare_ess(script), bare, "{script}");
    }
}

#[test]
fn no_task_runs_an_ess_the_runner_did_not_resolve() {
    let taskfile: Value = serde_yaml::from_str(&read("Taskfile.yml")).expect("Taskfile.yml");
    let mut check = Vec::new();
    for (name, task) in taskfile["tasks"].as_mapping().expect("tasks") {
        for command in task["cmds"].as_sequence().expect("cmds") {
            let Some(line) = command.as_str() else {
                continue;
            };
            assert_eq!(
                bare_ess(line),
                0,
                "task {name:?} runs the `ess` on PATH: {line}"
            );
            if name.as_str() == Some("check") {
                check.push(line.to_owned());
            }
        }
    }
    assert!(check.contains(&VALIDATE.to_owned()), "{check:?}");
}

/// CI validates the specification with the command `task check` has, so the forge and the local
/// gate go through one resolution, and no step of any workflow starts the `ess` on `PATH`. The
/// step that installs it is the one place that may.
#[test]
fn no_workflow_step_runs_an_ess_the_runner_did_not_resolve() {
    let directory = repository().join(".github/workflows");
    let mut scripts = 0;
    for entry in std::fs::read_dir(&directory).expect("the workflows are listed") {
        let file = entry.expect("an entry").file_name();
        let file = file.to_str().expect("a workflow file name");
        let workflow: Value =
            serde_yaml::from_str(&read(&format!(".github/workflows/{file}"))).expect("YAML");
        for (job, body) in workflow["jobs"].as_mapping().expect("jobs") {
            let Some(steps) = body["steps"].as_sequence() else {
                continue;
            };
            for step in steps {
                let name = step["name"].as_str();
                let Some(script) = step["run"].as_str() else {
                    continue;
                };
                scripts += 1;
                if file == "gate.yml" && name == Some(INSTALL) {
                    continue;
                }
                assert_eq!(
                    bare_ess(script),
                    0,
                    "{file}: step {name:?} of job {job:?} runs the `ess` on PATH: {script}"
                );
            }
        }
    }
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
