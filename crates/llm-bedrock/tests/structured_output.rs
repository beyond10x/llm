//! JSON-schema structured output on both Bedrock wires (story:structured-output).
//!
//! Converse carries the turn's `controls.output_format` as `outputConfig.textFormat`, the schema
//! as a JSON string and the name when it has one; `InvokeModel` sends the Messages body, so the
//! format is `output_config.format`. Bedrock always holds the answer to the schema, so a format
//! that asks it not to is refused before any I/O on either wire (`docs/bedrock.md`,
//! `docs/design.md` "Structured output").

use llm_bedrock::{Api, BedrockTarget, check_request, encode_request};
use llm_core::{
    Capabilities, Dispatch, Error, ErrorCode, Id, Item, OutputFormat, Protocol, Provenance,
    TurnRequest,
};
use serde_json::{Value, json};
use url::Url;

const MODEL: &str = "fixture-model";
const RECORDED_CONVERSE_BODY: &str =
    include_str!("fixtures/structured-output-converse-request.json");

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

fn turn(format: Option<OutputFormat>) -> TurnRequest {
    let mut request = TurnRequest::new(MODEL, vec![Item::user("Where is the tower?")]);
    "answer in JSON".clone_into(&mut request.instructions);
    request.controls.output_format = format;
    request
}

fn encoded(request: &TurnRequest, api: Api) -> Value {
    serde_json::from_slice(&encode_request(request, &target(api)).expect("encodes"))
        .expect("JSON bytes")
}

fn refused(request: &TurnRequest, api: Api) -> Error {
    let error = encode_request(request, &target(api)).expect_err("refused");
    assert_eq!(error.dispatch, Dispatch::NotSent);
    assert_eq!(
        check_request(request, &target(api)).expect_err("the same answer"),
        error
    );
    error
}

/// The bytes `encode_request` returns, which are the bytes `BedrockClient` sends, for a named
/// strict format: recorded in `tests/fixtures/structured-output-converse-request.json`.
/// Mutation: the schema sent as an object, the name dropped, a strict flag written.
#[test]
fn the_recorded_converse_structured_output_request_matches_its_fixture() {
    let request = turn(Some(
        OutputFormat::json_schema(schema())
            .with_name("place")
            .with_strict(true),
    ));
    assert_eq!(
        String::from_utf8(encode_request(&request, &target(Api::Converse)).expect("encodes"))
            .unwrap(),
        RECORDED_CONVERSE_BODY
            .strip_suffix('\n')
            .expect("one final newline"),
        "the exact request bytes changed; regenerate the fixture only on purpose"
    );
}

#[test]
fn converse_sends_the_schema_as_a_json_string_and_no_name_it_was_not_given() {
    let body = encoded(
        &turn(Some(OutputFormat::json_schema(schema()))),
        Api::Converse,
    );
    let text_format = &body["outputConfig"]["textFormat"];
    assert_eq!(text_format["type"], "json_schema");
    let definition = text_format["structure"]["jsonSchema"]
        .as_object()
        .expect("a JSON Schema definition");
    assert_eq!(
        definition.keys().map(String::as_str).collect::<Vec<_>>(),
        ["schema"]
    );
    let sent: Value =
        serde_json::from_str(definition["schema"].as_str().expect("a string")).expect("JSON");
    assert_eq!(sent, schema());
}

#[test]
fn absent_stays_absent_on_both_wires() {
    assert!(
        encoded(&turn(None), Api::Converse)
            .get("outputConfig")
            .is_none()
    );
    assert!(
        encoded(&turn(None), Api::AnthropicInvoke)
            .get("output_config")
            .is_none()
    );
}

#[test]
fn a_format_that_is_not_strict_is_refused_on_converse() {
    let error = refused(
        &turn(Some(OutputFormat::json_schema(schema()).with_strict(false))),
        Api::Converse,
    );
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        error.message,
        "Bedrock Converse always holds the answer to an output format's schema: a format that is not strict cannot be sent"
    );
}

#[test]
fn invoke_model_sends_the_messages_output_config_format() {
    let body = encoded(
        &turn(Some(
            OutputFormat::json_schema(schema())
                .with_name("place")
                .with_strict(true),
        )),
        Api::AnthropicInvoke,
    );
    assert_eq!(
        body["output_config"],
        json!({"format": {"type": "json_schema", "schema": schema()}})
    );
    assert_eq!(body["anthropic_version"], "bedrock-2023-05-31");
    let error = refused(
        &turn(Some(OutputFormat::json_schema(schema()).with_strict(false))),
        Api::AnthropicInvoke,
    );
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        error.message,
        "Messages always holds the answer to an output format's schema: a format that is not strict cannot be sent"
    );
}
