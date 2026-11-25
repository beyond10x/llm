//! The gate's test phase and its pull request paths, held to the workspace rules "Tests stay
//! fast" and "Planning-only pull requests skip the tests".
//!
//! Repository files are read at run time through `CARGO_MANIFEST_DIR`, never baked in at compile
//! time, so a binary reused from another tree cannot read that tree's files.

mod budget;
mod debug_info;
mod ess_pin;
#[cfg(unix)]
mod list;
mod paths;
#[cfg(unix)]
mod release;
mod report;

use std::path::{Path, PathBuf};

fn repository() -> PathBuf {
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    Path::new(&manifest)
        .join("../..")
        .canonicalize()
        .expect("the repository root exists")
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(repository().join(relative))
        .unwrap_or_else(|error| panic!("reading {relative}: {error}"))
}

fn workflow() -> serde_yaml::Value {
    serde_yaml::from_str(&read(".github/workflows/gate.yml")).expect("gate.yml is YAML")
}
