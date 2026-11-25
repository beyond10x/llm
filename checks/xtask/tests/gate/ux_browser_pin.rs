//! The `ux-browser` pin has one source, the `UX_BROWSER_VERSION` and `UX_BROWSER_SHA256` of the
//! "Install ux-browser" step in `docs-ux.yml`. The conformance runner reads it there to find or
//! fetch the `ux-browser` the local docs UX gate runs (`checks/conformance/src/ess.rs`), and the
//! harness in `ux/` reads the version there to compare it with what a run's evidence names. The
//! step's form, the documents and the texts that may name a `ux-browser` are held to it here.
//!
//! What this file holds of the workflows is what a search of their text holds: the install step
//! is read whole; no other text of a workflow names a `ux-browser`, a release of it, the uxui
//! repository or `UX_BROWSER`; no step uses an action of this repository; and in `docs-ux.yml` no
//! other step names the files a step writes the runner's environment and path to, and nothing
//! names a shell or a file read before a script. A step that hides the name (a name built from
//! parts, a script file the step runs, an action of another repository) is not found: what holds
//! then is the version check of the evidence and the digest the install step checks.

use super::{read, repository};
use serde_yaml::Value;

/// The workflow that holds the pin.
const WORKFLOW: &str = ".github/workflows/docs-ux.yml";

/// The step of job `ux` that installs the pinned release.
const INSTALL: &str = "Install ux-browser";

/// The runner's command up to the `ux-browser` it is handed: the one way a task names it.
const RUNNER: [&str; 6] = ["cargo", "run", "--locked", "-p", "llm-conformance", "--"];

/// The line of the install step's script that goes to the directory the release is unpacked in.
/// It stands once, before the first download; the hand-over names the path under that directory.
const CD: &str = "cd \"$RUNNER_TEMP\"";

/// The lines of the install step's script after `set -euo pipefail`, in their order and each with
/// what it is for. Beside them the script holds [`CD`] and no other line.
const SCRIPT: [(&str, &str); 9] = [
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
    ("tar -xzf \"$asset\"", "the checked asset is unpacked"),
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
    wrong.extend(members_wrong(step));
    wrong.extend(script_wrong(step["run"].as_str().unwrap_or("")));
    if wrong.is_empty() {
        Ok((version.to_owned(), digest.to_owned()))
    } else {
        Err(wrong)
    }
}

/// The members of the install step that are not the ones this file reads: the step is its `name`,
/// its `env` with the two members of the pin, and its `run`. Another member (a shell, a condition,
/// a third variable) changes what the script does without a line of it changing.
fn members_wrong(step: &Value) -> Vec<String> {
    let keys = |value: &Value| -> Vec<String> {
        value
            .as_mapping()
            .into_iter()
            .flatten()
            .map(|(key, _)| key.as_str().unwrap_or("a key that is no string").to_owned())
            .collect()
    };
    let mut wrong = Vec::new();
    for key in keys(step) {
        if !["name", "env", "run"].contains(&key.as_str()) {
            wrong.push(format!(
                "the step has a member `{key}`; this guard reads `name`, `env` and `run`"
            ));
        }
    }
    for key in keys(&step["env"]) {
        if !["UX_BROWSER_VERSION", "UX_BROWSER_SHA256"].contains(&key.as_str()) {
            wrong.push(format!(
                "the step's `env` has a member `{key}`; it holds the pin and nothing else"
            ));
        }
    }
    wrong
}

/// Everything in which the install step's script is not the one this file reads: it starts with
/// `set -euo pipefail`, holds each line of [`SCRIPT`] once and in that order, [`CD`] once before
/// the first download, and no other line. So a second download, a second line to `GITHUB_ENV`
/// and a hand-over of another path are each a line it does not read.
fn script_wrong(script: &str) -> Vec<String> {
    let lines: Vec<&str> = script
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let mut wrong = Vec::new();
    if lines.first() != Some(&"set -euo pipefail") {
        wrong.push("the script does not start with `set -euo pipefail`".to_owned());
    }
    for (line, purpose) in SCRIPT {
        match lines.iter().filter(|found| **found == line).count() {
            1 => {}
            0 => wrong.push(format!("the script has no line `{line}`: {purpose}")),
            times => wrong.push(format!(
                "the script has the line `{line}` {times} times, not once: {purpose}"
            )),
        }
    }
    let read = |line: &str| SCRIPT.iter().any(|(wanted, _)| *wanted == line);
    for line in lines.iter().skip(1) {
        if !(read(line) || *line == CD) {
            wrong.push(format!(
                "the script has a line this guard does not read: `{line}`"
            ));
        }
    }
    let order: Vec<&str> = lines.iter().copied().filter(|line| read(line)).collect();
    if wrong.is_empty() && order != SCRIPT.map(|(line, _)| line) {
        wrong.push(
            "the script's lines are not in their order: the names, both downloads, both digest checks, the unpacking, the two hand-overs"
                .to_owned(),
        );
    }
    let download = lines.iter().position(|line| *line == SCRIPT[2].0);
    let changes: Vec<usize> = (0..lines.len()).filter(|at| lines[*at] == CD).collect();
    if changes.len() != 1 || download.is_some_and(|download| changes[0] > download) {
        wrong.push(format!(
            "the script has `{CD}` {} times or after the first download; it stands once, before it",
            changes.len()
        ));
    }
    wrong
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

/// The digest of the workflow texts the tests of this file build.
const DIGEST: &str = "00000000000000000000000000000000000000000000000000000000000000aa";

/// A workflow whose job `ux` is the install step in the form this file reads, pinned to 7.8.9
/// and [`DIGEST`]. A step appended to it is a later step of that job, a line appended a line of
/// the install step's script.
fn install_step() -> String {
    let mut text = format!(
        "jobs:\n  ux:\n    steps:\n      - name: Install ux-browser\n        env:\n          UX_BROWSER_VERSION: \"7.8.9\"\n          UX_BROWSER_SHA256: {DIGEST}\n        run: |\n          set -euo pipefail\n          cd \"$RUNNER_TEMP\"\n"
    );
    for (line, _) in SCRIPT {
        text.push_str("          ");
        text.push_str(line);
        text.push('\n');
    }
    text
}

/// The step the reader takes, and each change of it that the reader refuses, with what it says.
#[test]
fn an_install_step_of_another_form_is_refused_with_what_is_wrong() {
    let digest = DIGEST;
    let good = install_step();
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

/// The script and the members of the install step are read whole: a second hand-over, a second
/// download, a line twice, another path, another order and a member beside `name`, `env` and
/// `run` are each refused with what was found.
#[test]
fn an_install_step_with_a_line_or_a_member_the_guard_does_not_read_is_refused() {
    let good = install_step();
    assert_eq!(pin(&good), Ok(("7.8.9".to_owned(), DIGEST.to_owned())));
    let handed = "          echo \"UX_BROWSER=$RUNNER_TEMP/ux-browser-$UX_BROWSER_VERSION-x86_64-unknown-linux-gnu/ux-browser\" >> \"$GITHUB_ENV\"\n";
    assert!(good.ends_with(handed), "the hand-over is the last line");
    let early = good.replace(handed, "").replace(
        "          cd \"$RUNNER_TEMP\"\n",
        &format!("          cd \"$RUNNER_TEMP\"\n{handed}"),
    );
    for (text, why) in [
        (
            format!(
                "{good}          echo \"UX_BROWSER=$RUNNER_TEMP/other/checks\" >> \"$GITHUB_ENV\"\n"
            ),
            "the script has a line this guard does not read: `echo \"UX_BROWSER=$RUNNER_TEMP/other/checks\" >> \"$GITHUB_ENV\"`",
        ),
        (
            format!(
                "{good}          curl --location --output other.tar.gz https://releases.test/other.tar.gz\n"
            ),
            "the script has a line this guard does not read: `curl --location --output other.tar.gz https://releases.test/other.tar.gz`",
        ),
        (
            format!("{good}{handed}"),
            "2 times, not once: the harness is handed the unpacked binary by its path",
        ),
        (
            good.replace(
                "/ux-browser\" >> \"$GITHUB_ENV\"",
                "/checks\" >> \"$GITHUB_ENV\"",
            ),
            "the harness is handed the unpacked binary by its path",
        ),
        (early, "the script's lines are not in their order"),
        (
            good.replace("          cd \"$RUNNER_TEMP\"\n", ""),
            "the script has `cd \"$RUNNER_TEMP\"` 0 times or after the first download",
        ),
        (
            good.replace("          cd \"$RUNNER_TEMP\"\n", "").replace(
                "\"$release/$asset\"\n",
                "\"$release/$asset\"\n          cd \"$RUNNER_TEMP\"\n",
            ),
            "the script has `cd \"$RUNNER_TEMP\"` 1 times or after the first download",
        ),
        (
            good.replace("          tar -xzf \"$asset\"\n", ""),
            "the checked asset is unpacked",
        ),
        (
            good.replace(
                "        env:\n",
                "        continue-on-error: true\n        env:\n",
            ),
            "the step has a member `continue-on-error`",
        ),
        (
            good.replace(
                "        run: |\n",
                "          UX_BROWSER: /other/checks\n        run: |\n",
            ),
            "the step's `env` has a member `UX_BROWSER`",
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

/// Whether `text` holds `name` as a whole name: what stands around it is no letter, no digit and
/// no `_`. So `UX_BROWSER` is found in `$UX_BROWSER` and in `UX_BROWSER=`, and not in
/// `UX_BROWSER_VERSION`.
fn names(text: &str, name: &str) -> bool {
    text.match_indices(name).any(|(at, _)| {
        let around = [
            text[..at].chars().next_back(),
            text[at + name.len()..].chars().next(),
        ];
        !around
            .iter()
            .flatten()
            .any(|character| character.is_ascii_alphanumeric() || *character == '_')
    })
}

/// What a string outside the install step names that the install step alone may: a `ux-browser`
/// to start, a release of it or the uxui repository, the variable `UX_BROWSER`. It errs on the
/// side of reading too much: a string that only mentions one of the names is found, and a
/// rewording mends it. A string that builds a name from parts is not found.
fn foreign(text: &str) -> Vec<&'static str> {
    let lower = text.to_ascii_lowercase();
    let mut found = Vec::new();
    if bare_ux_browser(text) > 0 {
        found.push("names a `ux-browser` to start");
    }
    if lower.contains("ux-browser-") || names(&lower, "uxui") {
        found.push("names a release of `ux-browser` or the uxui repository");
    }
    if names(text, "UX_BROWSER") {
        found.push("names `UX_BROWSER`, which the install step alone hands over");
    }
    found
}

/// The place of every mapping member named `name` in `value`, at any depth: in an `env` of the
/// workflow, of a job, of a step or of a container.
fn members(value: &Value, name: &str, place: &str, found: &mut Vec<String>) {
    match value {
        Value::Mapping(entries) => {
            for (key, entry) in entries {
                let key = key.as_str().unwrap_or("?");
                let place = format!("{place}.{key}");
                if key == name {
                    found.push(place.clone());
                }
                members(entry, name, &place, found);
            }
        }
        Value::Sequence(items) => {
            for (at, item) in items.iter().enumerate() {
                members(item, name, &format!("{place}[{at}]"), found);
            }
        }
        Value::Tagged(tagged) => members(&tagged.value, name, place, found),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// What `docs-ux.yml` may not hold because it runs something before or around a step's script
/// that this guard does not read: a member `defaults` of the workflow or of a job, which can name
/// the shell of every `run`, and a member `BASH_ENV` at any depth, a file bash reads first.
fn shell_wrong(workflow: &Value) -> Vec<String> {
    let mut found = Vec::new();
    let mut holders = Vec::new();
    if workflow.get("defaults").is_some() {
        holders.push("the workflow".to_owned());
    }
    for (job, body) in workflow["jobs"].as_mapping().into_iter().flatten() {
        if body.get("defaults").is_some() {
            holders.push(format!("job `{}`", job.as_str().unwrap_or("?")));
        }
    }
    for holder in holders {
        found.push(format!(
            "docs-ux.yml: {holder} has a member `defaults`, which can name the shell a script runs in; this guard reads no shell but the runner's own"
        ));
    }
    let mut set = Vec::new();
    members(workflow, "BASH_ENV", "", &mut set);
    for place in set {
        found.push(format!(
            "docs-ux.yml: `{}` is a member `BASH_ENV`; bash reads the file it names before a script, and this guard does not read that file",
            place.trim_start_matches('.')
        ));
    }
    found
}

/// What a step other than the install step does, beside what [`foreign`] finds, that this guard
/// does not pass: it uses an action of this repository, in any workflow file, or it names
/// `GITHUB_ENV` or `GITHUB_PATH` in `docs-ux.yml`, the files a step writes the environment and
/// the path of the steps after it to.
fn step_wrong(file: &str, place: &str, step: &Value) -> Vec<String> {
    let mut found = Vec::new();
    if let Some(action) = step["uses"].as_str().filter(|uses| uses.starts_with("./")) {
        found.push(format!(
            "{file}: {place} uses `{action}`, an action of this repository; this guard reads no action file, so the step belongs in the workflow"
        ));
    }
    if file == "docs-ux.yml" {
        for text in rest(step, &["name"]) {
            for name in ["GITHUB_ENV", "GITHUB_PATH"] {
                if names(&text, name) {
                    found.push(format!(
                        "{file}: {place} names `{name}`; in this workflow the install step alone writes the runner's environment and path files: {text}"
                    ));
                }
            }
        }
    }
    found
}

/// Every string of `value` but the ones under its members `left`.
fn rest(value: &Value, left: &[&str]) -> Vec<String> {
    let mut read = value.clone();
    if let Some(entries) = read.as_mapping_mut() {
        for key in left {
            entries.remove(*key);
        }
    }
    let mut texts = Vec::new();
    strings(&read, &mut texts);
    texts
}

/// Adds to `found` what each of `texts`, the strings at `place` of the workflow `file`, does that
/// the install step alone may.
fn note(found: &mut Vec<String>, file: &str, place: &str, texts: Vec<String>) {
    for text in texts {
        for what in foreign(&text) {
            found.push(format!("{file}: {place} {what}: {text}"));
        }
    }
}

/// How many steps of the workflow `file` have the install step's name, and everything in its
/// text outside the install step that names what the install step alone may.
///
/// The install step of job `ux` of `docs-ux.yml` is read by [`pin`]. Everything else is read
/// here: every string of every other step, of every job and of the workflow outside its jobs,
/// but a `name`, is found when it names a `ux-browser` to start, a release of it or the uxui
/// repository, or the variable `UX_BROWSER` ([`foreign`]); a member `UX_BROWSER` is found at any
/// depth, the install step included ([`members`]); a step that uses an action of this repository
/// is found, and in `docs-ux.yml` a step that names `GITHUB_ENV` or `GITHUB_PATH`
/// ([`step_wrong`]), a member `defaults` and a member `BASH_ENV` ([`shell_wrong`]). A workflow
/// without a mapping `jobs`, a job with neither a list `steps` nor a `uses`, and a step that is
/// no mapping are found too: this guard does not read them.
///
/// A job-level `uses` of a workflow file of this repository is passed: [`steps`] reads that file
/// as a workflow. What a text search cannot see is not found: a name built from parts, a script
/// file a step runs, an action of another repository that names none of the above.
fn steps(file: &str, text: &str) -> (usize, Vec<String>) {
    let workflow: Value =
        serde_yaml::from_str(text).unwrap_or_else(|error| panic!("{file}: {error}"));
    let mut installs = 0;
    let mut found = Vec::new();
    let mut set = Vec::new();
    members(&workflow, "UX_BROWSER", "", &mut set);
    for place in set {
        found.push(format!(
            "{file}: `{}` is a member `UX_BROWSER`; the install step alone hands the binary over, by its line to `GITHUB_ENV`",
            place.trim_start_matches('.')
        ));
    }
    if file == "docs-ux.yml" {
        found.extend(shell_wrong(&workflow));
    }
    let outside = rest(&workflow, &["jobs", "name"]);
    note(&mut found, file, "the workflow outside its jobs", outside);
    let Some(jobs) = workflow["jobs"].as_mapping() else {
        found.push(format!(
            "{file}: no mapping `jobs`; this guard reads the jobs of a workflow"
        ));
        return (installs, found);
    };
    let mut unread = Vec::new();
    for (job, body) in jobs {
        let job = job.as_str().unwrap_or("?");
        let outside = rest(body, &["steps", "name"]);
        note(
            &mut found,
            file,
            &format!("job `{job}` outside its steps"),
            outside,
        );
        let Some(listed) = body["steps"].as_sequence() else {
            if body.get("steps").is_some() || body["uses"].as_str().is_none() {
                unread.push(format!(
                    "{file}: job `{job}` has neither a list `steps` nor a `uses`; this guard reads no other job"
                ));
            }
            continue;
        };
        for (at, step) in listed.iter().enumerate() {
            let name = step["name"].as_str();
            if name == Some(INSTALL) {
                installs += 1;
                if file == "docs-ux.yml" && job == "ux" {
                    continue;
                }
            }
            if !step.is_mapping() {
                unread.push(format!(
                    "{file}: step {at} of job `{job}` is no mapping; this guard reads no other step"
                ));
            }
            let name = name.unwrap_or("without a name");
            let place = format!("step `{name}` of job `{job}`");
            note(&mut found, file, &place, rest(step, &["name"]));
            found.extend(step_wrong(file, &place, step));
        }
    }
    found.extend(unread);
    (installs, found)
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

/// The workflows of this repository as [`steps`] reads them: no text but the install step names
/// a `ux-browser` to start, a release of it, the uxui repository or the variable `UX_BROWSER`, no
/// step uses an action of this repository, and no other step of `docs-ux.yml` names the runner's
/// environment or path file. There is one install step. A step that hides the name is not found
/// here ([`steps`]).
#[test]
fn no_workflow_step_starts_a_ux_browser_the_install_step_did_not_hand_over() {
    let directory = repository().join(".github/workflows");
    let mut installs = 0;
    let mut files = 0;
    for entry in std::fs::read_dir(&directory).expect("the workflows are listed") {
        let file = entry.expect("an entry").file_name();
        let file = file.to_str().expect("a workflow file name");
        let (found, bare) = steps(file, &read(&format!(".github/workflows/{file}")));
        assert!(
            bare.is_empty(),
            "a `ux-browser` the install step did not hand over: {bare:#?}"
        );
        installs += found;
        files += 1;
    }
    assert_eq!(installs, 1, "one step of one workflow is named {INSTALL:?}");
    assert!(files > 1, "{files} workflows");
}

#[test]
fn a_name_is_found_as_a_whole_name_only() {
    for text in [
        "\"$UX_BROWSER\" checks",
        "UX_BROWSER=/other/checks npx playwright test",
        "UX_BROWSER<<EOF",
        "${UX_BROWSER}",
    ] {
        assert!(names(text, "UX_BROWSER"), "{text}");
    }
    for text in [
        "$UX_BROWSER_VERSION",
        "LLM_UX_BROWSER_STORE",
        "UX_BROWSERS",
        "ux_browser",
    ] {
        assert!(!names(text, "UX_BROWSER"), "{text}");
    }
    assert!(names("github.com/beyond10x/uxui/releases", "uxui"));
    assert!(!names("fluxuity", "uxui"));
}

/// Each way a workflow gets the harness a `ux-browser` the install step did not hand over, on a
/// text, with what the guard says: a step or a job that names a release, a step that names the
/// variable, and a member `UX_BROWSER` at each level.
#[test]
fn a_step_or_a_job_that_fetches_a_release_or_sets_ux_browser_itself_is_found() {
    let good = install_step();
    assert_eq!(steps("docs-ux.yml", &good), (1, Vec::new()));
    let release = "names a release of `ux-browser` or the uxui repository";
    let variable = "names `UX_BROWSER`, which the install step alone hands over";
    for (text, found) in [
        (
            format!(
                "{good}      - name: Another release\n        run: gh release download 7.9.0 --repo beyond10x/uxui\n"
            ),
            format!(
                "docs-ux.yml: step `Another release` of job `ux` {release}: gh release download 7.9.0 --repo beyond10x/uxui"
            ),
        ),
        (
            format!(
                "{good}      - name: A mirror\n        run: curl --location --remote-name https://mirror.test/ux-browser-7.9.0-x86_64-unknown-linux-gnu.tar.gz\n"
            ),
            format!(
                "docs-ux.yml: step `A mirror` of job `ux` {release}: curl --location --remote-name https://mirror.test/ux-browser-7.9.0-x86_64-unknown-linux-gnu.tar.gz"
            ),
        ),
        (
            format!(
                "{good}      - uses: beyond10x/uxui/.github/actions/install@0123456789abcdef0123456789abcdef01234567\n"
            ),
            format!(
                "docs-ux.yml: step `without a name` of job `ux` {release}: beyond10x/uxui/.github/actions/install@0123456789abcdef0123456789abcdef01234567"
            ),
        ),
        (
            format!(
                "{good}  other:\n    uses: beyond10x/uxui/.github/workflows/install.yml@0123456789abcdef0123456789abcdef01234567\n"
            ),
            format!(
                "docs-ux.yml: job `other` outside its steps {release}: beyond10x/uxui/.github/workflows/install.yml@0123456789abcdef0123456789abcdef01234567"
            ),
        ),
        (
            format!(
                "{good}      - name: Hand over\n        run: echo \"UX_BROWSER=$RUNNER_TEMP/other/checks\" >> \"$GITHUB_ENV\"\n"
            ),
            format!(
                "docs-ux.yml: step `Hand over` of job `ux` {variable}: echo \"UX_BROWSER=$RUNNER_TEMP/other/checks\" >> \"$GITHUB_ENV\""
            ),
        ),
        (
            format!(
                "{good}      - name: The UX step\n        run: UX_BROWSER=/other/checks npx playwright test\n"
            ),
            format!(
                "docs-ux.yml: step `The UX step` of job `ux` {variable}: UX_BROWSER=/other/checks npx playwright test"
            ),
        ),
        (
            format!(
                "{good}      - name: The UX step\n        env:\n          UX_BROWSER: /other/checks\n        run: npx playwright test\n"
            ),
            "docs-ux.yml: `jobs.ux.steps[1].env.UX_BROWSER` is a member `UX_BROWSER`".to_owned(),
        ),
        (
            good.replace(
                "  ux:\n",
                "  ux:\n    env:\n      UX_BROWSER: /other/checks\n",
            ),
            "docs-ux.yml: `jobs.ux.env.UX_BROWSER` is a member `UX_BROWSER`".to_owned(),
        ),
        (
            format!("env:\n  UX_BROWSER: /other/checks\n{good}"),
            "docs-ux.yml: `env.UX_BROWSER` is a member `UX_BROWSER`".to_owned(),
        ),
        (
            good.replace(
                "        run: |\n",
                "          UX_BROWSER: /other/checks\n        run: |\n",
            ),
            "docs-ux.yml: `jobs.ux.steps[0].env.UX_BROWSER` is a member `UX_BROWSER`".to_owned(),
        ),
    ] {
        let (installs, wrong) = steps("docs-ux.yml", &text);
        assert_eq!(installs, 1, "{text}");
        assert!(
            wrong.iter().any(|said| said.starts_with(&found)),
            "{found}: {wrong:#?}"
        );
    }
}

/// The guard reads every workflow file, and a form it does not read is found, not passed.
#[test]
fn another_workflow_file_is_read_as_well_and_a_form_the_guard_does_not_read_is_found() {
    let hand_over = "jobs:\n  check:\n    steps:\n      - run: echo \"UX_BROWSER=/other/checks\" >> \"$GITHUB_ENV\"\n";
    assert_eq!(
        steps("gate.yml", hand_over),
        (
            0,
            vec![
                "gate.yml: step `without a name` of job `check` names `UX_BROWSER`, which the install step alone hands over: echo \"UX_BROWSER=/other/checks\" >> \"$GITHUB_ENV\""
                    .to_owned()
            ]
        )
    );
    // The install step's form in another file, or in another job, is a step like any other there.
    let (installs, wrong) = steps("gate.yml", &install_step());
    assert_eq!((installs, wrong.is_empty()), (1, false), "{wrong:#?}");
    let (installs, wrong) = steps(
        "docs-ux.yml",
        &install_step().replace("  ux:\n", "  docs:\n"),
    );
    assert_eq!((installs, wrong.is_empty()), (1, false), "{wrong:#?}");
    for (text, found) in [
        (
            "name: A workflow\n",
            "other.yml: no mapping `jobs`; this guard reads the jobs of a workflow",
        ),
        (
            "jobs:\n  build:\n    runs-on: ubuntu-24.04\n",
            "other.yml: job `build` has neither a list `steps` nor a `uses`; this guard reads no other job",
        ),
        (
            "jobs:\n  build:\n    uses: ./.github/workflows/other.yml\n    steps: none\n",
            "other.yml: job `build` has neither a list `steps` nor a `uses`; this guard reads no other job",
        ),
        (
            "jobs:\n  build:\n    steps:\n      - echo\n",
            "other.yml: step 0 of job `build` is no mapping; this guard reads no other step",
        ),
    ] {
        assert_eq!(
            steps("other.yml", text),
            (0, vec![found.to_owned()]),
            "{text}"
        );
    }
    assert_eq!(
        steps(
            "other.yml",
            "jobs:\n  build:\n    uses: ./.github/workflows/other.yml\n"
        ),
        (0, Vec::new())
    );
}

/// The invariant of `AGENTS.md` says that no workflow text outside the install step names a
/// `ux-browser`, a release of it or `UX_BROWSER`, and names this file's guards for it. Three
/// workflows that hand the harness another binary than the pinned one by name, each read by the
/// guards as they read `docs-ux.yml`: [`pin`] for the install step, [`steps`] for every other
/// step.
#[test]
fn a_workflow_that_fetches_or_hands_over_another_ux_browser_is_found() {
    let digest = "00000000000000000000000000000000000000000000000000000000000000aa";
    let mut install = format!(
        "jobs:\n  ux:\n    steps:\n      - name: Install ux-browser\n        env:\n          UX_BROWSER_VERSION: \"7.8.9\"\n          UX_BROWSER_SHA256: {digest}\n        run: |\n          set -euo pipefail\n          cd \"$RUNNER_TEMP\"\n"
    );
    for (line, _) in SCRIPT {
        install.push_str("          ");
        install.push_str(line);
        install.push('\n');
    }
    assert_eq!(pin(&install), Ok(("7.8.9".to_owned(), digest.to_owned())));
    assert_eq!(steps("docs-ux.yml", &install), (1, Vec::new()));

    let found = |text: &str| pin(text).is_err() || !steps("docs-ux.yml", text).1.is_empty();
    let mut missed = Vec::new();
    for (what, added) in [
        (
            "a second step downloads the asset of another release",
            "      - name: Another release\n        run: |\n          curl --fail --location --output \"$RUNNER_TEMP/other.tar.gz\" https://github.com/beyond10x/uxui/releases/download/7.9.0/ux-browser-7.9.0-x86_64-unknown-linux-gnu.tar.gz\n          tar -xzf \"$RUNNER_TEMP/other.tar.gz\" -C \"$RUNNER_TEMP\" --strip-components=1\n",
        ),
        (
            "a later step hands the harness another binary as UX_BROWSER",
            "      - name: Hand over\n        run: echo \"UX_BROWSER=$RUNNER_TEMP/other/checks\" >> \"$GITHUB_ENV\"\n",
        ),
        (
            "the install step itself hands another binary over after the pinned one",
            "          echo \"UX_BROWSER=$RUNNER_TEMP/other/checks\" >> \"$GITHUB_ENV\"\n",
        ),
    ] {
        if !found(&format!("{install}{added}")) {
            missed.push(what);
        }
    }
    assert!(
        missed.is_empty(),
        "neither `pin` nor `steps` finds: {missed:#?}"
    );
}

/// A step that uses an action of this repository runs the steps of that action's `action.yml`, a
/// file no guard of this file opens. The workflow's own text names no `ux-browser`, and the
/// action can fetch one and hand it over: so the step is found, in every workflow file, and a
/// job that uses a workflow file of this repository is not, since [`steps`] reads that file.
#[test]
fn a_step_that_uses_an_action_of_this_repository_is_found() {
    let text = format!(
        "{}      - name: Page checks\n        uses: ./.github/actions/page-checks\n",
        install_step()
    );
    assert_eq!(pin(&text), Ok(("7.8.9".to_owned(), DIGEST.to_owned())));
    let (installs, found) = steps("docs-ux.yml", &text);
    assert_eq!(installs, 1);
    assert!(
        !found.is_empty(),
        "`steps` finds nothing in a step that runs `./.github/actions/page-checks`, an action whose steps no guard reads"
    );
    assert_eq!(
        found,
        vec![
            "docs-ux.yml: step `Page checks` of job `ux` uses `./.github/actions/page-checks`, an action of this repository; this guard reads no action file, so the step belongs in the workflow"
                .to_owned()
        ]
    );
    assert_eq!(
        steps(
            "gate.yml",
            "jobs:\n  check:\n    steps:\n      - uses: ./tools/setup\n"
        ),
        (
            0,
            vec![
                "gate.yml: step `without a name` of job `check` uses `./tools/setup`, an action of this repository; this guard reads no action file, so the step belongs in the workflow"
                    .to_owned()
            ]
        )
    );
    assert_eq!(
        steps(
            "gate.yml",
            "jobs:\n  check:\n    uses: ./.github/workflows/docs-ux.yml\n  other:\n    steps:\n      - uses: actions/checkout@0123456789abcdef0123456789abcdef01234567\n"
        ),
        (0, Vec::new())
    );
}

/// What `docs-ux.yml` alone is held to, each on a text with what the guard says: a step other
/// than the install step that names the runner's environment or path file, a member `defaults`
/// of the workflow or of a job, and a member `BASH_ENV` at each level. Another workflow file
/// holds the same forms and is passed.
#[test]
fn in_docs_ux_another_writer_of_the_runners_files_a_defaults_and_a_bash_env_are_found() {
    let good = install_step();
    assert_eq!(steps("docs-ux.yml", &good), (1, Vec::new()));
    let files =
        "in this workflow the install step alone writes the runner's environment and path files";
    let defaults = "has a member `defaults`, which can name the shell a script runs in; this guard reads no shell but the runner's own";
    let bash = "is a member `BASH_ENV`; bash reads the file it names before a script, and this guard does not read that file";
    for (text, found) in [
        (
            format!("{good}      - name: Append\n        run: cat ux/ci.env >> \"$GITHUB_ENV\"\n"),
            format!(
                "docs-ux.yml: step `Append` of job `ux` names `GITHUB_ENV`; {files}: cat ux/ci.env >> \"$GITHUB_ENV\""
            ),
        ),
        (
            format!(
                "{good}      - name: A path\n        run: echo \"$PWD/ux/bin\" >> \"${{GITHUB_PATH}}\"\n"
            ),
            format!(
                "docs-ux.yml: step `A path` of job `ux` names `GITHUB_PATH`; {files}: echo \"$PWD/ux/bin\" >> \"${{GITHUB_PATH}}\""
            ),
        ),
        (
            format!("defaults:\n  run:\n    shell: ux/ci-shell.sh {{0}}\n{good}"),
            format!("docs-ux.yml: the workflow {defaults}"),
        ),
        (
            good.replace(
                "  ux:\n",
                "  ux:\n    defaults:\n      run:\n        shell: ux/ci-shell.sh {0}\n",
            ),
            format!("docs-ux.yml: job `ux` {defaults}"),
        ),
        (
            format!("env:\n  BASH_ENV: ux/ci.env\n{good}"),
            format!("docs-ux.yml: `env.BASH_ENV` {bash}"),
        ),
        (
            good.replace("  ux:\n", "  ux:\n    env:\n      BASH_ENV: ux/ci.env\n"),
            format!("docs-ux.yml: `jobs.ux.env.BASH_ENV` {bash}"),
        ),
        (
            format!(
                "{good}      - name: The UX step\n        env:\n          BASH_ENV: ux/ci.env\n        run: npx playwright test\n"
            ),
            format!("docs-ux.yml: `jobs.ux.steps[1].env.BASH_ENV` {bash}"),
        ),
    ] {
        assert_eq!(steps("docs-ux.yml", &text), (1, vec![found]), "{text}");
    }
    let other = "defaults:\n  run:\n    shell: bash\nenv:\n  BASH_ENV: ci.env\njobs:\n  check:\n    defaults:\n      run:\n        working-directory: ux\n    steps:\n      - run: echo \"$RUNNER_TEMP/bin\" >> \"$GITHUB_PATH\"\n      - run: echo \"A=1\" >> \"$GITHUB_ENV\"\n";
    assert_eq!(steps("gate.yml", other), (0, Vec::new()));
}

/// Two reads of [`steps`]: a string of the workflow outside its jobs (a workflow-level `env` that
/// holds the release address under another name), and the uxui repository written in another
/// case, which the forge takes as the same repository.
#[test]
fn a_release_address_outside_the_jobs_or_in_another_case_is_found() {
    let good = install_step();
    assert_eq!(steps("docs-ux.yml", &good), (1, Vec::new()));
    let release = "names a release of `ux-browser` or the uxui repository";
    let outside = format!(
        "env:\n  CHECKS_RELEASE: https://github.com/beyond10x/uxui/releases/download/7.9.0\n{good}"
    );
    assert_eq!(
        steps("docs-ux.yml", &outside),
        (
            1,
            vec![format!(
                "docs-ux.yml: the workflow outside its jobs {release}: https://github.com/beyond10x/uxui/releases/download/7.9.0"
            )]
        )
    );
    let cased = format!(
        "{good}      - name: Another release\n        run: gh release download 7.9.0 --repo Beyond10x/UXUI\n"
    );
    assert_eq!(
        steps("docs-ux.yml", &cased),
        (
            1,
            vec![format!(
                "docs-ux.yml: step `Another release` of job `ux` {release}: gh release download 7.9.0 --repo Beyond10x/UXUI"
            )]
        )
    );
}
