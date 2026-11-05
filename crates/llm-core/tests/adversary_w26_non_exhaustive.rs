//! Adversary, wave 2026-11-05-w26, story:core-types-non-exhaustive.
//!
//! Two claims this unit made about itself, each driven against the code:
//!
//! - `docs/design.md` "Types that grow": "Inside its own crate a match still names every
//!   variant." `#[non_exhaustive]` does not reach inside `llm-core`, so there the compiler is the
//!   one guard that sends the next item kind (story:image-input, unit 2 of this wave) to every
//!   match on `Item`. It does that only for a match without a catch-all arm: a match ending in
//!   `_ =>`, or in a binding arm such as `other =>`, takes a new kind silently.
//! - The `with_*` setters each set their own declaration. The unit's own setter test sets all four
//!   switches on and then all four off, so a setter writing a neighbour's field passes it.

use llm_core::Capabilities;
use std::fs;
use std::path::{Path, PathBuf};

const ITEM_VARIANTS: [&str; 7] = [
    "UserText",
    "AssistantText",
    "AssistantRefusal",
    "ToolCall",
    "ToolResult",
    "Opaque",
    "UnattributedOpaque",
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// `_ => …` or `name => …`: an unguarded arm that matches whatever reaches it.
fn is_catch_all_arm(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") {
        return false;
    }
    let Some((pattern, _)) = trimmed.split_once("=>") else {
        return false;
    };
    let pattern = pattern.trim();
    pattern == "_"
        || (pattern.starts_with(|c: char| c.is_ascii_lowercase())
            && pattern
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
}

/// Whether `text` names an `Item` variant through `prefix` (`Item::` or, inside `impl Item`,
/// `Self::`), as a whole name: `Self::ToolCalls` is not `Self::ToolCall`.
fn names_an_item_variant(text: &str, prefix: &str) -> bool {
    ITEM_VARIANTS.iter().any(|variant| {
        let needle = format!("{prefix}{variant}");
        text.match_indices(&needle).any(|(at, _)| {
            !text[at + needle.len()..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
        })
    })
}

/// Every catch-all arm of a match on `Item` in `llm-core`'s own source, as `file:line`.
fn catch_all_arms_on_item() -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_files(&root.join("src"), &mut files);
    let mut sites = Vec::new();
    for file in files {
        let source = fs::read_to_string(&file).unwrap();
        let lines: Vec<&str> = source.lines().collect();
        let in_impl_item = file.ends_with("item.rs");
        for (at, line) in lines.iter().enumerate() {
            if !is_catch_all_arm(line) {
                continue;
            }
            let Some(head) = (0..at).rev().find(|&before| {
                !lines[before].trim().is_empty() && indent(lines[before]) < indent(line)
            }) else {
                continue;
            };
            if !lines[head].contains("match ") {
                continue;
            }
            let arms = lines[head..at].join("\n");
            if names_an_item_variant(&arms, "Item::")
                || (in_impl_item && names_an_item_variant(&arms, "Self::"))
            {
                let relative = file.strip_prefix(root).unwrap().display().to_string();
                sites.push(format!("crates/llm-core/{relative}:{}", at + 1));
            }
        }
    }
    sites
}

#[test]
fn the_scan_sees_a_catch_all_arm_and_not_a_guard_a_comment_or_a_longer_name() {
    // The scan's own check: it must see the arm it exists for, and leave the shapes it is not
    // about alone (a guarded arm, a comment, a variant whose name only starts like one).
    assert!(is_catch_all_arm("                _ => {}"));
    assert!(is_catch_all_arm("            other => Ok(other),"));
    assert!(!is_catch_all_arm(
        "    Item::Opaque { provenance, .. } if provenance != target => {"
    ));
    assert!(!is_catch_all_arm("    _ if value.is_null() => {}"));
    assert!(!is_catch_all_arm("///         _ => {"));
    assert!(names_an_item_variant(
        "Self::ToolCall(_) => \"tool-call\",",
        "Self::"
    ));
    assert!(!names_an_item_variant("Self::ToolCalls => {}", "Self::"));
}

#[test]
fn inside_llm_core_every_match_on_item_names_the_variants_it_handles() {
    let sites = catch_all_arms_on_item();
    assert!(
        sites.is_empty(),
        "docs/design.md \"Types that grow\" says a match inside llm-core still names every \
         variant, so the compiler sends a new item kind to each one; these matches on Item end \
         in a catch-all arm instead and take a new kind silently: {sites:#?}"
    );
}

/// The four switches a setter can flip, each with the key its serialized form writes.
const SWITCHES: [&str; 4] = ["tools", "tool_choice", "temperature", "top_p"];

fn document(on: &[&str]) -> String {
    let switches: Vec<String> = SWITCHES
        .iter()
        .map(|key| format!("\"{key}\":{}", on.contains(key)))
        .collect();
    format!(
        "{{{},\"reasoning_efforts\":[],\"context_window\":32768,\"max_output_tokens\":8192}}",
        switches.join(",")
    )
}

type Setter = fn(Capabilities, bool) -> Capabilities;

fn setters() -> [(&'static str, Setter); 4] {
    [
        ("tools", Capabilities::with_tools),
        ("tool_choice", Capabilities::with_tool_choice),
        ("temperature", Capabilities::with_temperature),
        ("top_p", Capabilities::with_top_p),
    ]
}

/// Whether `set` flips exactly `key`, on from none and off from all, as the document writes it.
fn sets_only(key: &str, set: Setter) -> bool {
    let all = Capabilities::text(32768, 8192)
        .with_tools(true)
        .with_tool_choice(true)
        .with_temperature(true)
        .with_top_p(true);
    let others: Vec<&str> = SWITCHES.into_iter().filter(|other| *other != key).collect();
    serde_json::to_string(&set(Capabilities::text(32768, 8192), true)).unwrap() == document(&[key])
        && serde_json::to_string(&set(all, false)).unwrap() == document(&others)
}

#[test]
fn each_setter_writes_its_own_declaration_and_no_other() {
    for (key, set) in setters() {
        assert!(sets_only(key, set), "with_{key} writes another declaration");
    }
    // A setter that wrote its neighbour's field, which the unit's own test cannot tell apart:
    // it sets every switch on, then every switch off.
    let swapped: Setter = |mut capabilities, value| {
        capabilities.top_p = value;
        capabilities
    };
    assert!(!sets_only("temperature", swapped));
}

#[test]
fn with_reasoning_efforts_replaces_what_was_declared_and_keeps_the_given_order() {
    let read: Capabilities = serde_json::from_str(
        r#"{"tools":false,"tool_choice":false,"temperature":false,"top_p":false,"reasoning_efforts":["low","medium"],"context_window":32768,"max_output_tokens":8192}"#,
    )
    .unwrap();
    let replaced = read.with_reasoning_efforts(["high", "low"]);
    assert_eq!(replaced.reasoning_efforts, ["high", "low"]);
    assert_eq!(
        serde_json::to_string(&replaced).unwrap(),
        r#"{"tools":false,"tool_choice":false,"temperature":false,"top_p":false,"reasoning_efforts":["high","low"],"context_window":32768,"max_output_tokens":8192}"#
    );
}
