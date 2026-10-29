//! Adversary, wave 2026-10-29-w21, story:unpublished-tool-call-reaches-an-opted-in-caller.
//!
//! The control is local: no client body sets it, no Chat Completions body carries it, and the
//! turn after an unpublished call answered as a failure goes out as a tool call and a tool
//! message.
mod common;

use common::binding;
use llm_chat::{decode_ingress_request, project_request};
use llm_core::{CallId, ErrorCode, Item, ToolCall, ToolName, ToolSpec, TurnRequest};
use serde_json::{Value, json};

fn body(extra: &[(&str, Value)]) -> Value {
    let mut body = json!({
        "model": "small",
        "messages": [{"role": "user", "content": "Summarise the repo"}],
        "tools": [{"type": "function", "function": {"name": "discover",
                   "parameters": {"type": "object"}}}],
    });
    for (key, value) in extra {
        body[*key] = value.clone();
    }
    body
}

#[test]
fn a_client_body_naming_the_control_is_refused_by_its_path() {
    let accepted = decode_ingress_request(&body(&[])).expect("the plain body is accepted");
    assert!(!accepted.request.controls.return_unpublished_calls);
    for value in [json!(true), json!(false)] {
        let error = decode_ingress_request(&body(&[("return_unpublished_calls", value.clone())]))
            .expect_err("a client cannot set a local outcome policy");
        assert_eq!(error.code, ErrorCode::Unsupported, "{value}");
        assert!(
            error.message.contains("return_unpublished_calls"),
            "{value}: {}",
            error.message
        );
    }
    let error = decode_ingress_request(&body(&[(
        "stream_options",
        json!({"include_usage": true, "return_unpublished_calls": true}),
    )]))
    .expect_err("nor nested");
    assert!(
        error
            .message
            .contains("stream_options.return_unpublished_calls"),
        "{}",
        error.message
    );
}

#[test]
fn the_body_is_the_same_with_and_without_the_control() {
    let mut off = TurnRequest::new("small", vec![Item::user("Summarise the repo")]);
    off.tools = vec![ToolSpec {
        name: ToolName::new("discover").unwrap(),
        description: "d".to_owned(),
        input_schema: json!({"type": "object"}),
    }];
    off.items.extend([
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
    let mut on = off.clone();
    on.controls.return_unpublished_calls = true;
    let binding = binding();
    for stream in [false, true] {
        let sent_off = project_request(&off, &binding, stream).expect("projects");
        let sent_on = project_request(&on, &binding, stream).expect("projects");
        assert_eq!(sent_on, sent_off, "stream {stream}");
        assert!(!sent_on.to_string().contains("unpublished_calls"));
        let messages = sent_on["messages"].as_array().unwrap();
        assert_eq!(messages[1]["tool_calls"][0]["function"]["name"], "find");
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "call_1");
    }
}
