//! Adversary pass 1, wave 2026-11-04-w25, story:structured-output, Messages wire.
//!
//! The acceptance reads: "A member a wire has no place for (Messages and Converse have no
//! `strict`) is refused by name before I/O, or, where `docs/design.md` "Client request fields"
//! classes it as a label the wire has no place for, written there as not sent; never dropped
//! silently". `strict` is a flag, not a label, so a format carrying it is refused by name.
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

/// Messages has no `strict`. A format whose strict flag is `true` is sent with the flag
/// silently left out; the acceptance asks for a refusal by name before any I/O.
#[test]
fn a_strict_flag_messages_has_no_place_for_is_refused_by_name_before_io() {
    let request = turn(OutputFormat::json_schema(schema()).with_strict(true));
    match encode_request(&request, &binding()) {
        Ok(bytes) => panic!(
            "strict: true was dropped, not refused; sent {}",
            String::from_utf8_lossy(&bytes)
        ),
        Err(error) => {
            assert_eq!(error.code, ErrorCode::Unsupported);
            assert_eq!(error.dispatch, Dispatch::NotSent);
            assert!(error.message.contains("strict"), "{}", error.message);
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
