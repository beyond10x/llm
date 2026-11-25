//! The `ux-browser` pin has one source, the `UX_BROWSER_VERSION` and `UX_BROWSER_SHA256` of the
//! "Install ux-browser" step in `docs-ux.yml`. The conformance runner reads it there to find or
//! fetch the `ux-browser` the local docs UX gate runs (`checks/conformance/src/ess.rs`), and the
//! harness in `ux/` reads the version there to compare it with what a run's evidence names. The
//! step's form, the documents and the places that may start a `ux-browser` are held to it here.

use super::{read, repository};
use serde_yaml::Value;

/// The workflow that holds the pin.
const WORKFLOW: &str = ".github/workflows/docs-ux.yml";

/// The step of job `ux` that installs the pinned release.
const INSTALL: &str = "Install ux-browser";

/// The runner's command up to the `ux-browser` it is handed: the one way a task names it.
const RUNNER: [&str; 6] = ["cargo", "run", "--locked", "-p", "llm-conformance", "--"];

/// What the install step's script has to hold, each with what it is for.
const SCRIPT: [(&str, &str); 8] = [
    (
        "asset=\"ux-browser-$UX_BROWSER_VERSION-x86_64-unknown-linux-gnu.tar.gz\"",
        "the asset is named by the pinned version",
    ),
    (
        "release=\"https://github.com/beyond10x/uxui/releases/download/$UX_BROWSER_VERSION\"",
        "the release is the pinned version's, of the uxui repository",
    ),
    (
        "curl --fail --silent --show-error --location --output \"$asset\" \"$release/$asset\"",
        "the asset is downloaded",
    ),
    (
        "curl --fail --silent --show-error --location --output SHA256SUMS \"$release/SHA256SUMS\"",
        "the release's digest list is downloaded",
    ),
    (
        "grep -Fx \"$UX_BROWSER_SHA256  $asset\" SHA256SUMS",
        "the release's digest line equals the pinned digest",
    ),
    (
        "grep -F \"  $asset\" SHA256SUMS | sha256sum --check --strict",
        "the downloaded file has that digest",
    ),
    (
        "echo \"$RUNNER_TEMP/ux-browser-$UX_BROWSER_VERSION-x86_64-unknown-linux-gnu\" >> \"$GITHUB_PATH\"",
        "the unpacked directory is on PATH",
    ),
    (
        "echo \"UX_BROWSER=$RUNNER_TEMP/ux-browser-$UX_BROWSER_VERSION-x86_64-unknown-linux-gnu/ux-browser\" >> \"$GITHUB_ENV\"",
        "the harness is handed the unpacked binary by its path",
    ),
];

/// The pinned version and digest of the workflow `text`, or everything in which its install step
/// is not the form the resolver, the harness and this file read.
fn pin(text: &str) -> Result<(String, String), Vec<String>> {
    let workflow: Value = match serde_yaml::from_str(text) {
        Ok(workflow) => workflow,
        Err(error) => return Err(vec![format!("not YAML: {error}")]),
    };
    let named: Vec<&Value> = workflow["jobs"]["ux"]["steps"]
        .as_sequence()
        .into_iter()
        .flatten()
        .filter(|step| step["name"].as_str() == Some(INSTALL))
        .collect();
    let [step] = named[..] else {
        return Err(vec![format!(
            "job ux has {} steps named {INSTALL:?}, not one",
            named.len()
        )]);
    };
    let mut wrong = Vec::new();
    let version = step["env"]["UX_BROWSER_VERSION"].as_str().unwrap_or("");
    let numbers: Vec<&str> = version.split('.').collect();
    if numbers.len() != 3
        || !numbers
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        wrong.push(format!(
            "UX_BROWSER_VERSION is {version:?}, not three numbers in a string"
        ));
    }
    let digest = step["env"]["UX_BROWSER_SHA256"].as_str().unwrap_or("");
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        wrong.push(format!(
            "UX_BROWSER_SHA256 is {digest:?}, not 64 lowercase hex digits"
        ));
    }
    for name in ["UX_BROWSER_VERSION:", "UX_BROWSER_SHA256:"] {
        let set = text.matches(name).count();
        if set != 1 {
            wrong.push(format!("{name} is set {set} times, not once"));
        }
    }
    // The harness reads the version as text, without a YAML reader: one line of this form.
    let line = format!("UX_BROWSER_VERSION: \"{version}\"");
    if text.lines().filter(|found| found.trim() == line).count() != 1 {
        wrong.push(format!(
            "no line is `{line}`, the form ux/core/pin.ts reads"
        ));
    }
    let script = step["run"].as_str().unwrap_or("");
    let lines: Vec<&str> = script.lines().map(str::trim).collect();
    if lines.first() != Some(&"set -euo pipefail") {
        wrong.push("the script does not start with `set -euo pipefail`".to_owned());
    }
    for (line, purpose) in SCRIPT {
        if !lines.contains(&line) {
            wrong.push(format!("the script has no line `{line}`: {purpose}"));
        }
    }
    if wrong.is_empty() {
        Ok((version.to_owned(), digest.to_owned()))
    } else {
        Err(wrong)
    }
}

/// The pin of this repository's workflow.
fn pinned() -> (String, String) {
    pin(&read(WORKFLOW)).unwrap_or_else(|wrong| panic!("{WORKFLOW}: {wrong:#?}"))
}

#[test]
fn ci_installs_ux_browser_pinned_by_version_and_digest() {
    let (version, digest) = pinned();
    assert_eq!(version.split('.').count(), 3, "{version}");
    assert_eq!(digest.len(), 64, "{digest}");
}

/// The step the reader takes, and each change of it that the reader refuses, with what it says.
#[test]
fn an_install_step_of_another_form_is_refused_with_what_is_wrong() {
    let digest = "00000000000000000000000000000000000000000000000000000000000000aa";
    let mut script = String::from("          set -euo pipefail\n          cd \"$RUNNER_TEMP\"\n");
    for (line, _) in SCRIPT {
        script.push_str("          ");
        script.push_str(line);
        script.push('\n');
    }
    let good = format!(
        "jobs:\n  ux:\n    steps:\n      - name: Install ux-browser\n        env:\n          UX_BROWSER_VERSION: \"7.8.9\"\n          UX_BROWSER_SHA256: {digest}\n        run: |\n{script}"
    );
    assert_eq!(pin(&good), Ok(("7.8.9".to_owned(), digest.to_owned())));

    let unquoted = good.replace("UX_BROWSER_VERSION: \"7.8.9\"", "UX_BROWSER_VERSION: 7.8.9");
    let folded = good.replace(
        "UX_BROWSER_VERSION: \"7.8.9\"",
        "UX_BROWSER_VERSION: >-\n            7.8.9",
    );
    let unchecked = good.replace(
        "          grep -F \"  $asset\" SHA256SUMS | sha256sum --check --strict\n",
        "",
    );
    let unlisted = good.replace("grep -Fx \"$UX_BROWSER_SHA256  $asset\" SHA256SUMS", "true");
    let twice = format!(
        "{good}      - name: Later\n        env:\n          UX_BROWSER_VERSION: \"7.9.0\"\n        run: echo\n"
    );
    let second = format!("{good}      - name: Install ux-browser\n        run: echo\n");
    for (text, why) in [
        (
            good.replace("  ux:\n", "  docs:\n"),
            "job ux has 0 steps named \"Install ux-browser\", not one",
        ),
        (
            second,
            "job ux has 2 steps named \"Install ux-browser\", not one",
        ),
        (
            good.replace("\"7.8.9\"", "\"7.8\""),
            "UX_BROWSER_VERSION is \"7.8\", not three numbers in a string",
        ),
        (unquoted, "no line is `UX_BROWSER_VERSION: \"7.8.9\"`"),
        (folded, "no line is `UX_BROWSER_VERSION: \"7.8.9\"`"),
        (
            good.replace(digest, "00AA"),
            "UX_BROWSER_SHA256 is \"00AA\", not 64 lowercase hex digits",
        ),
        (twice, "UX_BROWSER_VERSION: is set 2 times, not once"),
        (unchecked, "the downloaded file has that digest"),
        (
            unlisted,
            "the release's digest line equals the pinned digest",
        ),
        (
            good.replace("beyond10x/uxui/releases", "another/uxui/releases"),
            "the release is the pinned version's, of the uxui repository",
        ),
        (
            good.replace("set -euo pipefail", "set -e"),
            "the script does not start with `set -euo pipefail`",
        ),
        (
            good.replace(">> \"$GITHUB_ENV\"", ">> /dev/null"),
            "the harness is handed the unpacked binary by its path",
        ),
    ] {
        let wrong = pin(&text).expect_err(why);
        assert!(
            wrong.iter().any(|found| found.contains(why)),
            "{why}: {wrong:#?}"
        );
    }
}

/// A word as a reader takes it: without the punctuation and the code marks around it.
fn bare(word: &str) -> &str {
    word.trim_matches(|character: char| "`,;:.()[]*\"'".contains(character))
}

/// Every version `text` gives for `ux-browser` or for the uxui release it comes in: a word that
/// starts with a digit, after a `v` or not, and holds a dot, where the word before it, passing
/// over `version`, `release` and `tag`, is `ux-browser` or `uxui`; and a version joined to the
/// name, as in `ux-browser-0.8.1-x86_64-unknown-linux-gnu.tar.gz`.
///
/// It errs on the side of reading too much: a version that follows the name by accident is read
/// as one of `ux-browser` and fails the test, which a rewording mends.
fn versions(text: &str) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut found = Vec::new();
    for (at, word) in words.iter().enumerate() {
        let lower = word.to_ascii_lowercase();
        if let Some(start) = lower.find("ux-browser-") {
            let rest = &word[start + "ux-browser-".len()..];
            let length = rest
                .find(|character: char| !character.is_ascii_digit() && character != '.')
                .unwrap_or(rest.len());
            let version = rest[..length].trim_end_matches('.');
            if version.starts_with(|first: char| first.is_ascii_digit()) && version.contains('.') {
                found.push(version.to_owned());
            }
        }
        let version = bare(word);
        let version = version.strip_prefix(['v', 'V']).unwrap_or(version);
        if !(version.starts_with(|first: char| first.is_ascii_digit()) && version.contains('.')) {
            continue;
        }
        let subject = words[..at].iter().rev().find(|word| {
            !["version", "release", "tag"].contains(&bare(word).to_ascii_lowercase().as_str())
        });
        if subject.is_some_and(|word| {
            let name = bare(word).to_ascii_lowercase();
            name == "ux-browser" || name == "uxui"
        }) {
            found.push(version.to_owned());
        }
    }
    found
}

#[test]
fn a_version_is_read_as_a_ux_browser_version_by_the_word_before_it_or_joined_to_the_name() {
    for (text, found) in [
        ("`ux-browser` 0.8.1 of the uxui repository", &["0.8.1"][..]),
        ("uxui release 0.8.2, then", &["0.8.2"]),
        ("ux-browser version v0.9.0.", &["0.9.0"]),
        (
            "`ux-browser-0.8.1-x86_64-unknown-linux-gnu.tar.gz`",
            &["0.8.1"],
        ),
        ("UXUI 0.8 is another", &["0.8"]),
        (
            "ESS 0.98.2, Rust 1.98.0, `ux-browser-checks/1` and ux-browser-<version>",
            &[],
        ),
    ] {
        assert_eq!(versions(text), found, "{text}");
    }
}

/// `AGENTS.md` names the pinned version; `README.md` and the getting-started page name the same
/// when they name one. These are the documents the ESS pin is held in too; `CHANGELOG.md` is not
/// read: it is prose in the path list of `gate.yml`, and its lines keep the version of their day.
#[test]
fn the_documents_name_the_version_ci_installs() {
    let (version, digest) = pinned();
    for (document, names_it) in [
        ("AGENTS.md", true),
        ("README.md", false),
        ("website/docs/getting-started.md", false),
    ] {
        let text = read(document);
        let named = versions(&text);
        assert!(
            !(names_it && named.is_empty()),
            "{document} names a ux-browser version"
        );
        for found in named {
            assert_eq!(found, version, "{document} names ux-browser {found}");
        }
        assert!(
            !text.contains(&digest),
            "{document} repeats the pinned digest; it has one place, {WORKFLOW}"
        );
    }
}

/// How many words of a script name a `ux-browser` to start: the name as a whole name in a word,
/// where what stands around it is no letter, no digit and none of `_`, `.` and `-`. The one such
/// word that is not counted is the argument right after [`RUNNER`] on the same line, which makes
/// the runner print the pinned one's path.
fn bare_ux_browser(script: &str) -> usize {
    script
        .replace("\\\n", " ")
        .lines()
        .map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            words
                .iter()
                .enumerate()
                .filter(|&(at, word)| {
                    let named = word
                        .split(|character: char| {
                            !(character.is_ascii_alphanumeric() || "_.-".contains(character))
                        })
                        .any(|name| name.eq_ignore_ascii_case("ux-browser"));
                    let handed = *word == "ux-browser"
                        && at >= RUNNER.len()
                        && words[at - RUNNER.len()..at] == RUNNER;
                    named && !handed
                })
                .count()
        })
        .sum()
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

/// What in a Taskfile starts a `ux-browser` the runner did not resolve, or fetches a release
/// itself: every string of the document but the `desc` and `summary` of a task.
fn taskfile_wrong(text: &str) -> Vec<String> {
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
    for text in texts {
        if bare_ux_browser(&text) > 0 {
            found.push(format!("a ux-browser the runner did not resolve: {text}"));
        }
        if ["curl", "wget", "releases/download"]
            .iter()
            .any(|fetch| text.contains(fetch))
        {
            found.push(format!("a download: {text}"));
        }
    }
    found
}

#[test]
fn a_task_that_starts_or_fetches_a_ux_browser_itself_is_found() {
    let taskfile = "\
version: '3'
tasks:
  resolved:
    desc: Starts ux-browser; a description starts nothing
    vars:
      UX_BROWSER:
        sh: cargo run --locked -p llm-conformance -- ux-browser
    cmds:
      - UX_BROWSER={{.UX_BROWSER}} npx playwright test
      - node cli.ts round --config ux-browser.round.checks.yaml
  bare:
    cmds:
      - ux-browser checks --config ux-browser.checks.yaml
      - cmd: ./target/ux-browser/ux-browser checks
  fetched:
    cmds:
      - curl --location https://github.com/beyond10x/uxui/releases/download/0.8.1/asset.tar.gz
";
    assert_eq!(
        taskfile_wrong(taskfile),
        [
            "a ux-browser the runner did not resolve: ux-browser checks --config ux-browser.checks.yaml",
            "a ux-browser the runner did not resolve: ./target/ux-browser/ux-browser checks",
            "a download: curl --location https://github.com/beyond10x/uxui/releases/download/0.8.1/asset.tar.gz",
        ]
    );
    assert_eq!(
        taskfile_wrong("version: '3'\nincludes:\n  other: ./other\ntasks: {}\n"),
        ["includes: an included Taskfile is not read by this test"]
    );
}

/// The local side has one way to its `ux-browser`: the runner's `ux-browser` command, in the
/// install task and in each task that starts the harness with page checks.
#[test]
fn the_tasks_take_ux_browser_from_the_runner_and_fetch_nothing_themselves() {
    let text = read("Taskfile.yml");
    let wrong = taskfile_wrong(&text);
    assert!(wrong.is_empty(), "Taskfile.yml: {wrong:#?}");
    let taskfile: Value = serde_yaml::from_str(&text).expect("Taskfile.yml");
    let resolve = RUNNER.join(" ") + " ux-browser";
    for task in ["ux-install", "ux-check", "ux-round"] {
        let mut texts = Vec::new();
        strings(&taskfile["tasks"][task], &mut texts);
        assert!(
            texts.iter().any(|text| text.contains(&resolve)),
            "task {task} runs `{resolve}`: {texts:#?}"
        );
    }
    assert!(
        !repository().join("ux/ux-browser.pin").exists(),
        "the pin has one place, {WORKFLOW}; ux/ux-browser.pin is a second"
    );
}

/// CI installs the pinned release in its step and builds no resolver for it: no step of
/// `docs-ux.yml` runs `cargo`, and the UX step gets the binary as the `UX_BROWSER` the install
/// step wrote.
#[test]
fn the_docs_ux_workflow_builds_no_rust_and_its_ux_step_takes_what_the_install_step_handed_over() {
    let workflow: Value = serde_yaml::from_str(&read(WORKFLOW)).expect("the workflow is YAML");
    let steps = workflow["jobs"]["ux"]["steps"]
        .as_sequence()
        .expect("job ux has steps");
    let mut texts = Vec::new();
    strings(&workflow["jobs"], &mut texts);
    let cargo: Vec<&String> = texts
        .iter()
        .filter(|text| text.split_whitespace().any(|word| word == "cargo"))
        .collect();
    assert!(cargo.is_empty(), "{WORKFLOW} runs cargo: {cargo:#?}");
    let at = |wanted: &dyn Fn(&Value) -> bool| steps.iter().position(wanted);
    let install = at(&|step| step["name"].as_str() == Some(INSTALL)).expect("the install step");
    let ux = at(&|step| {
        step["run"]
            .as_str()
            .is_some_and(|run| run.contains("npx playwright test"))
    })
    .expect("a step runs the Playwright specs");
    assert!(
        install < ux,
        "the install step comes before the step that runs the specs"
    );
    assert!(
        steps[ux].get("env").is_none(),
        "the UX step sets no UX_BROWSER of its own: {:?}",
        steps[ux]["env"]
    );
}

/// Each string of a step of the workflow `file` that names a `ux-browser` to start, and how
/// many steps have the install step's name. The install step of job `ux` of `docs-ux.yml` is the
/// one step that may name it; a step's `name` and `uses` are not commands and are not read.
fn steps(file: &str, text: &str) -> (usize, Vec<String>) {
    let workflow: Value =
        serde_yaml::from_str(text).unwrap_or_else(|error| panic!("{file}: {error}"));
    let mut installs = 0;
    let mut bare = Vec::new();
    for (job, body) in workflow["jobs"].as_mapping().into_iter().flatten() {
        for step in body["steps"].as_sequence().into_iter().flatten() {
            let name = step["name"].as_str();
            if name == Some(INSTALL) {
                installs += 1;
                if file == "docs-ux.yml" && job.as_str() == Some("ux") {
                    continue;
                }
            }
            let mut read = step.clone();
            if let Some(entries) = read.as_mapping_mut() {
                entries.remove("name");
                entries.remove("uses");
            }
            let mut texts = Vec::new();
            strings(&read, &mut texts);
            for text in texts {
                if bare_ux_browser(&text) > 0 {
                    bare.push(format!("{file}: step {name:?} of job {job:?}: {text}"));
                }
            }
        }
    }
    (installs, bare)
}

#[test]
fn a_step_that_starts_a_ux_browser_from_path_is_found() {
    let workflow = "\
jobs:
  ux:
    steps:
      - name: Install ux-browser
        run: tar -xzf ux-browser.tar.gz && ux-browser checks --help
      - name: The UX step
        run: timeout 150 npx playwright test --workers=4
";
    assert_eq!(steps("docs-ux.yml", workflow), (1, Vec::new()));
    assert_eq!(steps("other.yml", workflow).1.len(), 1);
    let started = format!("{workflow}      - name: Checks\n        run: ux-browser checks\n");
    let (installs, bare) = steps("docs-ux.yml", &started);
    assert_eq!((installs, bare.len()), (1, 1), "{bare:?}");
}

/// No step of any workflow but the install step names a `ux-browser` to start: the UX step
/// starts the one the install step handed it as `UX_BROWSER`. There is one install step.
#[test]
fn no_workflow_step_starts_a_ux_browser_the_install_step_did_not_hand_over() {
    let directory = repository().join(".github/workflows");
    let mut installs = 0;
    let mut files = 0;
    for entry in std::fs::read_dir(&directory).expect("the workflows are listed") {
        let file = entry.expect("an entry").file_name();
        let file = file.to_str().expect("a workflow file name");
        let (found, bare) = steps(file, &read(&format!(".github/workflows/{file}")));
        assert!(bare.is_empty(), "a `ux-browser` from PATH: {bare:#?}");
        installs += found;
        files += 1;
    }
    assert_eq!(installs, 1, "one step of one workflow is named {INSTALL:?}");
    assert!(files > 1, "{files} workflows");
}
