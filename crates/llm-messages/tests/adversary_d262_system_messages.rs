//! Adversary pass on story:messages-ingress-system-messages (decision D262).
//!
//! A later `system` entry "stays at its place". When that place is between an assistant
//! `tool_use` and the user `tool_result` answering it, the marked user text lands in front of
//! the result, and the projection merges the two into one user message whose first block is
//! text. The Messages API documents that in the user message holding tool results the
//! `tool_result` blocks come first and any text after them; a body with text first is refused
//! with a 400. Before this change such a request was refused at ingress; now the gateway
//! accepts it and forwards a body the route refuses.
mod support;

use llm_core::{Error, TurnRequest};
use llm_messages::{SYSTEM_MESSAGE_CLOSE, SYSTEM_MESSAGE_OPEN, decode_request, encode_request};
use llm_providers::Binding;
use serde_json::{Value, json};
use support::{binding_with, capabilities};

fn binding() -> Binding {
    binding_with("https://messages.example.invalid/v1", capabilities())
}

fn decode(body: &Value) -> Result<TurnRequest, Error> {
    decode_request(body.to_string().as_bytes(), binding().provenance())
        .map(|ingress| ingress.request)
}

fn tool_turn_with_a_system_entry_after_the_call() -> Value {
    json!({
        "model": "internal-model",
        "max_tokens": 512,
        "tools": [{"name":"bash","description":"Run a command","input_schema":{"type":"object"}}],
        "messages": [
            {"role":"user","content":"Run the tests"},
            {"role":"assistant","content":[
                {"type":"tool_use","id":"toolu_01","name":"bash","input":{"command":"cargo test"}}
            ]},
            {"role":"system","content":"Hook output: the working tree is dirty"},
            {"role":"user","content":[
                {"type":"tool_result","tool_use_id":"toolu_01","content":"ok"}
            ]},
        ],
    })
}

#[test]
fn a_system_entry_between_a_tool_call_and_its_result_does_not_put_text_before_the_result() {
    let request = decode(&tool_turn_with_a_system_entry_after_the_call())
        .unwrap_or_else(|error| panic!("the ingress accepts the turn: {error:?}"));
    let wire: Value = serde_json::from_slice(
        &encode_request(&request, &binding()).unwrap_or_else(|error| panic!("{error:?}")),
    )
    .expect("the projection is JSON");
    let messages = wire["messages"].as_array().expect("messages");
    let answer = messages
        .iter()
        .position(|message| {
            message["role"] == "assistant"
                && message["content"]
                    .as_array()
                    .is_some_and(|blocks| blocks.iter().any(|b| b["type"] == "tool_use"))
        })
        .expect("the assistant tool call is projected")
        + 1;
    assert_eq!(
        messages[answer]["role"], "user",
        "the message after the tool call answers it: {wire}"
    );
    assert_eq!(
        messages[answer]["content"][0]["type"], "tool_result",
        "the user message answering a tool_use must open with its tool_result; text goes after \
         the results, or the route answers 400: {wire}"
    );
    let marked = format!(
        "{SYSTEM_MESSAGE_OPEN}Hook output: the working tree is dirty{SYSTEM_MESSAGE_CLOSE}"
    );
    assert!(
        messages.iter().any(|message| message["content"]
            .as_array()
            .is_some_and(|blocks| blocks.iter().any(|b| b["text"] == marked.as_str()))),
        "the system entry is still carried, not dropped: {wire}"
    );
}
