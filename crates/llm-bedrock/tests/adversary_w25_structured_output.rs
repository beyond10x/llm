//! Adversary pass 1, wave 2026-11-04-w25, story:structured-output, both Bedrock wires.
//!
//! The acceptance reads: "A member a wire has no place for (Messages and Converse have no
//! `strict`) is refused by name before I/O ... never dropped silently". Converse and the Messages
//! body `InvokeModel` sends both leave `strict: true` out without a word.

use llm_bedrock::{Api, BedrockTarget, encode_request};
use llm_core::{
    Capabilities, Dispatch, ErrorCode, Id, Item, OutputFormat, Protocol, Provenance, TurnRequest,
};
use serde_json::{Value, json};
use url::Url;

const MODEL: &str = "fixture-model";

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {"city": {"type": "string"}},
        "required": ["city"],
        "additionalProperties": false
    })
}

fn target(api: Api) -> BedrockTarget {
    let protocol = match api {
        Api::Converse => Protocol::BedrockConverse,
        Api::AnthropicInvoke => Protocol::Messages,
    };
    BedrockTarget::new(
        Provenance {
            protocol,
            provider: Id::new("bedrock").unwrap(),
            account: Id::new("fixture-account").unwrap(),
            endpoint: Id::new("us-east-1").unwrap(),
            model: Id::new(MODEL).unwrap(),
            binding_revision: Id::new("fixture-1").unwrap(),
        },
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
        Some(Url::parse("http://127.0.0.1:9/").unwrap()),
    )
    .expect("a valid fixture target")
}

fn turn(format: OutputFormat) -> TurnRequest {
    let mut request = TurnRequest::new(MODEL, vec![Item::user("Where is the tower?")]);
    request.controls.output_format = Some(format);
    request
}

fn assert_strict_refused(api: Api) {
    let request = turn(OutputFormat::json_schema(schema()).with_strict(true));
    match encode_request(&request, &target(api)) {
        Ok(bytes) => panic!(
            "{api:?}: strict: true was dropped, not refused; sent {}",
            String::from_utf8_lossy(&bytes)
        ),
        Err(error) => {
            assert_eq!(error.code, ErrorCode::Unsupported, "{api:?}");
            assert_eq!(error.dispatch, Dispatch::NotSent, "{api:?}");
            assert!(
                error.message.contains("strict"),
                "{api:?}: {}",
                error.message
            );
        }
    }
}

#[test]
fn converse_refuses_a_strict_flag_it_has_no_place_for_by_name_before_io() {
    assert_strict_refused(Api::Converse);
}

#[test]
fn invoke_model_refuses_a_strict_flag_it_has_no_place_for_by_name_before_io() {
    assert_strict_refused(Api::AnthropicInvoke);
}

/// Converse checks the format's name like every wire: one outside the class is refused before
/// any I/O rather than sent.
#[test]
fn converse_refuses_a_name_outside_the_class_before_io() {
    for name in ["a b", "", "place.v1"] {
        let error = encode_request(
            &turn(OutputFormat::json_schema(schema()).with_name(name)),
            &target(Api::Converse),
        )
        .expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{name:?}");
        assert_eq!(error.dispatch, Dispatch::NotSent, "{name:?}");
    }
}
