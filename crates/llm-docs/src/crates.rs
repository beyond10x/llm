//! The crate reference, from `cargo metadata --no-deps`.

use std::{fmt::Write as _, path::Path, process::Command};

use serde_json::Value;

use crate::{HEADER, Result, yaml_string};

/// `cargo metadata --no-deps` for the workspace at `root`.
pub(crate) fn metadata(root: &Path) -> Result<Value> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
        ])
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"))
        .output()
        .map_err(|error| format!("running cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("parsing cargo metadata: {error}"))
}

/// One workspace package as the page lists it.
struct Package {
    name: String,
    library: Option<String>,
    binaries: Vec<String>,
    path: String,
    description: Option<String>,
    /// Feature name, the features it turns on, and the optional dependencies it adds.
    features: Vec<(String, Vec<String>, Vec<String>)>,
    defaults: Vec<String>,
}

fn text(value: &Value, key: &str) -> Result<String> {
    value[key]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("cargo metadata: {key} is not a string"))
}

fn package(value: &Value, root: &str) -> Result<Package> {
    let name = text(value, "name")?;
    let manifest = text(value, "manifest_path")?;
    let path = manifest
        .strip_prefix(root)
        .and_then(|rest| rest.strip_suffix("Cargo.toml"))
        .map(|rest| rest.trim_matches('/').to_owned())
        .ok_or_else(|| format!("{name} lies outside the workspace root"))?;
    let mut library = None;
    let mut binaries = Vec::new();
    for target in value["targets"].as_array().into_iter().flatten() {
        let kinds: Vec<_> = target["kind"].as_array().into_iter().flatten().collect();
        let target_name = text(target, "name")?;
        if kinds.iter().any(|kind| *kind == "lib") {
            library = Some(target_name);
        } else if kinds.iter().any(|kind| *kind == "bin") {
            binaries.push(target_name);
        }
    }
    binaries.sort();
    let mut features = Vec::new();
    let mut defaults = Vec::new();
    if let Some(map) = value["features"].as_object() {
        for (feature, enables) in map {
            let enables: Vec<String> = enables
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            if feature == "default" {
                defaults = enables;
                continue;
            }
            let (dependencies, implied): (Vec<_>, Vec<_>) = enables
                .into_iter()
                .partition(|entry| entry.starts_with("dep:"));
            let dependencies = dependencies
                .into_iter()
                .map(|entry| entry.trim_start_matches("dep:").to_owned())
                .collect();
            features.push((feature.clone(), implied, dependencies));
        }
    }
    Ok(Package {
        name,
        library,
        binaries,
        path,
        description: value["description"].as_str().map(str::to_owned),
        features,
        defaults,
    })
}

fn code_list(items: &[String]) -> String {
    if items.is_empty() {
        "—".to_owned()
    } else {
        items
            .iter()
            .map(|item| format!("`{item}`"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The crate reference page.
pub(crate) fn page(metadata: &Value) -> Result<String> {
    let root = text(metadata, "workspace_root")?;
    let mut packages = metadata["packages"]
        .as_array()
        .ok_or("cargo metadata lists no packages")?
        .iter()
        .map(|value| package(value, &root))
        .collect::<Result<Vec<_>>>()?;
    packages.sort_by(|a, b| a.path.cmp(&b.path));
    let version = metadata["packages"][0]["version"]
        .as_str()
        .ok_or("cargo metadata: no version")?;

    let mut page = format!(
        "---\ntitle: Crates\nsidebar_position: 1\ndescription: {}\ncustom_edit_url: null\n---\n\n{HEADER}\n\n# Crates\n\n",
        yaml_string(&format!(
            "Every package in the llm workspace at {version}: what it is, its library name and its optional features."
        )),
    );
    let _ = write!(
        page,
        "The workspace holds {} packages at version `{version}`. None is published to a registry: \
         a project depends on them from Git at a release tag. Package names start with `b10x-`; \
         library names do not, so `b10x-llm-core` is `use llm_core`. This page is generated from \
         `cargo metadata` by `llm-docs`; [the overview](../concepts/overview.md#which-crate-do-i-need) \
         says which crate to depend on for what.\n\n",
        packages.len()
    );
    page.push_str("| Package | Library | Directory | What it is |\n| --- | --- | --- | --- |\n");
    for package in &packages {
        let library = match (&package.library, package.binaries.is_empty()) {
            (Some(library), _) => format!("`{library}`"),
            (None, false) => format!("binary {}", code_list(&package.binaries)),
            (None, true) => "—".to_owned(),
        };
        let _ = writeln!(
            page,
            "| `{}` | {library} | `{}` | {} |",
            package.name,
            package.path,
            package.description.as_deref().unwrap_or("—")
        );
    }
    page.push_str("\n## Optional features\n\n");
    page.push_str(
        "Every feature is off by default unless the package lists it as a default. A feature \
         adds the optional dependencies named beside it and nothing else.\n\n",
    );
    page.push_str(
        "| Package | Feature | Turns on | Adds dependencies |\n| --- | --- | --- | --- |\n",
    );
    for package in &packages {
        for (feature, implied, dependencies) in &package.features {
            let _ = writeln!(
                page,
                "| `{}` | `{feature}` | {} | {} |",
                package.name,
                code_list(implied),
                code_list(dependencies)
            );
        }
    }
    let with_defaults: Vec<_> = packages
        .iter()
        .filter(|package| !package.defaults.is_empty())
        .collect();
    if with_defaults.is_empty() {
        page.push_str("\nNo package turns on a feature by default.\n");
    } else {
        page.push_str("\nDefault features:\n\n");
        for package in with_defaults {
            let _ = writeln!(
                page,
                "- `{}`: {}",
                package.name,
                code_list(&package.defaults)
            );
        }
    }
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        json!({
            "workspace_root": "/w",
            "packages": [
                {
                    "name": "b10x-x", "version": "1.2.3", "manifest_path": "/w/crates/x/Cargo.toml",
                    "description": "The x crate.",
                    "targets": [{"kind": ["lib"], "name": "x"}, {"kind": ["test"], "name": "t"}],
                    "features": {"default": [], "a": ["dep:serde"], "b": ["a", "dep:toml"]}
                },
                {
                    "name": "tool", "version": "1.2.3", "manifest_path": "/w/checks/tool/Cargo.toml",
                    "targets": [{"kind": ["bin"], "name": "tool"}],
                    "features": {}
                }
            ]
        })
    }

    #[test]
    fn page_lists_packages_libraries_and_features() {
        let page = page(&sample()).unwrap();
        assert!(page.contains(HEADER));
        assert!(page.contains("custom_edit_url: null"));
        assert!(page.contains("| `b10x-x` | `x` | `crates/x` | The x crate. |"));
        assert!(page.contains("| `tool` | binary `tool` | `checks/tool` | — |"));
        assert!(page.contains("| `b10x-x` | `a` | — | `serde` |"));
        assert!(page.contains("| `b10x-x` | `b` | `a` | `toml` |"));
        assert!(page.contains("No package turns on a feature by default."));
        assert!(page.find("checks/tool").unwrap() < page.find("crates/x").unwrap());
        assert!(!page.contains("/w/"), "absolute paths stay out of the page");
    }

    #[test]
    fn a_package_outside_the_root_is_refused() {
        let mut metadata = sample();
        metadata["packages"][0]["manifest_path"] = json!("/elsewhere/Cargo.toml");
        assert!(page(&metadata).is_err());
    }
}
