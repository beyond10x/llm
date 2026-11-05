//! Adversary pass 1, wave 2026-11-04-w25, story:structured-output, Messages wire.
//!
//! The acceptance, amended after this pass (revision 21), reads: "`strict` on Messages, Anthropic
//! `InvokeModel` and Converse: those wires always constrain the answer to the schema ..., so
//! `strict: true` is honoured by construction and sends no member, and `strict: false` is refused
//! by name before I/O."
mod support;

use llm_core::{Dispatch, ErrorCode, Item, OutputFormat, TurnRequest};
use llm_messages::{decode_request, encode_request};
use serde_json::{Value, json};
use support::binding;

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
    request.controls.output_format = Some(format);
    request
}

fn body(extra: &[(&str, Value)]) -> Value {
    let mut body = json!({
        "model": "caller-alias",
        "max_tokens": 512,
        "messages": [{"role": "user", "content": "Where is the tower?"}]
    });
    for (key, value) in extra {
        body[*key] = value.clone();
    }
    body
}

/// Messages has no `strict` and always holds the answer to the schema: `true` is what the route
/// does anyway, so it is sent as no member; `false` asks for what the route cannot do, so it is
/// refused by name before any I/O.
#[test]
fn strict_true_sends_no_member_and_strict_false_is_refused_by_name_before_io() {
    let request = turn(OutputFormat::json_schema(schema()).with_strict(true));
    let sent = match encode_request(&request, &binding()) {
        Ok(bytes) => String::from_utf8(bytes).unwrap(),
        Err(error) => panic!(
            "story:structured-output: strict true is honoured by construction, not refused: {}",
            error.message
        ),
    };
    assert!(
        !sent.contains("\"strict\""),
        "story:structured-output: strict true sends no strict member: {sent}"
    );
    let sent: Value = serde_json::from_str(&sent).unwrap();
    assert_eq!(
        sent["output_config"],
        json!({"format": {"type": "json_schema", "schema": schema()}}),
        "story:structured-output: strict true sends the format and nothing more"
    );
    let request = turn(OutputFormat::json_schema(schema()).with_strict(false));
    match encode_request(&request, &binding()) {
        Ok(bytes) => panic!(
            "story:structured-output: strict false must be refused before I/O; sent {}",
            String::from_utf8_lossy(&bytes)
        ),
        Err(error) => {
            assert_eq!(
                error.code,
                ErrorCode::Unsupported,
                "story:structured-output: strict false is unsupported"
            );
            assert_eq!(
                error.dispatch,
                Dispatch::NotSent,
                "story:structured-output: strict false is refused before I/O"
            );
            assert!(
                error.message.contains("strict"),
                "story:structured-output: strict false is refused by name: {}",
                error.message
            );
        }
    }
}

/// The name is the documented label exception: it is not sent anywhere in the body.
#[test]
fn a_named_format_sends_its_name_nowhere() {
    let request = turn(OutputFormat::json_schema(schema()).with_name("zz_label_marker"));
    let sent = String::from_utf8(encode_request(&request, &binding()).expect("encodes")).unwrap();
    assert!(!sent.contains("zz_label_marker"), "{sent}");
    let sent: Value = serde_json::from_str(&sent).unwrap();
    assert_eq!(
        sent["output_config"],
        json!({"format": {"type": "json_schema", "schema": schema()}})
    );
}

/// The deprecated beta field is not read as the format and stays refused by its path.
#[test]
fn the_deprecated_top_level_output_format_is_refused_by_its_path() {
    let error = decode_request(
        body(&[(
            "output_format",
            json!({"type": "json_schema", "schema": schema()}),
        )])
        .to_string()
        .as_bytes(),
        binding().provenance(),
    )
    .expect_err("refused");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        error.message,
        "Messages field is outside the declared subset: output_format"
    );
}

/// A null effort beside a format reads as no effort, and the format is kept.
#[test]
fn a_format_beside_a_null_effort_reads_the_format_and_no_effort() {
    let read = decode_request(
        body(&[(
            "output_config",
            json!({"effort": null, "format": {"type": "json_schema", "schema": schema()}}),
        )])
        .to_string()
        .as_bytes(),
        binding().provenance(),
    )
    .expect("inside the subset")
    .request;
    assert_eq!(read.sampling.reasoning_effort, None);
    assert_eq!(
        read.controls.output_format,
        Some(OutputFormat::json_schema(schema()))
    );
}
