//! Adversarial cases for `story:unattributed-opaque-state`, driven from the documents the unit
//! wrote about itself. No implementation file is edited here.
mod support;

use llm_messages::{decode_request, encode_request};
use serde_json::{Value, json};
use support::binding;

/// `docs/messages.md`: `decode_request` carries a thinking block and, after the caller binds it,
/// "the block goes out" as it came in.
///
/// **Coordinator decision, correction round 1: the contract is JSON-equal, not byte-equal.** The
/// payload is held as a `serde_json::Value`, whose keys serialize sorted, and the workspace does
/// not enable `raw_value`. The block below is written in the field order a client writes it
/// (`type` first) so the case would still notice if equality were ever claimed for bytes; what it
/// asserts is that the block that went out is the same JSON value as the one that came in.
#[test]
fn a_bound_thinking_block_goes_out_exactly_as_it_came_in() {
    let block = r#"{"type":"thinking","thinking":"weighing","signature":"sig-1"}"#;
    let wire = format!(
        r#"{{"model":"internal-model","max_tokens":512,"messages":[{{"role":"user","content":"Summarise the log"}},{{"role":"assistant","content":[{block}]}},{{"role":"user","content":"go on"}}]}}"#
    );
    let mut request = decode_request(wire.as_bytes(), binding().provenance())
        .expect("ingress carries the block")
        .request;
    assert_eq!(
        request
            .bind_unattributed(binding().provenance())
            .expect("caller binds"),
        1
    );
    let sent: Value =
        serde_json::from_slice(&encode_request(&request, &binding()).expect("sendable once bound"))
            .expect("JSON");
    let arrived: Value = serde_json::from_str(block).expect("JSON");
    assert_eq!(
        sent["messages"][1]["content"],
        json!([arrived]),
        "the block that came in as {block} went out as another JSON value: {sent}"
    );
}
