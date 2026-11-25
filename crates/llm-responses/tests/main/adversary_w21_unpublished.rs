//! Adversary, wave 2026-10-29-w21, story:unpublished-tool-call-reaches-an-opted-in-caller.
//!
//! The control is local: no client body sets it, no Responses body carries it, and the turn
//! after an unpublished call answered as a failure goes out as an ordinary function call and
//! its output.

use llm_core::{
    CallId, ErrorCode, Id, Item, Protocol, Provenance, ToolCall, ToolName, ToolSpec, TurnRequest,
};
use llm_responses::{Binding, encode_request, ingest_request};
use serde_json::{Value, json};

fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
}

fn binding() -> Binding {
    Binding::new(
        Provenance {
            protocol: Protocol::Responses,
            provider: id("my-lab"),
            account: id("local"),
            endpoint: id("local-vllm"),
            model: id("small"),
            binding_revision: id("rev-1"),
        },
        id("example/Small-Model"),
    )
}

fn body(extra: &[(&str, Value)]) -> Value {
    let mut body = json!({
        "model": "example/Small-Model",
        "input": [{"type": "message", "role": "user",
                   "content": [{"type": "input_text", "text": "Hi"}]}],
        "tools": [{"type": "function", "name": "file_read", "description": "d",
                   "parameters": {"type": "object"}, "strict": false}],
        "stream": true, "store": false,
        "include": ["reasoning.encrypted_content"],
    });
    for (key, value) in extra {
        body[*key] = value.clone();
    }
    body
}

#[test]
fn a_client_body_naming_the_control_is_refused_by_its_path() {
    let accepted = ingest_request(&binding(), &body(&[])).expect("the plain body is accepted");
    assert!(!accepted.controls.return_unpublished_calls);
    for value in [json!(true), json!(false)] {
        let error = ingest_request(
            &binding(),
            &body(&[("return_unpublished_calls", value.clone())]),
        )
        .expect_err("a client cannot set a local outcome policy");
        assert_eq!(error.code, ErrorCode::Unsupported, "{value}");
        assert!(
            error.message.contains("return_unpublished_calls"),
            "{value}: {}",
            error.message
        );
    }
    let error = ingest_request(
        &binding(),
        &body(&[("reasoning", json!({"return_unpublished_calls": true}))]),
    )
    .expect_err("nor nested");
    assert!(
        error.message.contains("reasoning.return_unpublished_calls"),
        "{}",
        error.message
    );
}

fn answered_unpublished() -> TurnRequest {
    let mut request = TurnRequest::new("small", vec![Item::user("Summarise the repo")]);
    request.tools = vec![ToolSpec {
        name: ToolName::new("discover").unwrap(),
        description: "d".to_owned(),
        input_schema: json!({"type": "object"}),
    }];
    request.items.extend([
        Item::ToolCall(ToolCall {
            call_id: CallId::new("call_1").unwrap(),
            name: ToolName::new("find").unwrap(),
            arguments: json!({"pattern": "*.md"}),
        }),
        Item::ToolResult {
            call_id: CallId::new("call_1").unwrap(),
            output: json!("no tool by that name is published"),
            failed: true,
        },
    ]);
    request
}

#[test]
fn the_body_is_the_same_with_and_without_the_control() {
    let off = answered_unpublished();
    let mut on = off.clone();
    on.controls.return_unpublished_calls = true;
    let sent_off = encode_request(&binding(), &off, None).expect("encodes");
    let sent_on = encode_request(&binding(), &on, None).expect("encodes");
    assert_eq!(sent_on, sent_off);
    let body: Value = serde_json::from_slice(&sent_on).unwrap();
    assert!(!body.to_string().contains("unpublished_calls"));
    let input = body["input"].as_array().unwrap();
    assert_eq!(input[1]["type"], "function_call");
    assert_eq!(input[1]["name"], "find");
    assert_eq!(input[2]["type"], "function_call_output");
    assert_eq!(input[2]["call_id"], "call_1");
}
