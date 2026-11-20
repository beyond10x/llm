//! A Messages client's `cache_control` marks reach the Anthropic `InvokeModel` body where the
//! client placed them, with their lifetime, and no mark is added beside them
//! (story:client-cache-marks). A request without a mark keeps the projection's own breakpoints.
//!
//! A deployment that serves Claude on Bedrock through a gateway reported the defect: a mark on the
//! system block alone reached Bedrock with a second one on the last message, and a request without
//! any mark reached it with one. A cache write is priced above an uncached read, so a mark the
//! client did not send changes what the client pays.

use llm_bedrock::{Api, BedrockTarget, encode_request};
use llm_core::{Capabilities, Id, Protocol, Provenance, TurnRequest};
use llm_messages::{SYSTEM_MESSAGE_CLOSE, SYSTEM_MESSAGE_OPEN, decode_request};
use serde_json::{Value, json};

const MODEL: &str = "eu.anthropic.claude-sonnet-4-6";

fn provenance() -> Provenance {
    Provenance {
        protocol: Protocol::Messages,
        provider: Id::new("bedrock").unwrap(),
        account: Id::new("fixture-account").unwrap(),
        endpoint: Id::new("eu-central-1").unwrap(),
        model: Id::new("sonnet").unwrap(),
        binding_revision: Id::new("fixture-1").unwrap(),
    }
}

fn target() -> BedrockTarget {
    BedrockTarget::new(
        provenance(),
        Capabilities::text(200_000, 4_096)
            .with_tools(true)
            .with_tool_choice(true)
            .with_temperature(true)
            .with_top_p(true),
        "eu-central-1",
        MODEL,
        Api::AnthropicInvoke,
        true,
        None,
    )
    .expect("a valid fixture target")
}

fn decode(body: &Value) -> TurnRequest {
    decode_request(body.to_string().as_bytes(), &provenance())
        .expect("the Messages ingress reads the request")
        .request
}

/// The `InvokeModel` body `request` is sent as.
fn sent(request: &TurnRequest) -> Value {
    serde_json::from_slice(&encode_request(request, &target()).expect("a sendable turn"))
        .expect("the body is JSON")
}

/// Every mark in `value`, by its JSON path.
fn marks(value: &Value, path: &str, found: &mut Vec<(String, Value)>) {
    match value {
        Value::Object(fields) => {
            if let Some(mark) = fields.get("cache_control") {
                found.push((path.to_owned(), mark.clone()));
            }
            for (key, field) in fields {
                marks(field, &format!("{path}.{key}"), found);
            }
        }
        Value::Array(entries) => {
            for (index, entry) in entries.iter().enumerate() {
                marks(entry, &format!("{path}[{index}]"), found);
            }
        }
        _ => {}
    }
}

fn marks_of(body: &Value) -> Vec<(String, Value)> {
    let mut found = Vec::new();
    marks(body, "", &mut found);
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

const EPHEMERAL: fn() -> Value = || json!({"type": "ephemeral"});

#[test]
fn a_mark_on_the_system_block_alone_arrives_alone_on_that_block() {
    let request = decode(&json!({
        "model": "sonnet", "max_tokens": 4096,
        "system": [{"type": "text", "text": "Answer as the order desk",
            "cache_control": {"type": "ephemeral"}}],
        "messages": [{"role": "user", "content": "case-d where is order 7"}],
    }));
    let body = sent(&request);
    assert_eq!(
        body,
        json!({
            "anthropic_version": "bedrock-2023-05-31",
            "max_tokens": 4096,
            "system": [{"type": "text", "text": "Answer as the order desk",
                "cache_control": {"type": "ephemeral"}}],
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "case-d where is order 7"}]}],
        })
    );
    assert_eq!(
        marks_of(&body),
        vec![(".system[0]".to_owned(), EPHEMERAL())]
    );
}

#[test]
fn marks_on_content_blocks_and_a_tool_arrive_on_the_same_blocks_with_their_lifetime() {
    let hour = json!({"type": "ephemeral", "ttl": "1h"});
    let request = decode(&json!({
        "model": "sonnet", "max_tokens": 4096,
        "system": "Answer as the order desk",
        "tools": [
            {"name": "lookup_order", "description": "Order status", "input_schema": {"type": "object"}},
            {"name": "cancel_order", "description": "Cancel", "input_schema": {"type": "object"},
                "cache_control": hour},
        ],
        "messages": [
            {"role": "user", "content": [
                {"type": "text", "text": "where is order 7", "cache_control": hour},
                {"type": "text", "text": "and order 8"}]},
            {"role": "assistant", "content": [
                {"type": "tool_use", "id": "toolu_1", "name": "lookup_order", "input": {"id": "7"}}]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_1", "content": "shipped"},
                {"type": "text", "text": "thanks"}]},
        ],
    }));
    let body = sent(&request);
    assert_eq!(
        marks_of(&body),
        vec![
            (".messages[0].content[0]".to_owned(), hour.clone()),
            (".tools[1]".to_owned(), hour),
        ],
        "{body}"
    );
}

#[test]
fn a_later_system_entry_does_not_move_a_mark_after_it() {
    let request = decode(&json!({
        "model": "sonnet", "max_tokens": 4096,
        "messages": [
            {"role": "user", "content": "first"},
            {"role": "assistant", "content": "ok"},
            {"role": "system", "content": "Be brief"},
            {"role": "user", "content": [
                {"type": "text", "text": "second", "cache_control": {"type": "ephemeral"}}]},
        ],
    }));
    let body = sent(&request);
    let tail = &body["messages"][2]["content"];
    assert_eq!(
        tail,
        &json!([
            {"type": "text", "text": format!("{SYSTEM_MESSAGE_OPEN}Be brief{SYSTEM_MESSAGE_CLOSE}")},
            {"type": "text", "text": "second", "cache_control": {"type": "ephemeral"}},
        ]),
        "{body}"
    );
    assert_eq!(marks_of(&body).len(), 1, "{body}");
}

#[test]
fn a_request_without_a_mark_keeps_the_projections_own_breakpoints() {
    let request = decode(&json!({
        "model": "sonnet", "max_tokens": 4096,
        "system": "Answer as the order desk",
        "messages": [{"role": "user", "content": "case-a where is order 7"}],
    }));
    let body = sent(&request);
    assert_eq!(
        marks_of(&body),
        vec![
            (".messages[0].content[0]".to_owned(), EPHEMERAL()),
            (".system[0]".to_owned(), EPHEMERAL()),
        ],
        "{body}"
    );
}
