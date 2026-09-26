//! The falsification record accounts for every refusal this projection can make.
//!
//! A record that lists kills says nothing about the guards no mutation was ever aimed at, and
//! that gap is the defect — a missing entry is only its symptom. So the enumeration is computed
//! here instead of being maintained by hand: every fixed diagnostic in the projection's own
//! source is either **claimed** by a recorded mutation, which was applied to that source and
//! shown to fail a named case or a named scenario, or it is on the record's own **unaimed** list.
//! A refusal added to the codec is in neither until somebody puts it in one, and then this check
//! is what says so — rather than the next reviewer.

use serde_json::Value;
use std::{collections::BTreeSet, fs, path::Path, path::PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("the repository root")
}

fn record() -> Value {
    let path = root().join("docs/verification/messages-falsification.json");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("the falsification record at {}", path.display()));
    serde_json::from_str(&text).expect("the falsification record is JSON")
}

fn mutations(record: &Value) -> &Vec<Value> {
    record["mutations"]
        .as_array()
        .expect("the record carries a mutation list")
}

/// The contents of every double-quoted literal, escapes skipped.
fn literals(text: &str) -> Vec<String> {
    assert!(
        !text.contains("r#\""),
        "a raw string literal would need a different scanner"
    );
    let mut found = Vec::new();
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character != '"' {
            continue;
        }
        let mut literal = String::new();
        loop {
            match characters.next() {
                None | Some('"') => break,
                Some('\\') => {
                    characters.next();
                    literal.push('\\');
                }
                Some(character) => literal.push(character),
            }
        }
        found.push(literal);
    }
    found
}

/// Every fixed diagnostic this projection can emit.
///
/// A diagnostic is a literal with a space in it. The crate's remaining literals are producer wire
/// names, which `tests/wire_names.rs` is the check for, and two media types; nothing with a space
/// in it reaches a caller except as the text of a refusal.
fn diagnostics() -> BTreeSet<String> {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = BTreeSet::new();
    for entry in fs::read_dir(&source).expect("the projection's source") {
        let path = entry.expect("a source entry").path();
        if path.extension().is_none_or(|kind| kind != "rs") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("readable source");
        found.extend(
            literals(&text)
                .into_iter()
                .filter(|literal| literal.contains(' ')),
        );
    }
    assert!(
        found.len() > 50,
        "no diagnostics were scanned; {} were read",
        found.len(),
    );
    found
}

fn claimed(record: &Value) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for mutation in mutations(record) {
        for guard in mutation["guards"]
            .as_array()
            .expect("every mutation states which refusals it deletes")
        {
            names.insert(
                guard
                    .as_str()
                    .expect("a refusal is named by its diagnostic")
                    .to_owned(),
            );
        }
    }
    names
}

fn unaimed(record: &Value) -> BTreeSet<String> {
    record["unaimed"]
        .as_array()
        .expect("the record lists the refusals nothing was aimed at")
        .iter()
        .map(|value| value.as_str().expect("a diagnostic").to_owned())
        .collect()
}

#[test]
fn every_refusal_this_projection_can_make_is_falsified_or_listed_as_unaimed() {
    let record = record();
    let diagnostics = diagnostics();
    let claimed = claimed(&record);
    let unaimed = unaimed(&record);
    let unknown: Vec<_> = claimed.difference(&diagnostics).collect();
    assert!(
        unknown.is_empty(),
        "these refusals are claimed by a mutation and the source no longer makes them; a stale \
         claim hides a refusal nothing is aimed at: {unknown:?}",
    );
    let both: Vec<_> = claimed.intersection(&unaimed).collect();
    assert!(
        both.is_empty(),
        "these refusals are both claimed and listed as unaimed: {both:?}",
    );
    let accounted: BTreeSet<String> = claimed.union(&unaimed).cloned().collect();
    let missing: Vec<_> = diagnostics.difference(&accounted).collect();
    assert!(
        missing.is_empty(),
        "these refusals are in the source and in neither half of the falsification record; aim a \
         mutation at each and record which named case or scenario dies, or add it to `unaimed` \
         and say so: {missing:?}",
    );
    let gone: Vec<_> = accounted.difference(&diagnostics).collect();
    assert!(
        gone.is_empty(),
        "the record accounts for refusals this source no longer makes: {gone:?}",
    );
}

#[test]
fn every_recorded_mutation_still_describes_this_tree() {
    let record = record();
    let root = root();
    for mutation in mutations(&record) {
        let name = mutation["mutation"].as_str().expect("a mutation name");
        let path = root.join(mutation["source"].as_str().expect("a mutated source"));
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("{name} names {} and it is not there", path.display()));
        for edit in mutation["edits"].as_array().expect("the edits it applied") {
            let before = edit["before"].as_str().expect("the text it replaced");
            assert_eq!(
                text.matches(before).count(),
                1,
                "{name} replaced text that now occurs {} times in {}; the record describes a tree \
                 that no longer exists",
                text.matches(before).count(),
                path.display(),
            );
            assert_ne!(
                before,
                edit["after"].as_str().expect("the text it wrote"),
                "{name} is not a mutation",
            );
        }
    }
}

/// A claim is what shrinks the `unaimed` list, so a mutation may only claim a refusal its own
/// source makes.
///
/// This bounds the claim without closing it: a mutation still could name a refusal from
/// elsewhere in the same file and nothing here would notice. That last edge is stated in the
/// record's own note rather than left to be found — it is the one hand-written part of an
/// otherwise measured document.
#[test]
fn a_mutation_only_claims_a_refusal_its_own_source_makes() {
    let record = record();
    let root = root();
    for mutation in mutations(&record) {
        let name = mutation["mutation"].as_str().expect("a mutation name");
        let path = root.join(mutation["source"].as_str().expect("a mutated source"));
        let text = fs::read_to_string(&path).expect("a mutated source");
        for guard in mutation["guards"].as_array().expect("its claims") {
            let guard = guard.as_str().expect("a diagnostic");
            assert!(
                text.contains(&format!("\"{guard}\"")),
                "{name} mutates {} and claims a refusal that file does not make: {guard}",
                path.display(),
            );
        }
    }
}

#[test]
fn every_recorded_mutation_was_killed_and_its_source_restored() {
    let record = record();
    for mutation in mutations(&record) {
        let name = mutation["mutation"].as_str().expect("a mutation name");
        assert_eq!(
            mutation["restored"],
            Value::Bool(true),
            "{name} did not restore its source",
        );
        assert_eq!(
            mutation["source_before_sha256"], mutation["restored_sha256"],
            "{name} restored its source to different bytes",
        );
        let lanes = mutation["killed_by"]
            .as_array()
            .expect("the lanes that killed it");
        assert!(
            !lanes.is_empty(),
            "{name} survived every lane, so nothing in the gate tests what it broke",
        );
        let named: usize = ["failed_cases", "failed_scenarios"]
            .iter()
            .map(|field| mutation[*field].as_array().map_or(0, Vec::len))
            .sum();
        assert!(named > 0, "{name} names no failure");
    }
}
