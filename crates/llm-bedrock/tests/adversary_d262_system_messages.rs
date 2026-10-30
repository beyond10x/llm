//! Adversary pass on story:messages-ingress-system-messages (decision D262), Bedrock side.
//!
//! A later `system` entry between an assistant tool call and the user's tool result becomes a
//! marked user text in front of the result. Both Bedrock wires merge it with the result into
//! one user message whose first block is text, ahead of the `toolResult` / `tool_result` that
//! answers the call. Anthropic models on Bedrock require the result blocks first.

use llm_bedrock::{Api, BedrockTarget, encode_request};
use llm_core::{Capabilities, Id, Protocol, Provenance};
use llm_messages::decode_request;
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

fn body(api: Api) -> Value {
    let ingress = json!({
        "model": MODEL,
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
    });
    let request = decode_request(
        ingress.to_string().as_bytes(),
        &provenance(Protocol::Messages),
    )
    .expect("the Messages ingress accepts the turn")
    .request;
    serde_json::from_slice(&encode_request(&request, &target(api)).expect("a sendable turn"))
        .expect("the body is JSON")
}

#[test]
fn converse_opens_the_answer_to_a_tool_call_with_its_tool_result() {
    let body = body(Api::Converse);
    assert_eq!(body["messages"][2]["role"], "user", "{body}");
    assert!(
        body["messages"][2]["content"][0]
            .get("toolResult")
            .is_some(),
        "the user message answering the toolUse opens with its toolResult: {body}"
    );
}

#[test]
fn invoke_model_opens_the_answer_to_a_tool_call_with_its_tool_result() {
    let body = body(Api::AnthropicInvoke);
    assert_eq!(body["messages"][2]["role"], "user", "{body}");
    assert_eq!(
        body["messages"][2]["content"][0]["type"], "tool_result",
        "the user message answering the tool_use opens with its tool_result: {body}"
    );
}
