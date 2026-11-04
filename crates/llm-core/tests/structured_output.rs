//! A turn asks for JSON-schema structured output (story:structured-output): the neutral
//! `Controls::output_format`, absent by default, and its checks before any I/O
//! (`docs/design.md` "Structured output").
use llm_core::{
    Controls, ErrorCode, Item, MAX_OUTPUT_FORMAT_NAME_BYTES, MAX_TOOL_ARGUMENT_BYTES, OutputFormat,
    TurnDocument, TurnRequest,
};
use serde_json::{Value, json};

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {"city": {"type": "string"}},
        "required": ["city"],
        "additionalProperties": false
    })
}

fn turn(format: Option<OutputFormat>) -> TurnRequest {
    let mut request = TurnRequest::new("code", vec![Item::user("Where is the tower?")]);
    request.controls.output_format = format;
    request
}

/// Absent is the 0.20.0 turn exactly: no key, version `llm.turn/3`, and the same bytes.
#[test]
fn a_turn_without_an_output_format_keeps_its_bytes() {
    let request = turn(None);
    assert!(request.controls.is_empty());
    let written = serde_json::to_string(&TurnDocument::new(request)).unwrap();
    assert_eq!(
        written,
        r#"{"format":"llm.turn/3","request":{"model":"code","instructions":"","items":[{"kind":"user-text","text":"Where is the tower?"}],"tools":[]}}"#
    );
}

#[test]
fn an_output_format_is_a_control_written_as_turn_4_and_read_back_equal() {
    let format = OutputFormat::json_schema(schema())
        .with_name("place")
        .with_strict(true);
    let request = turn(Some(format.clone()));
    assert!(!request.controls.is_empty());
    request.validate().expect("a named object schema is valid");
    let written = serde_json::to_value(TurnDocument::new(request.clone())).unwrap();
    assert_eq!(written["format"], json!("llm.turn/4"));
    assert_eq!(
        written["request"]["controls"],
        json!({"output_format": {"name": "place", "schema": schema(), "strict": true}})
    );
    let read: TurnDocument = serde_json::from_value(written).unwrap();
    assert_eq!(read.request, request);
    assert_eq!(read.request.controls.output_format, Some(format));
}

/// A name and a strict flag nobody gave stay absent: the wire's own default decides.
#[test]
fn an_unnamed_format_without_a_strict_flag_writes_only_its_schema() {
    let format = OutputFormat::json_schema(schema());
    assert_eq!(format.name, None);
    assert_eq!(format.strict, None);
    assert_eq!(
        serde_json::to_value(&format).unwrap(),
        json!({"schema": schema()})
    );
    let controls = Controls {
        output_format: Some(format),
        ..Controls::default()
    };
    assert!(!controls.is_empty());
}

#[test]
fn a_key_outside_the_format_refuses_the_document() {
    let mut written = serde_json::to_value(TurnDocument::new(turn(Some(
        OutputFormat::json_schema(schema()).with_name("place"),
    ))))
    .unwrap();
    written["request"]["controls"]["output_format"]["description"] = json!("a place");
    assert!(serde_json::from_value::<TurnDocument>(written).is_err());
}

#[test]
fn a_name_outside_the_wire_class_is_refused_before_io() {
    let longest = "a".repeat(MAX_OUTPUT_FORMAT_NAME_BYTES);
    turn(Some(OutputFormat::json_schema(schema()).with_name(longest)))
        .validate()
        .expect("64 bytes of the class is a name");
    turn(Some(
        OutputFormat::json_schema(schema()).with_name("Az09_-"),
    ))
    .validate()
    .expect("letters, digits, '_' and '-'");
    for name in [
        String::new(),
        "a".repeat(MAX_OUTPUT_FORMAT_NAME_BYTES + 1),
        "place.v1".to_owned(),
        "a place".to_owned(),
        "plätze".to_owned(),
    ] {
        let error = turn(Some(OutputFormat::json_schema(schema()).with_name(name)))
            .validate()
            .expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(
            error.message,
            "an output format name is 1 to 64 ASCII letters, digits, '_' or '-'"
        );
    }
}

#[test]
fn a_schema_that_is_not_an_object_or_exceeds_its_bound_is_refused_before_io() {
    for schema in [json!(true), json!("object"), json!(null), json!([])] {
        let error = turn(Some(OutputFormat::json_schema(schema)))
            .validate()
            .expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(
            error.message,
            "an output format schema must be a JSON Schema object"
        );
    }
    let oversized = json!({"description": "x".repeat(MAX_TOOL_ARGUMENT_BYTES)});
    let error = turn(Some(OutputFormat::json_schema(oversized)))
        .validate()
        .expect_err("refused");
    assert_eq!(error.code, ErrorCode::TooLarge);
    assert_eq!(error.message, "output format schema exceeds its bound");
}
