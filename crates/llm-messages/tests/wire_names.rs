//! Every producer name this projection speaks is classified by the evidence behind it.
//!
//! A wire name is not a behaviour: the behaviour around `inference_geo` is tested whether or not
//! the route ever sends a field by that name. A name that is wrong is silently never seen, or
//! refuses a field the route really sends, and no fixture written from the same wrong name can
//! notice. So the names are enumerated here and each one carries its evidence.
//!
//! This is a check rather than a paragraph because a list maintained by hand is the defect: the
//! next name added to the codec fails this test until somebody says where it came from.

use std::{collections::BTreeSet, fs, path::Path};

/// Names read this session from `beyond10x/harness` `crates/harness-messages` at
/// `709a2ebadcc14602b82b6f3c240350e4ddc1c88c` — the extraction provenance the implementation
/// contract names. Each appears as a literal in that crate's own projection.
const CONFIRMED: &[&str] = &[
    // The pinned API version and the header that names it, read at
    // `crates/harness-messages/src/lib.rs:74` and `:77` of that same commit. Both are sent on
    // every request this projection makes, and neither was reachable by the scan below until it
    // admitted a hyphen and a leading digit.
    "2023-06-01",
    "anthropic-version",
    // Read at Harness `2fd7235bdef80be1f708af0b4e95a10ef091535d` for parity wave 2026-10-05-w27:
    // the prompt-cache breakpoint at `crates/harness-messages/src/project.rs:298` and `:538`,
    // and the two warning codes at `src/lib.rs:446` and `src/project.rs:345`.
    "cache_control",
    "ephemeral",
    // Read at Harness `3169042f` for story:anthropic-access (parity M5): the beta header and the
    // beta a subscription token requires, `crates/harness-messages/src/lib.rs:86` and `:93`.
    "anthropic-beta",
    "oauth-2025-04-20",
    "unknown-output-item",
    "unknown-stream-event",
    "any",
    "api_error",
    "assistant",
    "authentication_error",
    "cache_creation_input_tokens",
    "cache_read_input_tokens",
    "content",
    "content_block",
    "content_block_delta",
    "content_block_start",
    "content_block_stop",
    "data",
    "delta",
    "description",
    "effort",
    "end_turn",
    "error",
    "id",
    "index",
    "input",
    "input_json_delta",
    "input_schema",
    "input_tokens",
    "invalid_request_error",
    "is_error",
    "max_tokens",
    "message",
    "message_delta",
    "message_start",
    "message_stop",
    "messages",
    "model",
    "name",
    "not_found_error",
    "output_config",
    "output_tokens",
    "overloaded_error",
    "partial_json",
    "permission_error",
    "ping",
    "redacted_thinking",
    "request_too_large",
    "role",
    "server_tool_use",
    "signature",
    "signature_delta",
    "stop_reason",
    "stop_sequence",
    "stream",
    "system",
    "temperature",
    "text",
    "text_delta",
    "thinking",
    "thinking_delta",
    "tool",
    "tool_choice",
    "tool_result",
    "tool_use",
    "tool_use_id",
    "tools",
    "top_p",
    "type",
    "usage",
    "user",
];

/// Names that arrived with the carried draft at `83e24eb` and that **no source read in this
/// session supports**: not the harness projection, and not the producer's own types, which need
/// network access this gate does not have. The behaviour around each is tested; the spelling is
/// an open question for `story:anthropic-access`, which reads the producer for its own reasons.
const CARRIED: &[&str] = &[
    "auto",
    "cache_creation",
    "caller",
    "citations",
    "ephemeral_1h_input_tokens",
    "ephemeral_5m_input_tokens",
    "inference_geo",
    "output_tokens_details",
    "rate_limit_error",
    "service_tier",
    "thinking_tokens",
    "timeout_error",
    "web_fetch_requests",
    "web_search_requests",
];

/// Every wire-shaped string literal in the projection's own source.
fn spoken() -> BTreeSet<String> {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut names = BTreeSet::new();
    for entry in fs::read_dir(&source).expect("the projection's source") {
        let path = entry.expect("a source entry").path();
        if path.extension().is_none_or(|kind| kind != "rs") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("readable source");
        for literal in literals(&text) {
            if is_wire_shaped(&literal) {
                names.insert(literal);
            }
        }
    }
    assert!(!names.is_empty(), "no source was scanned");
    names
}

/// The contents of every double-quoted literal, escapes skipped. The projection's source uses no
/// raw strings, which this asserts rather than assumes.
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

/// The shape a producer name takes on this route, which is wider than one character class.
///
/// A scan admitting only `[a-z0-9_]` cannot reach a hyphenated header name or a dated version
/// string, and this projection sends one of each on **every** request: `anthropic-version` and
/// `2023-06-01`. A check that classifies only the names it happens to select says nothing about
/// the names the projection speaks, which is the one thing it exists to say.
fn is_wire_shaped(literal: &str) -> bool {
    (2..=41).contains(&literal.len())
        && literal.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && literal
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

#[test]
fn every_wire_name_the_projection_speaks_carries_its_evidence() {
    let spoken = spoken();
    let classified: BTreeSet<String> = CONFIRMED
        .iter()
        .chain(CARRIED)
        .map(|name| (*name).to_owned())
        .collect();
    let unclassified: Vec<_> = spoken.difference(&classified).collect();
    assert!(
        unclassified.is_empty(),
        "these producer names are spoken by the projection and nothing says where they came \
         from; add each to CONFIRMED with the source that was read, or to CARRIED: {unclassified:?}",
    );
    let stale: Vec<_> = classified.difference(&spoken).collect();
    assert!(
        stale.is_empty(),
        "these names are classified but no longer spoken; a stale entry hides the next \
         unclassified one: {stale:?}",
    );
}

#[test]
fn a_carried_name_is_never_also_claimed_as_confirmed() {
    for name in CARRIED {
        assert!(
            !CONFIRMED.contains(name),
            "{name} cannot be both read from a source and carried unread",
        );
    }
}
