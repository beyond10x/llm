//! Adversary pass 1, wave 2026-11-04-w25, story:structured-output, Responses wire: the name
//! bounds on ingress, and a format missing a member is refused rather than read without it.

mod wire_support;

use llm_core::{Dispatch, ErrorCode, MAX_OUTPUT_FORMAT_NAME_BYTES};
use llm_responses::{Binding, ingest_request, project_request};
use serde_json::{Value, json};
use wire_support::projection;

fn schema() -> Value {
    json!({"type": "object", "properties": {"city": {"type": "string"}}})
}

fn binding() -> Binding {
    projection("http://127.0.0.1:9/v1")
}

fn wire_body(text: &Value) -> Value {
    json!({
        "model": "example/Small-Model",
        "input": [{"type": "message", "role": "user",
                   "content": [{"type": "input_text", "text": "Hi"}]}],
        "tools": [],
        "stream": true, "store": false,
        "include": ["reasoning.encrypted_content"],
        "text": text
    })
}

#[test]
fn the_longest_name_round_trips_and_one_byte_more_is_refused_on_ingress() {
    let longest = "n".repeat(MAX_OUTPUT_FORMAT_NAME_BYTES);
    let body = wire_body(&json!({"format": {"type": "json_schema", "name": longest,
                                            "schema": schema()}}));
    let request = ingest_request(&binding(), &body).expect("64 bytes is a name");
    assert_eq!(
        project_request(&binding(), &request).expect("projects"),
        body
    );

    for name in [
        "n".repeat(MAX_OUTPUT_FORMAT_NAME_BYTES + 1),
        String::new(),
        "plätze".to_owned(),
    ] {
        let error = ingest_request(
            &binding(),
            &wire_body(&json!({"format": {"type": "json_schema", "name": name,
                                          "schema": schema()}})),
        )
        .expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{name:?}");
        assert_eq!(error.dispatch, Dispatch::NotSent, "{name:?}");
    }
}

#[test]
fn a_json_schema_format_without_a_name_or_schema_is_refused_not_read_without_it() {
    for format in [
        json!({"type": "json_schema", "schema": schema()}),
        json!({"type": "json_schema", "name": "place"}),
    ] {
        let error = ingest_request(&binding(), &wire_body(&json!({"format": format})))
            .expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{format}");
    }
}
