//! Adversarial pass 2: the wire-name classification is only as wide as the scan that feeds it.
//!
//! `docs/messages.md:99` and `docs/verification/messages.md:132` both make the same claim about
//! `tests/wire_names.rs`: it "scans this crate's source for every wire-shaped literal and fails on
//! any name that is in neither list". The verification record adds that this was measured "by
//! adding one literal to `usage.rs`, watching the check name it, and restoring the file".
//!
//! One literal of one shape. `is_wire_shaped` (`tests/wire_names.rs:154`) admits a literal only when
//! every character is `[a-z0-9_]`, so the selection cannot reach a producer name containing a
//! hyphen — and the two the projection puts on the wire of **every single request** both do:
//! `anthropic-version` and its pinned value `2023-06-01` (src/client.rs:23, :25). Neither is in
//! CONFIRMED and neither is in CARRIED. The check passes anyway, so "all 79 wire literals are
//! classified" is a statement about the 79 the scan happens to select, not about the names the
//! projection speaks.
//!
//! These are the names where being wrong is least visible to a fixture, which is the stated reason
//! for keeping the lists at all: a wrong version header is not a decode failure, it is a live
//! route answering differently, and no fixture written from the same wrong spelling can notice.

use llm_messages::{ANTHROPIC_VERSION, VERSION_HEADER};
use std::{collections::BTreeSet, fs, path::Path};

/// Every name in either classification list, read out of the check that owns them.
fn classified() -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("wire_names.rs");
    let source = fs::read_to_string(&path).expect("the wire-name check's own source");
    let mut names = BTreeSet::new();
    for list in ["const CONFIRMED", "const CARRIED"] {
        let start = source
            .find(list)
            .unwrap_or_else(|| panic!("{list} is declared in {}", path.display()));
        let body = &source[start..];
        let end = body.find("];").expect("a closing bracket for the list");
        for literal in body[..end].split('"').skip(1).step_by(2) {
            names.insert(literal.to_owned());
        }
    }
    assert!(
        names.len() > 50,
        "the two lists were not parsed; {} names were read",
        names.len(),
    );
    names
}

/// Guards the case below: these are not invented strings, they are what the source really sends.
#[test]
fn the_names_under_test_are_really_spoken_by_the_projection() {
    let client = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("client.rs"),
    )
    .expect("the client's source");
    for name in [VERSION_HEADER, ANTHROPIC_VERSION] {
        assert!(
            client.contains(&format!("\"{name}\"")),
            "{name} is not a literal in the projection's own source",
        );
    }
}

#[test]
fn every_producer_name_on_every_request_carries_its_evidence() {
    let classified = classified();
    let unclassified: Vec<&str> = [VERSION_HEADER, ANTHROPIC_VERSION]
        .into_iter()
        .filter(|name| !classified.contains(*name))
        .collect();
    assert!(
        unclassified.is_empty(),
        "these producer names are on the wire of every request and are in neither CONFIRMED nor \
         CARRIED, and tests/wire_names.rs does not fail on them because is_wire_shaped rejects a \
         hyphen: {unclassified:?}",
    );
}
