//! Adversary, wave 2026-10-29-w21, story:unpublished-tool-call-reaches-an-opted-in-caller.
//!
//! The control is local: no client body sets it, no Messages body carries it, and the turn
//! after an unpublished call answered as a failure goes out as `tool_use` and an error result.
mod support;

use llm_core::{CallId, ErrorCode, Item, ToolCall, ToolName, ToolSpec, TurnRequest};
use llm_messages::{decode_request, encode_request};
use serde_json::{Value, json};
use support::{binding_with, capabilities};

fn binding() -> llm_providers::Binding {
    binding_with("https://messages.example.invalid/v1", capabilities())
}

fn body(extra: &[(&str, Value)]) -> Value {
    let mut body = json!({
        "model": "internal-model",
        "max_tokens": 512,
        "messages": [{"role":"user","content":[{"type":"text","text":"Summarise the repo"}]}],
        "tools": [{"name":"discover","description":"d","input_schema":{"type":"object"}}],
    });
    for (key, value) in extra {
        body[*key] = value.clone();
    }
    body
}

#[test]
fn a_client_body_naming_the_control_is_refused_by_its_path() {
    let origin = binding();
    let accepted = decode_request(body(&[]).to_string().as_bytes(), origin.provenance())
        .expect("the plain body is accepted");
    assert!(!accepted.request.controls.return_unpublished_calls);
    for value in [json!(true), json!(false)] {
        let error = decode_request(
            body(&[("return_unpublished_calls", value.clone())])
                .to_string()
                .as_bytes(),
            origin.provenance(),
        )
        .expect_err("a client cannot set a local outcome policy");
        assert_eq!(error.code, ErrorCode::Unsupported, "{value}");
        assert!(
            error.message.contains("return_unpublished_calls"),
            "{value}: {}",
            error.message
        );
    }
    let error = decode_request(
        body(&[(
            "tool_choice",
            json!({"type": "auto", "return_unpublished_calls": true}),
        )])
        .to_string()
        .as_bytes(),
        origin.provenance(),
    )
    .expect_err("nor nested");
    assert!(
        error.message.contains("return_unpublished_calls"),
        "{}",
        error.message
    );
}

#[test]
fn the_body_is_the_same_with_and_without_the_control() {
    let mut off = TurnRequest::new("internal-model", vec![Item::user("Summarise the repo")]);
    off.tools = vec![ToolSpec {
        name: ToolName::new("discover").unwrap(),
        description: "d".to_owned(),
        input_schema: json!({"type": "object"}),
    }];
    off.items.extend([
        Item::ToolCall(ToolCall {
            call_id: CallId::new("toolu_1").unwrap(),
            name: ToolName::new("find").unwrap(),
            arguments: json!({"pattern": "*.md"}),
        }),
        Item::ToolResult {
            call_id: CallId::new("toolu_1").unwrap(),
            output: json!("no tool by that name is published"),
            failed: true,
        },
    ]);
    let mut on = off.clone();
    on.controls.return_unpublished_calls = true;
    let sent_off = encode_request(&off, &binding()).expect("encodes");
    let sent_on = encode_request(&on, &binding()).expect("encodes");
    assert_eq!(sent_on, sent_off);
    let body: Value = serde_json::from_slice(&sent_on).unwrap();
    assert!(!body.to_string().contains("unpublished_calls"));
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages[1]["content"][0]["type"], "tool_use");
    assert_eq!(messages[1]["content"][0]["name"], "find");
    assert_eq!(messages[2]["content"][0]["type"], "tool_result");
    assert_eq!(messages[2]["content"][0]["is_error"], true);
}
