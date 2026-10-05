//! story:secrets-resolver: core crates gain no dependency on the `secrets` library. Every crate
//! from its git source is reached only through `b10x-llm-credentials` feature `secrets`,
//! optional, off by default, pinned to one release tag whose commit the lockfile records, and no
//! core crate forwards that feature from its own `[features]`. Read from the manifests at run
//! time; each rule has a mutant below that shows it can fail.
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};
use toml::{Table, Value};

/// The library crates `b10x-llm-credentials` declares. A crate counts as the library's when its
/// git source is the library repository in any spelling, or when it carries one of these names.
const LIBRARY_CRATES: [&str; 2] = ["secrets-core", "secrets-keychain"];
const SOURCE: &str = "https://github.com/beyond10x/secrets";
/// The repository path every spelling of the library source reduces to.
const REPOSITORY: &str = "beyond10x/secrets";
const TAG: &str = "v0.5.0";
const REVISION: &str = "8c4eabb15e719fea1a770898bd6852e381900fb0";
const OWNER: &str = "crates/llm-credentials";
const CREDENTIALS: &str = "b10x-llm-credentials";
/// Not core crates: the conformance harness observes the adapter through the feature.
const FEATURE_USERS: [&str; 1] = ["checks/conformance"];
const DEPENDENCY_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

fn root() -> PathBuf {
    PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..")
}

fn read(path: &Path) -> Table {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
        .parse()
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// Every dependency table of a manifest, with the table's name.
fn dependency_tables(manifest: &Table) -> Vec<(String, &Table)> {
    let mut tables = Vec::new();
    for name in DEPENDENCY_TABLES {
        if let Some(table) = manifest.get(name).and_then(Value::as_table) {
            tables.push((name.to_owned(), table));
        }
    }
    if let Some(targets) = manifest.get("target").and_then(Value::as_table) {
        for (cfg, target) in targets {
            for name in DEPENDENCY_TABLES {
                if let Some(table) = target.get(name).and_then(Value::as_table) {
                    tables.push((format!("target.{cfg}.{name}"), table));
                }
            }
        }
    }
    tables
}

/// The workspace entry a `workspace = true` entry inherits, or the entry itself.
fn inherited<'a>(key: &str, entry: &'a Value, workspace: &'a Table) -> &'a Value {
    if entry.get("workspace").and_then(Value::as_bool) == Some(true) {
        workspace.get(key).unwrap_or(entry)
    } else {
        entry
    }
}

/// The package a dependency entry names: its `package` key, or the workspace entry's, or its key.
fn package<'a>(key: &'a str, entry: &'a Value, workspace: &'a Table) -> &'a str {
    entry
        .get("package")
        .or_else(|| inherited(key, entry, workspace).get("package"))
        .and_then(Value::as_str)
        .unwrap_or(key)
}

/// Whether a git URL names the library repository, in any spelling: `https://`, `ssh://git@`,
/// `git@host:`, with or without `.git`, a trailing `/` or a different case.
fn is_library_source(url: &str) -> bool {
    let url = url.trim().to_ascii_lowercase();
    let url = url.split(['?', '#']).next().unwrap_or_default();
    let Some((_, path)) = url.split_once("github.com") else {
        return false;
    };
    let path = path.trim_start_matches([':', '/']).trim_end_matches('/');
    path.strip_suffix(".git").unwrap_or(path) == REPOSITORY
}

/// The library crate a dependency entry reaches, if it reaches one.
fn library_crate<'a>(key: &'a str, entry: &'a Value, workspace: &'a Table) -> Option<&'a str> {
    let name = package(key, entry, workspace);
    let from_source = inherited(key, entry, workspace)
        .get("git")
        .and_then(Value::as_str)
        .is_some_and(is_library_source);
    (from_source || LIBRARY_CRATES.contains(&name)).then_some(name)
}

fn enables_secrets(entry: &Value) -> bool {
    entry
        .get("features")
        .and_then(Value::as_array)
        .is_some_and(|features| features.iter().any(|f| f.as_str() == Some("secrets")))
}

/// Every breach of the boundary in the workspace at `root`; empty when it holds.
fn violations(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let workspace_manifest = read(&root.join("Cargo.toml"));
    let workspace = &workspace_manifest["workspace"];
    let shared = workspace
        .get("dependencies")
        .and_then(Value::as_table)
        .cloned()
        .unwrap_or_default();
    let empty = Table::new();
    for (key, entry) in &shared {
        if let Some(name) = library_crate(key, entry, &empty) {
            found.push(format!("workspace.dependencies names {name}"));
        }
        if package(key, entry, &empty) == CREDENTIALS && enables_secrets(entry) {
            found.push("workspace.dependencies enables llm-credentials feature secrets".into());
        }
    }
    let members: Vec<&str> = workspace["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|member| member.as_str().unwrap())
        .collect();
    if !members.contains(&OWNER) {
        found.push(format!("{OWNER} is not a workspace member"));
    }
    // The owner's library dependencies: their keys (what a feature item names) and crates.
    let mut declared_keys = BTreeSet::new();
    let mut declared = BTreeSet::new();
    for member in &members {
        let manifest = read(&root.join(member).join("Cargo.toml"));
        let mut credential_keys = Vec::new();
        for (table_name, table) in dependency_tables(&manifest) {
            for (key, entry) in table {
                if package(key, entry, &shared) == CREDENTIALS {
                    credential_keys.push(key.clone());
                    if enables_secrets(entry) && !FEATURE_USERS.contains(member) {
                        found.push(format!(
                            "{member} [{table_name}] enables llm-credentials feature secrets"
                        ));
                    }
                }
                let Some(name) = library_crate(key, entry, &shared) else {
                    continue;
                };
                if *member != OWNER {
                    found.push(format!("{member} [{table_name}] depends on {name}"));
                    continue;
                }
                if table_name != "dependencies" {
                    found.push(format!("{member} names {name} in [{table_name}]"));
                    continue;
                }
                if entry.get("optional").and_then(Value::as_bool) != Some(true) {
                    found.push(format!("{member}: {name} is not optional"));
                }
                if entry.get("git").and_then(Value::as_str) != Some(SOURCE)
                    || entry.get("tag").and_then(Value::as_str) != Some(TAG)
                    || entry.get("rev").is_some()
                    || entry.get("branch").is_some()
                {
                    found.push(format!("{member}: {name} is not pinned to {SOURCE} {TAG}"));
                }
                declared_keys.insert(key.clone());
                declared.insert(name.to_owned());
            }
        }
        if *member == OWNER {
            check_features(&manifest, &declared_keys, &mut found);
        } else if !FEATURE_USERS.contains(member) {
            check_forwarding(member, &manifest, &credential_keys, &mut found);
        }
    }
    for name in &declared {
        if !LIBRARY_CRATES.contains(&name.as_str()) {
            found.push(format!(
                "{OWNER} declares {name}, which is not one of {LIBRARY_CRATES:?}"
            ));
        }
    }
    if declared.len() != LIBRARY_CRATES.len() {
        found.push(format!(
            "{OWNER} declares {} of the {} library crates",
            declared.len(),
            LIBRARY_CRATES.len()
        ));
    }
    check_lock(root, &mut found);
    found
}

/// A core crate's own feature that turns on `llm-credentials/secrets`, also weakly (`?/`).
fn check_forwarding(
    member: &str,
    manifest: &Table,
    credential_keys: &[String],
    found: &mut Vec<String>,
) {
    let Some(features) = manifest.get("features").and_then(Value::as_table) else {
        return;
    };
    for (feature, items) in features {
        for item in items
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let forwards = credential_keys
                .iter()
                .any(|key| item == format!("{key}/secrets") || item == format!("{key}?/secrets"));
            if forwards {
                found.push(format!("{member} feature {feature} forwards {item}"));
            }
        }
    }
}

/// `secrets` enables exactly the library crates; no other feature and not `default` does.
fn check_features(manifest: &Table, declared: &BTreeSet<String>, found: &mut Vec<String>) {
    let features = manifest
        .get("features")
        .and_then(Value::as_table)
        .cloned()
        .unwrap_or_default();
    let list = |name: &str| -> Vec<String> {
        features
            .get(name)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    if !features.contains_key("secrets") {
        found.push(format!("{OWNER} has no feature secrets"));
    }
    for crate_name in LIBRARY_CRATES {
        if !list("secrets").contains(&format!("dep:{crate_name}")) {
            found.push(format!("feature secrets does not enable {crate_name}"));
        }
    }
    if list("default").iter().any(|item| item == "secrets") {
        found.push("feature secrets is on by default".into());
    }
    for (feature, _) in &features {
        if feature == "secrets" {
            continue;
        }
        for item in list(feature) {
            let target = item.trim_start_matches("dep:");
            let target = target.split(['/', '?']).next().unwrap_or_default();
            if declared.contains(target) || item == "secrets" {
                found.push(format!(
                    "feature {feature} reaches the library through {item}"
                ));
            }
        }
    }
}

/// The lockfile records every package from the library source at the tag's own commit, and
/// holds each declared crate.
fn check_lock(root: &Path, found: &mut Vec<String>) {
    let lock = read(&root.join("Cargo.lock"));
    let expected = format!("git+{SOURCE}?tag={TAG}#{REVISION}");
    let packages = lock["package"].as_array().unwrap();
    for crate_name in LIBRARY_CRATES {
        if !packages
            .iter()
            .any(|package| package["name"].as_str() == Some(crate_name))
        {
            found.push(format!("Cargo.lock has no {crate_name}"));
        }
    }
    for package in packages {
        let name = package["name"].as_str().unwrap_or_default();
        let source = package.get("source").and_then(Value::as_str).unwrap_or("");
        let from_library = source.strip_prefix("git+").is_some_and(is_library_source);
        if (from_library || LIBRARY_CRATES.contains(&name)) && source != expected {
            found.push(format!("Cargo.lock records {name} from {source}"));
        }
    }
}

#[test]
fn the_secrets_library_is_reached_only_through_the_credentials_feature() {
    let found = violations(&root());
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn every_spelling_of_the_library_source_is_recognised() {
    for url in [
        "https://github.com/beyond10x/secrets",
        "https://github.com/beyond10x/secrets.git",
        "https://github.com/beyond10x/secrets/",
        "https://GitHub.com/Beyond10x/Secrets",
        "ssh://git@github.com/beyond10x/secrets",
        "git@github.com:beyond10x/secrets.git",
        "https://github.com/beyond10x/secrets?tag=v0.5.0#8c4eabb1",
    ] {
        assert!(is_library_source(url), "{url}");
    }
    for url in [
        "https://github.com/beyond10x/secrets-fork",
        "https://github.com/beyond10x/llm",
        "https://example.invalid/beyond10x/secrets",
    ] {
        assert!(!is_library_source(url), "{url}");
    }
}

/// A copy of every manifest and the lockfile, for one mutant.
fn copy(name: &str) -> PathBuf {
    let source = root();
    let copy = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("secrets-boundary-{name}"));
    let _ = fs::remove_dir_all(&copy);
    let workspace = read(&source.join("Cargo.toml"));
    let members = workspace["workspace"]["members"].as_array().unwrap();
    for path in ["Cargo.toml", "Cargo.lock"]
        .into_iter()
        .map(PathBuf::from)
        .chain(
            members
                .iter()
                .map(|member| Path::new(member.as_str().unwrap()).join("Cargo.toml")),
        )
    {
        let to = copy.join(&path);
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        fs::copy(source.join(&path), to).unwrap();
    }
    copy
}

/// Replaces `from` with `to` in one copied file; the mutant must actually change it.
fn mutate(root: &Path, file: &str, from: &str, to: &str) {
    let path = root.join(file);
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains(from), "{file} no longer holds {from:?}");
    fs::write(&path, text.replacen(from, to, 1)).unwrap();
}

fn assert_named(root: &Path, needle: &str) {
    let found = violations(root);
    assert!(
        found.iter().any(|violation| violation.contains(needle)),
        "expected a violation naming {needle:?}, got {found:#?}"
    );
}

#[test]
fn a_core_crate_that_depends_on_the_library_is_named() {
    let root = copy("core-dependency");
    mutate(
        &root,
        "crates/llm-core/Cargo.toml",
        "[dependencies]\n",
        "[dependencies]\nsecrets-core = { git = \"https://github.com/beyond10x/secrets\", tag = \"v0.5.0\" }\n",
    );
    assert_named(
        &root,
        "crates/llm-core [dependencies] depends on secrets-core",
    );
}

#[test]
fn a_core_crate_that_enables_the_feature_is_named() {
    let root = copy("core-feature");
    mutate(
        &root,
        "crates/llm-providers/Cargo.toml",
        "llm-credentials.workspace = true",
        "llm-credentials = { workspace = true, features = [\"secrets\"] }",
    );
    assert_named(
        &root,
        "crates/llm-providers [dependencies] enables llm-credentials feature secrets",
    );
}

#[test]
fn a_required_library_dependency_is_named() {
    let root = copy("not-optional");
    mutate(
        &root,
        "crates/llm-credentials/Cargo.toml",
        "tag = \"v0.5.0\", optional = true }\nsecrets-keychain",
        "tag = \"v0.5.0\" }\nsecrets-keychain",
    );
    assert_named(&root, "secrets-core is not optional");
}

#[test]
fn a_moved_tag_is_named() {
    let root = copy("moved-tag");
    mutate(
        &root,
        "crates/llm-credentials/Cargo.toml",
        "secrets-keychain = { git = \"https://github.com/beyond10x/secrets\", tag = \"v0.5.0\"",
        "secrets-keychain = { git = \"https://github.com/beyond10x/secrets\", tag = \"v0.4.1\"",
    );
    assert_named(&root, "secrets-keychain is not pinned");
}

#[test]
fn a_lockfile_at_another_commit_is_named() {
    let root = copy("moved-lock");
    mutate(
        &root,
        "Cargo.lock",
        REVISION,
        "0000000000000000000000000000000000000000",
    );
    assert_named(&root, "Cargo.lock records secrets-");
}

#[test]
fn a_default_on_feature_is_named() {
    let root = copy("default-on");
    mutate(
        &root,
        "crates/llm-credentials/Cargo.toml",
        "default = []",
        "default = [\"secrets\"]",
    );
    assert_named(&root, "feature secrets is on by default");
}

#[test]
fn a_core_crate_on_another_library_crate_is_named() {
    let root = copy("other-library-crate");
    mutate(
        &root,
        "crates/llm-providers/Cargo.toml",
        "[dependencies]\n",
        "[dependencies]\nsecrets-client = { git = \"https://github.com/beyond10x/secrets\", tag = \"v0.5.0\" }\n",
    );
    assert_named(
        &root,
        "crates/llm-providers [dependencies] depends on secrets-client",
    );
}

#[test]
fn a_renamed_library_crate_under_another_spelling_of_the_source_is_named() {
    let root = copy("renamed-spelling");
    mutate(
        &root,
        "crates/llm-routing/Cargo.toml",
        "[dependencies]\n",
        "[dependencies]\nvault = { package = \"secrets-remote\", git = \"ssh://git@github.com/Beyond10x/secrets.git\", tag = \"v0.5.0\" }\n",
    );
    assert_named(
        &root,
        "crates/llm-routing [dependencies] depends on secrets-remote",
    );
}

#[test]
fn a_core_crate_inheriting_a_library_crate_from_the_workspace_is_named() {
    let root = copy("workspace-inherited");
    mutate(
        &root,
        "Cargo.toml",
        "[workspace.dependencies]\n",
        "[workspace.dependencies]\nsecrets-federation = { git = \"https://github.com/beyond10x/secrets\", tag = \"v0.5.0\" }\n",
    );
    mutate(
        &root,
        "crates/llm-chat/Cargo.toml",
        "[dependencies]\n",
        "[dependencies]\nsecrets-federation.workspace = true\n",
    );
    assert_named(&root, "workspace.dependencies names secrets-federation");
    assert_named(
        &root,
        "crates/llm-chat [dependencies] depends on secrets-federation",
    );
}

#[test]
fn a_library_crate_in_a_target_table_of_a_core_crate_is_named() {
    let root = copy("target-table");
    mutate(
        &root,
        "crates/llm-core/Cargo.toml",
        "[dependencies]\n",
        "[target.'cfg(unix)'.dev-dependencies]\nsecrets-app = { git = \"https://github.com/beyond10x/secrets\", tag = \"v0.5.0\" }\n\n[dependencies]\n",
    );
    assert_named(
        &root,
        "crates/llm-core [target.cfg(unix).dev-dependencies] depends on secrets-app",
    );
}

#[test]
fn a_core_crate_forwarding_the_feature_is_named() {
    let root = copy("forwarded");
    mutate(
        &root,
        "crates/llm-providers/Cargo.toml",
        "[lints]",
        "[features]\nsecrets = [\"llm-credentials/secrets\"]\n\n[lints]",
    );
    assert_named(
        &root,
        "crates/llm-providers feature secrets forwards llm-credentials/secrets",
    );
}

#[test]
fn a_core_crate_forwarding_the_feature_weakly_is_named() {
    let root = copy("forwarded-weakly");
    mutate(
        &root,
        "crates/llm-messages/Cargo.toml",
        "[lints]",
        "[features]\nvault = [\"llm-credentials?/secrets\"]\n\n[lints]",
    );
    assert_named(
        &root,
        "crates/llm-messages feature vault forwards llm-credentials?/secrets",
    );
}

#[test]
fn an_extra_library_crate_in_the_credentials_crate_is_named() {
    let root = copy("owner-extra");
    mutate(
        &root,
        "crates/llm-credentials/Cargo.toml",
        "[dependencies]\n",
        "[dependencies]\nsecrets-client = { git = \"https://github.com/beyond10x/secrets\", tag = \"v0.5.0\", optional = true }\n",
    );
    assert_named(&root, "declares secrets-client, which is not one of");
}

#[test]
fn another_feature_reaching_a_library_crate_is_named() {
    let root = copy("other-feature");
    mutate(
        &root,
        "crates/llm-credentials/Cargo.toml",
        "default = []",
        "default = []\nvault = [\"secrets-keychain/native-keychain\"]",
    );
    assert_named(
        &root,
        "feature vault reaches the library through secrets-keychain/native-keychain",
    );
}
