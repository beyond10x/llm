//! story:secrets-resolver: core crates gain no dependency on the `secrets` library. Its crates
//! are reached only through `b10x-llm-credentials` feature `secrets`, optional, off by default,
//! pinned to one release tag whose commit the lockfile records. Read from the manifests at run
//! time; each rule has a mutant below that shows it can fail.
use std::{
    fs,
    path::{Path, PathBuf},
};
use toml::{Table, Value};

const LIBRARY_CRATES: [&str; 2] = ["secrets-core", "secrets-keychain"];
const SOURCE: &str = "https://github.com/beyond10x/secrets";
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

/// The package a dependency entry names: its `package` key, or the workspace entry's, or its key.
fn package<'a>(key: &'a str, entry: &'a Value, workspace: &'a Table) -> &'a str {
    if let Some(package) = entry.get("package").and_then(Value::as_str) {
        return package;
    }
    if entry.get("workspace").and_then(Value::as_bool) == Some(true)
        && let Some(package) = workspace
            .get(key)
            .and_then(|entry| entry.get("package"))
            .and_then(Value::as_str)
    {
        return package;
    }
    key
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
        let name = package(key, entry, &empty);
        if LIBRARY_CRATES.contains(&name) {
            found.push(format!("workspace.dependencies names {name}"));
        }
        if name == CREDENTIALS && enables_secrets(entry) {
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
    let mut pinned = 0;
    for member in &members {
        let manifest = read(&root.join(member).join("Cargo.toml"));
        for (table_name, table) in dependency_tables(&manifest) {
            for (key, entry) in table {
                let name = package(key, entry, &shared);
                if LIBRARY_CRATES.contains(&name) {
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
                    pinned += 1;
                }
                if name == CREDENTIALS && enables_secrets(entry) && !FEATURE_USERS.contains(member)
                {
                    found.push(format!(
                        "{member} [{table_name}] enables llm-credentials feature secrets"
                    ));
                }
            }
        }
        if *member == OWNER {
            check_features(&manifest, &mut found);
        }
    }
    if pinned != LIBRARY_CRATES.len() {
        found.push(format!(
            "{OWNER} declares {pinned} of the {} library crates",
            LIBRARY_CRATES.len()
        ));
    }
    check_lock(root, &mut found);
    found
}

/// `secrets` enables exactly the library crates; no other feature and not `default` does.
fn check_features(manifest: &Table, found: &mut Vec<String>) {
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
            if LIBRARY_CRATES
                .iter()
                .any(|crate_name| item.trim_start_matches("dep:").starts_with(crate_name))
                || item == "secrets"
            {
                found.push(format!(
                    "feature {feature} reaches the library through {item}"
                ));
            }
        }
    }
}

/// The lockfile records every library crate at the tag's own commit.
fn check_lock(root: &Path, found: &mut Vec<String>) {
    let lock = read(&root.join("Cargo.lock"));
    let expected = format!("git+{SOURCE}?tag={TAG}#{REVISION}");
    for crate_name in LIBRARY_CRATES {
        let sources: Vec<&str> = lock["package"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|package| package["name"].as_str() == Some(crate_name))
            .map(|package| package.get("source").and_then(Value::as_str).unwrap_or(""))
            .collect();
        if sources.is_empty() {
            found.push(format!("Cargo.lock has no {crate_name}"));
        }
        for source in sources {
            if source != expected {
                found.push(format!("Cargo.lock records {crate_name} from {source}"));
            }
        }
    }
}

#[test]
fn the_secrets_library_is_reached_only_through_the_credentials_feature() {
    let found = violations(&root());
    assert!(found.is_empty(), "{found:#?}");
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
