//! JSON-schema structured output on the Messages wire (story:structured-output): the turn's
//! `controls.output_format` is `output_config.format` in both directions (`docs/messages.md`,
//! `docs/design.md` "Structured output").
//!
//! The route always holds the answer to the schema, so a strict flag `true` is honoured by
//! sending nothing more, `false` is refused, and the format's name, a label this wire has no
//! place for, is not sent.
mod support;

use llm_core::{Dispatch, Error, ErrorCode, Item, OutputFormat, TurnRequest};
use llm_messages::{decode_request, encode_request};
use serde_json::{Value, json};
use support::binding;

const RECORDED_BODY: &str = include_str!("fixtures/structured-output-request.json");

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {"city": {"type": "string"}},
        "required": ["city"],
        "additionalProperties": false
    })
}

fn turn(format: OutputFormat) -> TurnRequest {
    let mut request = TurnRequest::new("internal-model", vec![Item::user("Where is the tower?")]);
    "answer in JSON".clone_into(&mut request.instructions);
    request.controls.output_format = Some(format);
    request
}

fn encoded(request: &TurnRequest) -> Value {
    serde_json::from_slice(&encode_request(request, &binding()).expect("encodes"))
        .expect("JSON bytes")
}

fn with_config(config: &Value) -> Value {
    json!({
        "model": "caller-alias",
        "max_tokens": 512,
        "messages": [{"role": "user", "content": "Where is the tower?"}],
        "output_config": config
    })
}

fn decode(body: &Value) -> Result<TurnRequest, Error> {
    decode_request(body.to_string().as_bytes(), binding().provenance())
        .map(|ingress| ingress.request)
}

fn refused(body: &Value) -> Error {
    match decode(body) {
        Ok(request) => panic!("{body} was accepted as {request:?}"),
        Err(error) => {
            assert_eq!(error.dispatch, Dispatch::NotSent, "{body}");
            error
        }
    }
}

/// The bytes `encode_request` returns, which are the bytes `MessagesClient` sends, for a named
/// strict format: recorded in `tests/fixtures/structured-output-request.json`. Mutation: any
/// change to `output_config.format`, or a name or strict flag written into it.
#[test]
fn the_recorded_structured_output_request_matches_its_fixture() {
    let request = turn(
        OutputFormat::json_schema(schema())
            .with_name("place")
            .with_strict(true),
    );
    assert_eq!(
        String::from_utf8(encode_request(&request, &binding()).expect("encodes")).unwrap(),
        RECORDED_BODY.strip_suffix('\n').expect("one final newline"),
        "the exact request bytes changed; regenerate the fixture only on purpose"
    );
}

#[test]
fn the_format_shares_output_config_with_the_effort() {
    let mut request = turn(OutputFormat::json_schema(schema()));
    request.sampling.reasoning_effort = Some("high".to_owned());
    assert_eq!(
        encoded(&request)["output_config"],
        json!({"effort": "high", "format": {"type": "json_schema", "schema": schema()}})
    );
}

/// The route always holds the answer to the schema: it cannot be asked not to.
#[test]
fn a_format_that_is_not_strict_is_refused_before_io() {
    let error = encode_request(
        &turn(OutputFormat::json_schema(schema()).with_strict(false)),
        &binding(),
    )
    .expect_err("refused");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(error.dispatch, Dispatch::NotSent);
    assert_eq!(
        error.message,
        "Messages always holds the answer to an output format's schema: a format that is not strict cannot be sent"
    );
}

#[test]
fn ingress_reads_output_config_format_and_round_trips() {
    for config in [
        json!({"format": {"type": "json_schema", "schema": schema()}}),
        json!({"effort": "high", "format": {"type": "json_schema", "schema": schema()}}),
    ] {
        let mut first = decode(&with_config(&config)).expect("inside the subset");
        // The caller's alias routes to the fixture binding's model.
        "internal-model".clone_into(&mut first.model);
        assert_eq!(
            first.controls.output_format,
            Some(OutputFormat::json_schema(schema())),
            "{config}"
        );
        let wire = encoded(&first);
        assert_eq!(wire["output_config"], config);
        let mut second = decode(&wire).expect("decodes back");
        second.model.clone_from(&first.model);
        // Every projection places the cache breakpoints, which read back as the cache hint.
        second
            .controls
            .hints
            .prompt_cache
            .take()
            .expect("the breakpoints every projection places");
        assert_eq!(second, first, "{config}");
    }
}

#[test]
fn a_null_format_is_absent_and_an_empty_config_is_refused_as_before() {
    let request =
        decode(&with_config(&json!({"effort": "high", "format": null}))).expect("an explicit null");
    assert_eq!(request.controls.output_format, None);
    assert_eq!(request.sampling.reasoning_effort.as_deref(), Some("high"));
    let error = refused(&with_config(&json!({})));
    assert_eq!(error.code, ErrorCode::Protocol);
    assert_eq!(error.message, "Messages string field is missing or invalid");
}

#[test]
fn a_key_inside_the_format_outside_the_subset_is_refused_by_its_path() {
    for (format, path) in [
        (
            json!({"type": "json_schema", "schema": schema(), "name": "place"}),
            "output_config.format.name",
        ),
        (
            json!({"type": "json_schema", "schema": schema(), "strict": true}),
            "output_config.format.strict",
        ),
    ] {
        let error = refused(&with_config(&json!({"format": format})));
        assert_eq!(error.code, ErrorCode::Unsupported, "{path}");
        assert_eq!(
            error.message,
            format!("Messages field is outside the declared subset: {path}")
        );
    }
}

#[test]
fn a_format_of_another_type_or_shape_is_refused() {
    let error = refused(&with_config(
        &json!({"format": {"type": "json_object", "schema": schema()}}),
    ));
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        error.message,
        "Messages output format type is outside the declared subset"
    );
    for format in [json!("json_schema"), json!({"schema": schema()})] {
        let error = refused(&with_config(&json!({"format": format})));
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{format}");
        assert_eq!(error.message, "Messages output format must name its type");
    }
    for schema in [json!(true), json!("object")] {
        let error = refused(&with_config(
            &json!({"format": {"type": "json_schema", "schema": schema}}),
        ));
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{schema}");
        assert_eq!(
            error.message,
            "Messages output format schema must be an object"
        );
    }
    let error = refused(&with_config(&json!({"format": {"type": "json_schema"}})));
    assert_eq!(error.code, ErrorCode::InvalidRequest);
}
