//! The single-attempt Responses client over local sockets. No provider is contacted.
//!
//! Every fixture is a literal stream served from `127.0.0.1`, and every credential comes from an
//! injected resolver; nothing is read from this machine.

use llm_core::{
    AuthKind, BillingKind, BoxFuture, CallId, Cancel, Capabilities, Dispatch, ErrorCode, Id, Item,
    Model, Protocol, StopReason, StreamEvent, ToolCall, ToolChoice, ToolName, ToolSpec,
    TurnRequest, VecSink,
};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_http::{HttpClient, Limits};
use llm_providers::{
    Account, BaseUrl, Binding, BindingDocument, Endpoint, Provider, ServedModel, ServingModel,
};
use llm_responses::ResponsesClient;
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

/// One Responses answer carrying a single `function_call`, as the wire streams it.
const STREAM: &[u8] = b"event: response.created\n\
data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"status\":\"in_progress\"}}\n\n\
event: response.output_item.added\n\
data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"file_read\",\"arguments\":\"\"}}\n\n\
event: response.function_call_arguments.delta\n\
data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"output_index\":0,\"delta\":\"{\\\"path\\\":\"}\n\n\
event: response.function_call_arguments.delta\n\
data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"output_index\":0,\"delta\":\"\\\"README.md\\\"}\"}\n\n\
event: response.function_call_arguments.done\n\
data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc_1\",\"output_index\":0,\"arguments\":\"{\\\"path\\\":\\\"README.md\\\"}\"}\n\n\
event: response.output_item.done\n\
data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"file_read\",\"arguments\":\"{\\\"path\\\":\\\"README.md\\\"}\",\"status\":\"completed\"}}\n\n\
event: response.completed\n\
data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"model\":\"example/Small-Model\",\"status\":\"completed\",\"output\":[{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"file_read\",\"arguments\":\"{\\\"path\\\":\\\"README.md\\\"}\",\"status\":\"completed\"}],\"usage\":{\"input_tokens\":20,\"output_tokens\":9}}}\n\n";

/// The bytes of [`STREAM`] before its terminal `response.completed` event, cut on an event
/// boundary so that whatever fails afterwards is not a framing error.
fn before_completion() -> &'static [u8] {
    let marker = b"event: response.completed\n";
    let end = STREAM
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("the fixture carries a terminal event");
    &STREAM[..end]
}

const SSE_HEAD: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";

fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
}

fn capabilities() -> Capabilities {
    Capabilities {
        tools: true,
        tool_choice: true,
        temperature: true,
        top_p: true,
        reasoning_efforts: vec!["medium".to_owned()],
        context_window: 32_768,
        max_output_tokens: 2_048,
    }
}

/// A bearer-authenticated Responses binding at `base_url`. The account names the credential by
/// reference only; the resolver decides what it is at request time.
fn binding(base_url: &str) -> Binding {
    BindingDocument::new(
        Provider {
            id: id("my-lab"),
            category: id("hosted"),
        },
        Account {
            id: id("local"),
            provider_id: id("my-lab"),
            auth_kind: AuthKind::Bearer,
            billing_kind: BillingKind::Metered,
            secret_reference_id: Some(SecretRef::new("responses-key").expect("reference")),
            api_key_header: None,
        },
        Endpoint {
            id: id("local-endpoint"),
            account_id: id("local"),
            base_url: BaseUrl::new(base_url).expect("fixture URL"),
        },
        ServedModel {
            id: id("small"),
            upstream_name: id("example/Small-Model"),
        },
        ServingModel {
            id: id("serving"),
            endpoint_id: id("local-endpoint"),
            model_id: id("small"),
            protocol: Protocol::Responses,
            capabilities: capabilities(),
        },
    )
    .bind()
    .expect("a valid fixture binding")
}

/// An injected resolver holding one fixture credential, or one fixed failure.
struct StaticResolver {
    failure: Option<SecretError>,
}

impl SecretResolver for StaticResolver {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move {
            if let Some(failure) = self.failure {
                return Err(failure);
            }
            Ok(ResolvedSecret {
                secret: Secret::new(b"fixture-token".to_vec())?,
                version: SecretVersion::new("generation-1".to_owned())?,
            })
        })
    }
}

fn client(url: &str, failure: Option<SecretError>) -> ResponsesClient {
    let total = Duration::from_secs(10);
    ResponsesClient::new(
        binding(url),
        HttpClient::new(Limits {
            response_headers: total,
            idle: total,
            total,
        })
        .expect("bounded transport"),
        Arc::new(StaticResolver { failure }),
    )
    .expect("a Responses binding")
}

/// A turn that forces one named tool.
fn request() -> TurnRequest {
    let mut request = TurnRequest::new("small", vec![Item::user("Read the readme")]);
    request.tools = vec![ToolSpec {
        name: ToolName::new("file_read").expect("tool name"),
        description: "Read one file".to_owned(),
        input_schema: json!({"type": "object"}),
    }];
    request.tool_choice = ToolChoice::Named(ToolName::new("file_read").expect("tool name"));
    request.max_output_tokens = Some(256);
    request
}

async fn listener() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
    (listener, url)
}

/// Bounded so that a client which never connects fails the test instead of hanging it.
async fn accept(listener: &TcpListener) -> TcpStream {
    tokio::time::timeout(Duration::from_secs(10), listener.accept())
        .await
        .expect("the client never opened a connection")
        .expect("an accepted connection")
        .0
}

/// Reads one whole request: its head and the body its `content-length` declares.
async fn read_request(socket: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        let length = socket.read(&mut buffer).await.expect("a readable socket");
        assert!(length > 0, "the client closed before sending a request");
        bytes.extend_from_slice(&buffer[..length]);
        assert!(bytes.len() < 256 * 1024);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let length: usize = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .expect("a bounded body")
                .trim()
                .parse()
                .expect("a numeric content-length");
            if bytes.len() >= end + 4 + length {
                return String::from_utf8(bytes).expect("UTF-8 request");
            }
        }
    }
}

/// Asserts that no second connection arrives: one attempt, no retry, no fallback.
async fn assert_no_second_attempt(listener: &TcpListener) {
    assert!(
        tokio::time::timeout(Duration::from_millis(150), listener.accept())
            .await
            .is_err(),
        "the client attempted the request a second time"
    );
}

#[tokio::test]
async fn a_responses_turn_returns_its_function_call() {
    let (listener, url) = listener().await;
    let server = tokio::spawn(async move {
        let mut socket = accept(&listener).await;
        let captured = read_request(&mut socket).await;
        socket.write_all(SSE_HEAD).await.expect("head written");
        socket.write_all(STREAM).await.expect("stream written");
        socket.shutdown().await.expect("closed");
        assert_no_second_attempt(&listener).await;
        captured
    });
    let client = client(&url, None);
    let mut sink = VecSink::new(16, 4096);
    let outcome = client
        .turn(&request(), &mut sink, &Cancel::new())
        .await
        .expect("a decoded turn");
    let captured = server.await.expect("the fixture server");

    // One streaming POST to `{base_url}responses`, carrying the resolver's bearer.
    let (head, body) = captured.split_once("\r\n\r\n").expect("a head and a body");
    let head = head.to_ascii_lowercase();
    assert!(head.starts_with("post /v1/responses "), "{head}");
    assert!(
        head.lines()
            .any(|line| line == "authorization: bearer fixture-token"),
        "{head}"
    );
    assert!(
        head.lines().any(|line| line == "accept: text/event-stream"),
        "{head}"
    );
    assert!(
        head.lines()
            .any(|line| line == "content-type: application/json"),
        "{head}"
    );
    // Headers are fixed by the binding: nothing the caller did not declare, so no originator,
    // session or account header rides along.
    let names: BTreeSet<&str> = head
        .lines()
        .skip(1)
        .filter_map(|line| line.split_once(':').map(|(name, _)| name.trim()))
        .collect();
    assert_eq!(
        names,
        BTreeSet::from([
            "accept",
            "authorization",
            "content-length",
            "content-type",
            "host"
        ]),
        "{head}"
    );

    // The request body is the existing projection, and it names the forced tool.
    let body: Value = serde_json::from_str(body).expect("a JSON body");
    assert_eq!(body["model"], json!("example/Small-Model"));
    assert_eq!(body["stream"], json!(true));
    assert_eq!(body["tools"][0]["name"], json!("file_read"));
    assert_eq!(
        body["tool_choice"],
        json!({"type": "function", "name": "file_read"})
    );

    // The outcome carries the streamed call and its arguments.
    assert_eq!(outcome.stop_reason, StopReason::ToolCalls);
    assert_eq!(
        outcome.tool_calls().cloned().collect::<Vec<_>>(),
        vec![ToolCall {
            call_id: CallId::new("call_1").expect("call id"),
            name: ToolName::new("file_read").expect("tool name"),
            arguments: json!({"path": "README.md"}),
        }]
    );
    assert!(outcome.observation.final_usage);
    assert_eq!(
        outcome
            .observation
            .usage
            .expect("reported usage")
            .input_tokens,
        Some(20)
    );
    assert_eq!(
        sink.events(),
        [
            StreamEvent::ToolCallStarted {
                call_id: CallId::new("call_1").expect("call id"),
                name: ToolName::new("file_read").expect("tool name"),
            },
            StreamEvent::ToolArgumentsDelta {
                call_id: CallId::new("call_1").expect("call id"),
                delta: "{\"path\":".to_owned(),
            },
            StreamEvent::ToolArgumentsDelta {
                call_id: CallId::new("call_1").expect("call id"),
                delta: "\"README.md\"}".to_owned(),
            },
        ]
    );
}

/// The connection drops after the call streamed and before `response.completed`: the body the
/// server declared never arrives, which is a transport failure after dispatch.
#[tokio::test]
async fn a_stream_cut_off_before_completion_gives_the_transport_error() {
    let (listener, url) = listener().await;
    let server = tokio::spawn(async move {
        let mut socket = accept(&listener).await;
        read_request(&mut socket).await;
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            STREAM.len()
        );
        socket
            .write_all(head.as_bytes())
            .await
            .expect("head written");
        socket
            .write_all(before_completion())
            .await
            .expect("partial stream written");
        socket.shutdown().await.expect("closed");
        assert_no_second_attempt(&listener).await;
    });
    let client = client(&url, None);
    let mut sink = VecSink::new(16, 4096);
    let error = client
        .turn(&request(), &mut sink, &Cancel::new())
        .await
        .expect_err("a stream that never completed is not a turn");
    server.await.expect("the fixture server");
    assert_eq!(error.code, ErrorCode::Transport, "{error}");
    assert_eq!(error.dispatch, Dispatch::Accepted, "{error}");
}

/// The server closes cleanly before `response.completed`. End of body is not terminal truth on
/// this wire, and `docs/responses.md` names the refusal: `Protocol`, never a manufactured turn.
#[tokio::test]
async fn a_stream_closed_cleanly_before_completion_is_refused_not_completed() {
    let (listener, url) = listener().await;
    let server = tokio::spawn(async move {
        let mut socket = accept(&listener).await;
        read_request(&mut socket).await;
        socket.write_all(SSE_HEAD).await.expect("head written");
        socket
            .write_all(before_completion())
            .await
            .expect("partial stream written");
        socket.shutdown().await.expect("closed");
        assert_no_second_attempt(&listener).await;
    });
    let client = client(&url, None);
    let mut sink = VecSink::new(16, 4096);
    let error = client
        .turn(&request(), &mut sink, &Cancel::new())
        .await
        .expect_err("end of body never manufactures success");
    server.await.expect("the fixture server");
    assert_eq!(error.code, ErrorCode::Protocol, "{error}");
    assert_ne!(error.dispatch, Dispatch::NotSent, "{error}");
}

#[tokio::test]
async fn a_resolver_error_gives_the_credential_error_before_any_request_is_sent() {
    let (listener, url) = listener().await;
    let client = client(&url, Some(SecretError::Missing));
    let mut sink = VecSink::new(16, 4096);
    let error = client
        .turn(&request(), &mut sink, &Cancel::new())
        .await
        .expect_err("no credential, no turn");
    assert_eq!(error.code, ErrorCode::Unauthorized, "{error}");
    assert_eq!(error.dispatch, Dispatch::NotSent, "{error}");
    assert_eq!(sink.events(), []);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err(),
        "an unresolved credential still opened a connection"
    );
}
