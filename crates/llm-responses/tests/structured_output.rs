//! JSON-schema structured output on the Responses wire (story:structured-output): the turn's
//! `controls.output_format` is `text.format` in both directions (`docs/responses.md`,
//! `docs/design.md` "Structured output").

mod wire_support;

use llm_core::{
    Cancel, Dispatch, ErrorCode, Item, Model, OutputFormat, TurnRequest, VecSink, Verbosity,
};
use llm_responses::{Binding, ingest_request, project_request};
use serde_json::{Value, json};
use wire_support::{TEXT_STREAM, body_fixture, client, listener, projection, serve};

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {"city": {"type": "string"}},
        "required": ["city"],
        "additionalProperties": false
    })
}

fn binding() -> Binding {
    projection("http://127.0.0.1:9/v1")
}

fn turn(format: OutputFormat) -> TurnRequest {
    let mut request = TurnRequest::new("small", vec![Item::user("Where is the tower?")]);
    "answer in JSON".clone_into(&mut request.instructions);
    request.controls.output_format = Some(format);
    request
}

/// One authored wire body, `text` set to `text`.
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

fn refused(body: &Value) -> llm_core::Error {
    match ingest_request(&binding(), body) {
        Ok(request) => panic!("accepted, returning {request:?} for {body}"),
        Err(error) => {
            assert_eq!(error.dispatch, Dispatch::NotSent, "{body}");
            error
        }
    }
}

/// The bytes the client sends for a turn asking for structured output, recorded in
/// `tests/fixtures/structured-output-request.json`. Mutation: any change to `text.format`, its
/// members, their order or their encoding.
#[tokio::test]
async fn the_recorded_structured_output_request_matches_its_fixture() {
    let (listener, url) = listener().await;
    let server = serve(listener, vec![TEXT_STREAM.to_vec()]);
    let client = client(&url);
    let request = turn(
        OutputFormat::json_schema(schema())
            .with_name("place")
            .with_strict(true),
    );
    let mut sink = VecSink::new(16, 4096);
    // What the turn answers is not this case's business; the request it sent is.
    let _ = client.turn(&request, &mut sink, &Cancel::new()).await;
    let captured = server.await.expect("the fixture server").remove(0);
    assert_eq!(
        String::from_utf8_lossy(&captured.body),
        String::from_utf8_lossy(&body_fixture("structured-output-request.json")),
        "the exact request bytes changed; regenerate the fixture only on purpose"
    );
}

#[test]
fn the_format_is_written_as_text_format_beside_verbosity() {
    let mut request = turn(
        OutputFormat::json_schema(schema())
            .with_name("place")
            .with_strict(false),
    );
    request.controls.hints.verbosity = Some(Verbosity::Low);
    let body = project_request(&binding(), &request).expect("projects");
    assert_eq!(
        body["text"],
        json!({
            "format": {"type": "json_schema", "name": "place", "schema": schema(), "strict": false},
            "verbosity": "low"
        })
    );
}

/// Absent stays absent: a strict flag nobody set is the provider's default to decide.
#[test]
fn a_strict_flag_nobody_set_is_not_written() {
    let body = project_request(
        &binding(),
        &turn(OutputFormat::json_schema(schema()).with_name("place")),
    )
    .expect("projects");
    assert_eq!(
        body["text"],
        json!({"format": {"type": "json_schema", "name": "place", "schema": schema()}})
    );
}

/// The wire requires a name and llm invents none.
#[test]
fn a_format_without_a_name_is_refused_before_io() {
    let error = project_request(&binding(), &turn(OutputFormat::json_schema(schema())))
        .expect_err("no name to send");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(error.dispatch, Dispatch::NotSent);
    assert_eq!(
        error.message,
        "Responses needs a name for a JSON Schema output format"
    );
}

#[test]
fn ingress_reads_text_format_and_the_turn_projects_back_to_the_same_body() {
    for format in [
        json!({"type": "json_schema", "name": "place", "schema": schema(), "strict": true}),
        json!({"type": "json_schema", "name": "place", "schema": schema(), "strict": false}),
        json!({"type": "json_schema", "name": "place", "schema": schema()}),
    ] {
        let body = wire_body(&json!({"format": format, "verbosity": "high"}));
        let request = ingest_request(&binding(), &body).expect("inside the subset");
        let expected = OutputFormat::json_schema(schema()).with_name("place");
        let expected = match format.get("strict").and_then(Value::as_bool) {
            Some(strict) => expected.with_strict(strict),
            None => expected,
        };
        assert_eq!(request.controls.output_format, Some(expected), "{format}");
        assert_eq!(request.controls.hints.verbosity, Some(Verbosity::High));
        assert_eq!(
            project_request(&binding(), &request).expect("projects"),
            body,
            "{format}"
        );
    }
}

/// `{"type": "text"}` is this wire's documented default, which a turn without a format asks for.
#[test]
fn ingress_reads_the_text_type_as_no_format() {
    let request = ingest_request(&binding(), &wire_body(&json!({"format": {"type": "text"}})))
        .expect("the default format");
    assert_eq!(request.controls.output_format, None);
    let request =
        ingest_request(&binding(), &wire_body(&json!({"format": null}))).expect("an explicit null");
    assert_eq!(request.controls.output_format, None);
}

/// JSON mode asks for any JSON at all, which no JSON Schema the neutral turn holds says.
#[test]
fn ingress_refuses_json_mode_by_its_type() {
    let error = refused(&wire_body(&json!({"format": {"type": "json_object"}})));
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        error.message,
        "`text.format` of type `json_object` is outside the pinned subset: the neutral output format is a JSON Schema"
    );
    for kind in [json!("grammar"), json!(7), Value::Null] {
        let mut format = json!({"name": "place", "schema": schema()});
        if !kind.is_null() {
            format["type"] = kind.clone();
        }
        let error = refused(&wire_body(&json!({"format": format})));
        assert_eq!(error.code, ErrorCode::Unsupported, "{kind}");
        assert_eq!(
            error.message, "`text.format.type` is outside the pinned subset",
            "{kind}"
        );
    }
}

#[test]
fn a_key_inside_text_format_outside_the_subset_is_refused_by_its_path() {
    for (format, path) in [
        (
            json!({"type": "json_schema", "name": "place", "schema": schema(), "description": "d"}),
            "text.format.description",
        ),
        (
            json!({"type": "json_schema", "name": "place", "schema": schema(), "foo": 1}),
            "text.format.foo",
        ),
        (json!({"type": "text", "name": "place"}), "text.format.name"),
    ] {
        let error = refused(&wire_body(&json!({"format": format})));
        assert_eq!(error.code, ErrorCode::Unsupported, "{path}");
        assert_eq!(
            error.message,
            format!("request body carries a field outside the pinned subset: {path}")
        );
    }
}

#[test]
fn a_member_of_the_wrong_type_is_refused_as_invalid() {
    for (format, message) in [
        (json!("json_schema"), "`text.format` is not an object"),
        (
            json!({"type": "json_schema", "schema": schema()}),
            "`text.format.name` is not a string",
        ),
        (
            json!({"type": "json_schema", "name": 7, "schema": schema()}),
            "`text.format.name` is not a string",
        ),
        (
            json!({"type": "json_schema", "name": "place"}),
            "`text.format.schema` is not an object",
        ),
        (
            json!({"type": "json_schema", "name": "place", "schema": true}),
            "`text.format.schema` is not an object",
        ),
        (
            json!({"type": "json_schema", "name": "place", "schema": schema(), "strict": "yes"}),
            "`text.format.strict` is not a boolean",
        ),
    ] {
        let error = refused(&wire_body(&json!({"format": format})));
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{format}");
        assert_eq!(error.message, message, "{format}");
    }
    // A name the neutral turn cannot hold is the turn's own refusal.
    let error = refused(&wire_body(&json!({"format":
        {"type": "json_schema", "name": "place.v1", "schema": schema()}})));
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert_eq!(
        error.message,
        "an output format name is 1 to 64 ASCII letters, digits, '_' or '-'"
    );
}
