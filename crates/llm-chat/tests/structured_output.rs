//! JSON-schema structured output on the Chat Completions wire (story:structured-output): the
//! turn's `controls.output_format` is `response_format` in both directions (`docs/chat.md`,
//! `docs/design.md` "Structured output").
mod common;

use common::{binding, binding_at, capable};
use llm_chat::{
    ChatClient, NAMED_UNSUPPORTED_REQUEST_FIELDS, SUPPORTED_REQUEST_FIELDS, decode_ingress_request,
    project_request,
};
use llm_core::{
    Cancel, Dispatch, ErrorCode, Item, Model, OutputFormat, Protocol, TurnRequest, VecSink,
};
use llm_credentials::{ResolvedSecret, SecretError, SecretRef, SecretResolver};
use llm_http::{HttpClient, Limits};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const OPENAI_TEXT: &[u8] = include_bytes!("../fixtures/openai-text-and-usage.sse");
const RECORDED_BODY: &str = include_str!("../fixtures/structured-output-request.json");

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {"city": {"type": "string"}},
        "required": ["city"],
        "additionalProperties": false
    })
}

fn turn(format: OutputFormat) -> TurnRequest {
    let mut request = TurnRequest::new("small", vec![Item::user("Where is the tower?")]);
    "answer in JSON".clone_into(&mut request.instructions);
    request.controls.output_format = Some(format);
    request
}

fn with_format(format: &Value) -> Value {
    json!({
        "model": "small",
        "messages": [{"role": "user", "content": "Where is the tower?"}],
        "response_format": format
    })
}

fn refused(body: &Value) -> llm_core::Error {
    let error = decode_ingress_request(body).expect_err("refused");
    assert_eq!(error.dispatch, Dispatch::NotSent, "{body}");
    error
}

/// An anonymous binding never reaches a resolver.
struct NoResolver;
impl SecretResolver for NoResolver {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> llm_core::BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async { Err(SecretError::Missing) })
    }
}

/// Accepts one request, answers the pinned stream, and hands back the request body.
async fn serve_once(body: &'static [u8]) -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bound");
    let port = listener.local_addr().expect("address").port();
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accepted");
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        let sent = loop {
            let read = socket.read(&mut buffer).await.expect("read");
            request.extend_from_slice(&buffer[..read]);
            let text = String::from_utf8_lossy(&request).into_owned();
            let body = text.split_once("\r\n\r\n").and_then(|(head, rest)| {
                head.to_ascii_lowercase()
                    .split_once("content-length:")
                    .and_then(|(_, tail)| tail.split(['\r', ';']).next())
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .filter(|length| rest.len() >= *length)
                    .map(|_| rest.to_owned())
            });
            if let Some(body) = body {
                break body;
            }
            assert!(read != 0, "the request ended before its body");
        };
        let mut response = Vec::from(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
        );
        response.extend_from_slice(body);
        socket.write_all(&response).await.expect("written");
        socket.shutdown().await.expect("closed");
        sent
    });
    (format!("http://127.0.0.1:{port}/v1"), handle)
}

/// The bytes the client sends for a turn asking for structured output, recorded in
/// `fixtures/structured-output-request.json`. Mutation: any change to `response_format`, its
/// members, their order or their encoding.
#[tokio::test]
async fn the_recorded_structured_output_request_matches_its_fixture() {
    let (base_url, server) = serve_once(OPENAI_TEXT).await;
    let client = ChatClient::new(
        binding_at(Protocol::ChatCompletions, capable(), &base_url),
        HttpClient::new(Limits::default()).expect("client"),
        Arc::new(NoResolver),
    );
    let request = turn(
        OutputFormat::json_schema(schema())
            .with_name("place")
            .with_strict(true),
    );
    let mut sink = VecSink::new(64, 64 * 1024);
    let model: &dyn Model = &client;
    // What the turn answers is not this case's business; the request it sent is.
    let _ = model.turn(&request, &mut sink, &Cancel::new()).await;
    let sent = server.await.expect("the fixture server");
    assert_eq!(
        sent,
        RECORDED_BODY.strip_suffix('\n').expect("one final newline"),
        "the exact request bytes changed; regenerate the fixture only on purpose"
    );
}

#[test]
fn the_format_is_written_as_response_format() {
    let body = project_request(
        &turn(
            OutputFormat::json_schema(schema())
                .with_name("place")
                .with_strict(false),
        ),
        &binding(),
        false,
    )
    .expect("projected");
    assert_eq!(
        body["response_format"],
        json!({"type": "json_schema",
               "json_schema": {"name": "place", "schema": schema(), "strict": false}})
    );
    // Absent stays absent: a strict flag nobody set is the provider's default to decide.
    let body = project_request(
        &turn(OutputFormat::json_schema(schema()).with_name("place")),
        &binding(),
        false,
    )
    .expect("projected");
    assert_eq!(
        body["response_format"],
        json!({"type": "json_schema", "json_schema": {"name": "place", "schema": schema()}})
    );
}

/// The wire requires a name and llm invents none.
#[test]
fn a_format_without_a_name_is_refused_before_io() {
    let error = project_request(&turn(OutputFormat::json_schema(schema())), &binding(), true)
        .expect_err("no name to send");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(error.dispatch, Dispatch::NotSent);
    assert_eq!(
        error.message,
        "Chat Completions needs a name for a JSON Schema output format"
    );
}

#[test]
fn response_format_left_the_named_unsupported_list() {
    assert!(!NAMED_UNSUPPORTED_REQUEST_FIELDS.contains(&"response_format"));
    assert!(SUPPORTED_REQUEST_FIELDS.contains(&"response_format"));
}

#[test]
fn ingress_reads_response_format_and_the_turn_projects_back_to_the_same_format() {
    for format in [
        json!({"type": "json_schema",
               "json_schema": {"name": "place", "schema": schema(), "strict": true}}),
        json!({"type": "json_schema",
               "json_schema": {"name": "place", "schema": schema(), "strict": false}}),
        json!({"type": "json_schema", "json_schema": {"name": "place", "schema": schema()}}),
    ] {
        let request = decode_ingress_request(&with_format(&format))
            .expect("inside the subset")
            .request;
        let expected = OutputFormat::json_schema(schema()).with_name("place");
        let expected = match format["json_schema"].get("strict").and_then(Value::as_bool) {
            Some(strict) => expected.with_strict(strict),
            None => expected,
        };
        assert_eq!(request.controls.output_format, Some(expected), "{format}");
        let body = project_request(&request, &binding(), false).expect("projected");
        assert_eq!(body["response_format"], format);
        // The body names the binding's upstream model, not the caller's alias.
        let mut back = decode_ingress_request(&body).expect("decodes back").request;
        back.model.clone_from(&request.model);
        assert_eq!(back, request, "{format}");
    }
}

/// `{"type": "text"}` is this wire's documented default, which a turn without a format asks for.
#[test]
fn ingress_reads_the_text_type_and_null_as_no_format() {
    for format in [json!({"type": "text"}), Value::Null] {
        let request = decode_ingress_request(&with_format(&format))
            .expect("the default format")
            .request;
        assert_eq!(request.controls.output_format, None, "{format}");
    }
}

/// JSON mode asks for any JSON at all, which no JSON Schema the neutral turn holds says.
#[test]
fn ingress_refuses_json_mode_by_its_type() {
    let error = refused(&with_format(&json!({"type": "json_object"})));
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        error.message,
        "chat response_format of type `json_object` is outside the published subset: the neutral output format is a JSON Schema"
    );
    for kind in [json!("grammar"), json!(7), Value::Null] {
        let mut format = json!({"json_schema": {"name": "place", "schema": schema()}});
        if !kind.is_null() {
            format["type"] = kind.clone();
        }
        let error = refused(&with_format(&format));
        assert_eq!(error.code, ErrorCode::Unsupported, "{kind}");
        assert_eq!(
            error.message, "chat response_format type is outside the published subset",
            "{kind}"
        );
    }
}

#[test]
fn a_key_inside_response_format_outside_the_subset_is_refused_by_its_path() {
    for (format, path) in [
        (
            json!({"type": "json_schema", "json_schema": {"name": "place", "schema": schema()},
                   "strict": true}),
            "response_format.strict",
        ),
        (
            json!({"type": "json_schema",
                   "json_schema": {"name": "place", "schema": schema(), "description": "d"}}),
            "response_format.json_schema.description",
        ),
        (
            json!({"type": "text", "json_schema": {"name": "place", "schema": schema()}}),
            "response_format.json_schema",
        ),
    ] {
        let error = refused(&with_format(&format));
        assert_eq!(error.code, ErrorCode::Unsupported, "{path}");
        assert_eq!(
            error.message,
            format!("chat request field is outside the published subset: {path}")
        );
    }
}

#[test]
fn a_member_of_the_wrong_type_is_refused_as_invalid() {
    for (format, message) in [
        (
            json!("json_schema"),
            "chat request response_format is not an object",
        ),
        (
            json!({"type": "json_schema"}),
            "chat response_format json_schema is not an object",
        ),
        (
            json!({"type": "json_schema", "json_schema": {"schema": schema()}}),
            "chat response_format names no format name",
        ),
        (
            json!({"type": "json_schema", "json_schema": {"name": "place"}}),
            "chat response_format schema is not an object",
        ),
        (
            json!({"type": "json_schema", "json_schema": {"name": "place", "schema": []}}),
            "chat response_format schema is not an object",
        ),
        (
            json!({"type": "json_schema",
                   "json_schema": {"name": "place", "schema": schema(), "strict": 1}}),
            "chat response_format strict is not a boolean",
        ),
    ] {
        let error = refused(&with_format(&format));
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{format}");
        assert_eq!(error.message, message, "{format}");
    }
    // A name the neutral turn cannot hold is the turn's own refusal.
    let error = refused(&with_format(&json!({"type": "json_schema",
        "json_schema": {"name": "a place", "schema": schema()}})));
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert_eq!(
        error.message,
        "an output format name is 1 to 64 ASCII letters, digits, '_' or '-'"
    );
}
