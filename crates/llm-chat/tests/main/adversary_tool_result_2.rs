//! Adversary pass 2 on Chat ingress reading a failed tool result back (wave 2026-10-20-w09).
use llm_chat::decode_ingress_request;
use llm_core::{CallId, Item};
use serde_json::json;

/// `spec/domains/chat.yaml` (`IngressDecoding.items`) and `docs/chat.md`: only a repeated
/// `error` or `ok` key leaves the content a successful string. The `error` value is read as any
/// JSON value is, so a key repeated inside it keeps its last value and the content reads back
/// failed, with the first `code` gone.
#[test]
fn adv_a_key_repeated_inside_the_error_reads_back_failed_keeping_its_last_value() {
    let content = r#"{"error":{"code":404,"code":500},"ok":false}"#;
    let body = json!({
        "model": "code",
        "messages": [
            {"role": "user", "content": "Weather?"},
            {"role": "assistant", "content": null, "tool_calls": [
                {"id": "call-1", "type": "function",
                 "function": {"name": "lookup", "arguments": "{}"}}
            ]},
            {"role": "tool", "tool_call_id": "call-1", "content": content}
        ],
        "tools": [{"type": "function", "function": {
            "name": "lookup", "description": "Look up", "parameters": {"type": "object"}
        }}]
    });
    let item = decode_ingress_request(&body)
        .expect("decoded")
        .request
        .items
        .pop()
        .expect("the tool result");
    assert_eq!(
        item,
        Item::ToolResult {
            call_id: CallId::new("call-1").expect("fixture call"),
            output: json!({"code": 500}),
            failed: true,
        }
    );
}
