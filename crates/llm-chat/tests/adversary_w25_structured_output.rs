//! Adversary pass 1, wave 2026-11-04-w25, story:structured-output, Chat Completions wire: the
//! name bounds on ingress, and a `response_format` missing a member is refused rather than read
//! without it.
mod common;

use common::binding;
use llm_chat::{decode_ingress_request, project_request};
use llm_core::{Dispatch, ErrorCode, MAX_OUTPUT_FORMAT_NAME_BYTES};
use serde_json::{Value, json};

fn schema() -> Value {
    json!({"type": "object", "properties": {"city": {"type": "string"}}})
}

fn with_format(format: &Value) -> Value {
    json!({
        "model": "small",
        "messages": [{"role": "user", "content": "Where is the tower?"}],
        "response_format": format
    })
}

#[test]
fn the_longest_name_round_trips_and_one_byte_more_is_refused_on_ingress() {
    let longest = "n".repeat(MAX_OUTPUT_FORMAT_NAME_BYTES);
    let format = json!({"type": "json_schema",
                        "json_schema": {"name": longest, "schema": schema()}});
    let request = decode_ingress_request(&with_format(&format))
        .expect("64 bytes is a name")
        .request;
    let body = project_request(&request, &binding(), false).expect("projected");
    assert_eq!(body["response_format"], format);

    for name in [
        "n".repeat(MAX_OUTPUT_FORMAT_NAME_BYTES + 1),
        String::new(),
        "a place".to_owned(),
    ] {
        let error = decode_ingress_request(&with_format(&json!({"type": "json_schema",
                     "json_schema": {"name": name, "schema": schema()}})))
        .expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{name:?}");
        assert_eq!(error.dispatch, Dispatch::NotSent, "{name:?}");
    }
}

#[test]
fn a_json_schema_format_missing_a_member_is_refused_not_read_without_it() {
    for format in [
        json!({"type": "json_schema"}),
        json!({"type": "json_schema", "json_schema": null}),
        json!({"type": "json_schema", "json_schema": {"name": "place"}}),
        json!({"type": "json_schema", "json_schema": {"schema": schema()}}),
    ] {
        let error = decode_ingress_request(&with_format(&format)).expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{format}");
        assert_eq!(error.dispatch, Dispatch::NotSent, "{format}");
    }
}
