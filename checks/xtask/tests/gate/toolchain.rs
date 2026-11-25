//! One Rust toolchain for the local gate and for CI, held to the workspace rule "The local gate
//! equals the forge": `rust-toolchain.toml` pins one exact release, every job that runs `cargo`
//! installs what that file names, and no workflow and no task names a toolchain of its own.

use super::{read, repository};
use serde_yaml::Value;

/// The file rustup reads in every directory of the repository.
const FILE: &str = "rust-toolchain.toml";

/// The step a job runs to install what [`FILE`] names: with no argument, rustup installs the
/// toolchain that is active in the directory, with the components the file lists.
const INSTALL: &str = "rustup toolchain install";

/// The channel and the components [`FILE`] names.
fn pin() -> (String, Vec<String>) {
    let file: toml::Value = toml::from_str(&read(FILE)).expect("rust-toolchain.toml is TOML");
    let toolchain = &file["toolchain"];
    let channel = toolchain["channel"]
        .as_str()
        .expect("the channel is a string")
        .to_owned();
    let components = toolchain["components"]
        .as_array()
        .expect("the components are a list")
        .iter()
        .map(|each| each.as_str().expect("a component is a string").to_owned())
        .collect();
    (channel, components)
}

/// The three numbers of `version` when it is an exact release such as `1.98.0`, and nothing for a
/// channel name, a date or a version of two numbers, each of which moves without a commit.
fn exact(version: &str) -> Option<[u64; 3]> {
    let mut numbers = version.split('.').map(|part| {
        let plain = !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
        plain.then(|| part.parse::<u64>().ok()).flatten()
    });
    let read = [numbers.next()??, numbers.next()??, numbers.next()??];
    numbers.next().is_none().then_some(read)
}

#[test]
fn the_file_pins_one_exact_release_with_clippy_and_rustfmt() {
    let (channel, components) = pin();
    let [major, minor, _] =
        exact(&channel).unwrap_or_else(|| panic!("{FILE} pins `{channel}`, not an exact release"));
    assert_eq!(components, ["clippy", "rustfmt"], "{FILE}");
    // The pinned release compiles what the manifest says the workspace needs at least.
    let manifest: toml::Value = toml::from_str(&read("Cargo.toml")).expect("Cargo.toml is TOML");
    let least = manifest["workspace"]["package"]["rust-version"]
        .as_str()
        .expect("the workspace names its rust-version");
    let mut least = least.split('.').map(|part| part.parse::<u64>().expect("a number"));
    let least = (least.next().expect("a major"), least.next().expect("a minor"));
    assert!(
        (major, minor) >= least,
        "{FILE} pins {channel}, older than the manifest's rust-version {least:?}"
    );
}

#[test]
fn only_an_exact_release_is_read_as_one() {
    assert_eq!(exact("1.98.0"), Some([1, 98, 0]));
    assert_eq!(exact("1.100.12"), Some([1, 100, 12]));
    for moving in [
        "stable",
        "nightly",
        "1.98",
        "1",
        "1.98.0.1",
        "1.98.x",
        "1.98.",
        ".98.0",
        "1.98.0-beta",
        "nightly-2026-11-01",
        "+1.98.0",
        "",
    ] {
        assert_eq!(exact(moving), None, "`{moving}`");
    }
}

/// The programs a `+<toolchain>` word after them hands to rustup.
const PROXIES: [&str; 5] = ["cargo", "rustc", "rustfmt", "rustdoc", "cargo-clippy"];

/// Every way `text`, the text of a workflow or of a Taskfile, names a toolchain of its own: one
/// entry for each line that does, with what was found. The step [`INSTALL`] names none.
fn own_toolchains(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in text.lines() {
        let mut say = |what: &str| found.push(format!("{what}: {}", line.trim()));
        if line.contains("RUSTUP_TOOLCHAIN") {
            say("the variable that overrides the file");
        }
        if line.contains("rust-toolchain@") {
            say("an action that installs a toolchain it is handed");
        }
        if line.trim_start().starts_with("toolchain:") {
            say("a toolchain handed to an action");
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        for pair in words.windows(2) {
            let program = pair[0].trim_start_matches(['-', '"', '\'', '(']);
            if PROXIES.contains(&program) && pair[1].starts_with('+') {
                say("a toolchain named after a command");
            }
        }
        for (at, word) in words.iter().enumerate() {
            if word.trim_start_matches(['-', '"', '\'', '(']) != "rustup" {
                continue;
            }
            let rest: Vec<&str> = words
                .iter()
                .skip(at + 1)
                .map(|word| word.trim_end_matches(['"', '\'', ')', ';']))
                .take_while(|word| !["&&", "||", "|", ""].contains(word))
                .collect();
            if rest != ["toolchain", "install"] {
                say("a rustup command other than the install of the file's toolchain");
            }
        }
    }
    found
}

#[test]
fn each_way_to_name_a_toolchain_beside_the_file_is_found() {
    let clean = "steps:\n  - name: Install the Rust toolchain rust-toolchain.toml pins\n    run: rustup toolchain install\n  - run: |\n      cargo fmt --all --check\n      cargo clippy --workspace -- -D warnings\n      rustup toolchain install && cargo test\n";
    assert_eq!(own_toolchains(clean), Vec::<String>::new());
    for (line, found) in [
        (
            "      - cargo +1.98.0 clippy --workspace",
            "a toolchain named after a command: - cargo +1.98.0 clippy --workspace",
        ),
        (
            "        run: rustfmt +nightly --check src/lib.rs",
            "a toolchain named after a command: run: rustfmt +nightly --check src/lib.rs",
        ),
        (
            "          RUSTUP_TOOLCHAIN: 1.99.1",
            "the variable that overrides the file: RUSTUP_TOOLCHAIN: 1.99.1",
        ),
        (
            "      - uses: dtolnay/rust-toolchain@4360b52568e2003a75bf9bc1d59f33a8e3fc893c",
            "an action that installs a toolchain it is handed: - uses: dtolnay/rust-toolchain@4360b52568e2003a75bf9bc1d59f33a8e3fc893c",
        ),
        (
            "          toolchain: 1.98.0",
            "a toolchain handed to an action: toolchain: 1.98.0",
        ),
        (
            "        run: rustup toolchain install 1.98.0",
            "a rustup command other than the install of the file's toolchain: run: rustup toolchain install 1.98.0",
        ),
        (
            "        run: rustup default stable",
            "a rustup command other than the install of the file's toolchain: run: rustup default stable",
        ),
        (
            "        run: rustup override set 1.99.1 && cargo test",
            "a rustup command other than the install of the file's toolchain: run: rustup override set 1.99.1 && cargo test",
        ),
        (
            "        run: rustup run 1.99.1 cargo test",
            "a rustup command other than the install of the file's toolchain: run: rustup run 1.99.1 cargo test",
        ),
    ] {
        assert_eq!(own_toolchains(line), [found], "`{line}`");
    }
}

/// The workflows of the repository, each with its file name.
fn workflows() -> Vec<(String, String)> {
    let mut workflows = Vec::new();
    let directory = repository().join(".github/workflows");
    for entry in std::fs::read_dir(&directory).expect("the workflows are listed") {
        let name = entry.expect("a directory entry").file_name();
        let name = name.to_str().expect("a workflow's name is UTF-8").to_owned();
        if name.ends_with(".yml") || name.ends_with(".yaml") {
            let text = read(&format!(".github/workflows/{name}"));
            workflows.push((name, text));
        }
    }
    workflows.sort();
    workflows
}

#[test]
fn no_workflow_and_no_task_names_a_toolchain_of_its_own() {
    let mut texts = workflows();
    assert!(texts.len() >= 2, "the workflows are read: {}", texts.len());
    texts.push(("Taskfile.yml".to_owned(), read("Taskfile.yml")));
    for (name, text) in texts {
        assert_eq!(own_toolchains(&text), Vec::<String>::new(), "{name}");
    }
}

/// The jobs of the workflow `text` that run `cargo` in a step without a step before it that is
/// [`INSTALL`] and nothing else: each with the name of the step that runs `cargo` first.
fn cargo_before_the_install(text: &str) -> Vec<String> {
    let workflow: Value = serde_yaml::from_str(text).expect("a workflow is YAML");
    let mut found = Vec::new();
    let Some(jobs) = workflow["jobs"].as_mapping() else {
        return found;
    };
    for (job, body) in jobs {
        let job = job.as_str().unwrap_or_default();
        let steps: Vec<&Value> = body["steps"].as_sequence().into_iter().flatten().collect();
        let runs_cargo = |step: &&Value| {
            step["run"].as_str().is_some_and(|run| {
                run.split_whitespace()
                    .any(|word| word.trim_start_matches(['"', '\'', '(']) == "cargo")
            })
        };
        let Some(first) = steps.iter().position(runs_cargo) else {
            continue;
        };
        let installed = steps[..first]
            .iter()
            .any(|step| step["run"].as_str().map(str::trim) == Some(INSTALL));
        if !installed {
            let step = steps[first]["name"].as_str().unwrap_or("a step without a name");
            found.push(format!("job {job}: `{step}` runs cargo before `{INSTALL}`"));
        }
    }
    found
}

#[test]
fn every_job_that_runs_cargo_installs_the_files_toolchain_first() {
    let mut jobs_with_the_install = 0;
    for (name, text) in workflows() {
        assert_eq!(cargo_before_the_install(&text), Vec::<String>::new(), "{name}");
        jobs_with_the_install += text
            .lines()
            .filter(|line| line.trim() == format!("run: {INSTALL}"))
            .count();
    }
    // The gate's two jobs that compile and the job that publishes the pages.
    assert_eq!(jobs_with_the_install, 3, "steps `{INSTALL}` over all workflows");
}

#[test]
fn a_job_that_runs_cargo_without_the_install_before_it_is_found() {
    let with = "jobs:\n  build:\n    steps:\n      - run: rustup toolchain install\n      - name: Tests\n        run: cargo test\n  prose:\n    steps:\n      - run: echo no rust here\n";
    assert_eq!(cargo_before_the_install(with), Vec::<String>::new());
    let without = with.replace("      - run: rustup toolchain install\n", "");
    assert_eq!(
        cargo_before_the_install(&without),
        ["job build: `Tests` runs cargo before `rustup toolchain install`"]
    );
    let after = "jobs:\n  build:\n    steps:\n      - name: Tests\n        run: |\n          cargo fmt --all --check\n          cargo test\n      - run: rustup toolchain install\n";
    assert_eq!(
        cargo_before_the_install(after),
        ["job build: `Tests` runs cargo before `rustup toolchain install`"]
    );
    let other = with.replace("rustup toolchain install", "rustup toolchain install 1.98.0");
    assert_eq!(
        cargo_before_the_install(&other),
        ["job build: `Tests` runs cargo before `rustup toolchain install`"]
    );
}
