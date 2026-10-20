//! Adversary cases for Chat ingress reading a failed tool result back (wave 2026-10-20-w09).
mod common;

use common::binding;
use llm_chat::{decode_ingress_request, project_request};
use llm_core::{CallId, ErrorCode, Item, ToolCall, ToolName, ToolSpec, TurnRequest};
use serde_json::{Value, json};

fn call_id(value: &str) -> CallId {
    CallId::new(value).expect("fixture call")
}

fn lookup() -> ToolName {
    ToolName::new("lookup").expect("fixture tool")
}

fn turn_with_result(output: Value, failed: bool) -> TurnRequest {
    let mut request = TurnRequest::new(
        "small",
        vec![
            Item::ToolCall(ToolCall {
                call_id: call_id("call-1"),
                name: lookup(),
                arguments: json!({}),
            }),
            Item::ToolResult {
                call_id: call_id("call-1"),
                output,
                failed,
            },
        ],
    );
    request.tools = vec![ToolSpec {
        name: lookup(),
        description: "Look up".to_owned(),
        input_schema: json!({"type": "object"}),
    }];
    request
}

fn body_answering_with(tool_message: &Value) -> Value {
    json!({
        "model": "code",
        "messages": [
            {"role": "user", "content": "Weather?"},
            {"role": "assistant", "content": null, "tool_calls": [
                {"id": "call-1", "type": "function",
                 "function": {"name": "lookup", "arguments": "{}"}}
            ]},
            tool_message
        ],
        "tools": [{"type": "function", "function": {
            "name": "lookup", "description": "Look up", "parameters": {"type": "object"}
        }}]
    })
}

/// The deepest output a failed result can carry and still read back failed: `serde_json` parses
/// client text at most 127 levels deep (its recursion limit of 128), and the failure object
/// `{"error": <output>, "ok": false}` is one of those levels.
const DEEPEST_OUTPUT_READ_BACK: usize = 126;

/// `output` nested inside `levels` arrays.
fn nested(levels: usize) -> Value {
    let mut output = json!("no such city");
    for _ in 0..levels {
        output = Value::Array(vec![output]);
    }
    output
}

/// The tool result a failed `output` sent by the Chat encoder decodes back to on Chat ingress.
fn failed_result_read_back(output: Value) -> Item {
    let request = turn_with_result(output, true);
    let projected = project_request(&request, &binding(), false).expect("projected");
    decode_ingress_request(&projected)
        .expect("the projected body decodes")
        .request
        .items
        .pop()
        .expect("the tool result")
}

/// `docs/chat.md` "Gateway ingress" states the limit: ingress parses a tool message's content
/// at most 127 levels deep, a bound on client input that is kept, so a failed output nested
/// deeper than 126 levels is sent as the failure object and reads back as a successful string
/// carrying that object's text.
#[test]
fn adv_a_failed_output_nested_past_the_parse_limit_reads_back_as_a_successful_string() {
    for levels in [DEEPEST_OUTPUT_READ_BACK + 1, 200] {
        let output = nested(levels);
        let content = json!({"error": output, "ok": false}).to_string();
        assert_eq!(
            failed_result_read_back(output),
            Item::ToolResult {
                call_id: call_id("call-1"),
                output: json!(content),
                failed: false,
            },
            "a failed output nested {levels} levels deep is past serde_json's 128-level parse \
             limit, so it reads back as a successful string, as docs/chat.md states"
        );
    }
}

/// The other side of the limit: content 127 levels deep, the failure object and an output
/// nested 126 levels, still reads back failed with its output.
#[test]
fn adv_a_failed_output_nested_to_the_parse_limit_still_decodes_back_failed() {
    let output = nested(DEEPEST_OUTPUT_READ_BACK);
    assert_eq!(
        failed_result_read_back(output.clone()),
        Item::ToolResult {
            call_id: call_id("call-1"),
            output,
            failed: true,
        }
    );
}

/// Guard: nothing outside the tool-result rule changes in Chat ingress. The `tool` arm reads the
/// call identifier before the content, as the base did, so a tool message carrying both an
/// unusable identifier and an unsupported content part is refused `InvalidRequest` for its
/// call. Reading the content first (an earlier revision of this change did) refuses it
/// `Unsupported` instead.
#[test]
fn adv_an_unusable_call_id_is_still_refused_before_the_content_is_read() {
    let body = body_answering_with(&json!({
        "role": "tool",
        "tool_call_id": "",
        "content": [{"type": "image_url"}]
    }));
    let error = decode_ingress_request(&body).expect_err("refused");
    assert_eq!(error.code, ErrorCode::InvalidRequest, "{error}");
}

/// Guard for `docs/chat.md`: content with a repeated `error` or `ok` key is not the two-member
/// object the Chat encoder sends, so it stays a successful result carrying its text byte for
/// byte. Reading it as a JSON value (an earlier revision of this change did) keeps the last
/// value of the repeated key, so the text collapses to a failure and its bytes are lost.
#[test]
fn adv_content_with_a_repeated_key_stays_a_successful_string() {
    for content in [
        r#"{"error":"x","ok":true,"ok":false}"#,
        r#"{"error":"a","error":"b","ok":false}"#,
    ] {
        let body = body_answering_with(&json!({
            "role": "tool", "tool_call_id": "call-1", "content": content
        }));
        let item = decode_ingress_request(&body)
            .expect("decoded")
            .request
            .items
            .pop()
            .expect("the tool result");
        assert_eq!(
            item,
            Item::ToolResult {
                call_id: call_id("call-1"),
                output: json!(content),
                failed: false,
            },
            "{content}"
        );
    }
}
