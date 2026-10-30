//! A Messages request with `system` entries inside `messages`, read by the Messages ingress and
//! sent to Bedrock (story:messages-ingress-system-messages, decision D262).
//!
//! The mapping is made once, onto the neutral turn, so both Bedrock wires send the same thing:
//! the leading entry in `system`, the later one as user text in the fixed mark, merged with the
//! user turn after it into one user message.

use llm_bedrock::{Api, BedrockTarget, encode_request};
use llm_core::{Capabilities, Id, Protocol, Provenance};
use llm_messages::{SYSTEM_MESSAGE_CLOSE, SYSTEM_MESSAGE_OPEN, decode_request};
use serde_json::{Value, json};

const MODEL: &str = "us.anthropic.claude-3-7-sonnet-20250219-v1:0";

fn provenance(protocol: Protocol) -> Provenance {
    Provenance {
        protocol,
        provider: Id::new("bedrock").unwrap(),
        account: Id::new("fixture-account").unwrap(),
        endpoint: Id::new("us-east-1").unwrap(),
        model: Id::new(MODEL).unwrap(),
        binding_revision: Id::new("fixture-1").unwrap(),
    }
}

fn target(api: Api) -> BedrockTarget {
    let protocol = match api {
        Api::Converse => Protocol::BedrockConverse,
        Api::AnthropicInvoke => Protocol::Messages,
    };
    BedrockTarget::new(
        provenance(protocol),
        Capabilities {
            tools: true,
            tool_choice: true,
            temperature: true,
            top_p: true,
            reasoning_efforts: Vec::new(),
            context_window: 200_000,
            max_output_tokens: 4_096,
            output_streaming: true,
            input_during_output: false,
        },
        "us-east-1",
        MODEL,
        api,
        true,
        None,
    )
    .expect("a valid fixture target")
}

fn ingress() -> llm_core::TurnRequest {
    let body = json!({
        "model": MODEL,
        "max_tokens": 512,
        "messages": [
            {"role":"system","content":"Answer in English"},
            {"role":"user","content":"Summarise the log"},
            {"role":"assistant","content":"Looking"},
            {"role":"system","content":"Be brief"},
            {"role":"user","content":"Go on"},
        ],
    });
    decode_request(body.to_string().as_bytes(), &provenance(Protocol::Messages))
        .expect("the Messages ingress reads a system entry inside messages")
        .request
}

fn marked(text: &str) -> String {
    format!("{SYSTEM_MESSAGE_OPEN}{text}{SYSTEM_MESSAGE_CLOSE}")
}

fn body(api: Api) -> Value {
    serde_json::from_slice(&encode_request(&ingress(), &target(api)).expect("a sendable turn"))
        .expect("the body is JSON")
}

fn roles(body: &Value) -> Vec<&str> {
    body["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .map(|message| message["role"].as_str().expect("a role"))
        .collect()
}

#[test]
fn converse_sends_the_leading_entry_as_system_and_the_later_one_as_marked_user_text() {
    let body = body(Api::Converse);
    assert_eq!(body["system"][0]["text"], "Answer in English", "{body}");
    assert_eq!(roles(&body), ["user", "assistant", "user"], "{body}");
    assert_eq!(
        body["messages"][2]["content"][0]["text"],
        marked("Be brief"),
        "{body}"
    );
    assert_eq!(body["messages"][2]["content"][1]["text"], "Go on", "{body}");
}

#[test]
fn invoke_model_sends_the_leading_entry_as_system_and_the_later_one_as_marked_user_text() {
    let body = body(Api::AnthropicInvoke);
    assert_eq!(body["system"][0]["text"], "Answer in English", "{body}");
    assert_eq!(roles(&body), ["user", "assistant", "user"], "{body}");
    assert_eq!(
        body["messages"][2]["content"][0]["text"],
        marked("Be brief"),
        "{body}"
    );
    assert_eq!(body["messages"][2]["content"][1]["text"], "Go on", "{body}");
}
