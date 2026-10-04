//! Adversary cases for `ResponsesClient`, over local sockets only. No provider is contacted.
//!
//! Each case states the contract it drives. A case that is red names the defect it found; a green
//! one records an attack that did not break the client.

use llm_core::{
    AuthKind, BillingKind, BoxFuture, CallId, Cancel, Capabilities, Dispatch, ErrorCode, Id, Item,
    MAX_REQUEST_BYTES, Model, Protocol, StopReason, StreamEvent, ToolCall, ToolChoice, ToolName,
    ToolSpec, TurnRequest, VecSink, encoded_len,
};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_http::{HttpClient, Limits};
use llm_providers::{
    Account, BaseUrl, Binding, BindingDocument, Endpoint, Provider, ServedModel, ServingModel,
};
use llm_responses::{ResponsesClient, project_request};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const TOKEN: &str = "fixture-token";

/// One Responses answer carrying a single `function_call` to `file_read`.
const STREAM: &str = "event: response.created\n\
data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"status\":\"in_progress\"}}\n\n\
event: response.output_item.added\n\
data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"file_read\",\"arguments\":\"\"}}\n\n\
event: response.function_call_arguments.delta\n\
data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"output_index\":0,\"delta\":\"{\\\"path\\\":\"}\n\n\
event: response.function_call_arguments.delta\n\
data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"output_index\":0,\"delta\":\"\\\"README.md\\\"}\"}\n\n\
event: response.output_item.done\n\
data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"file_read\",\"arguments\":\"{\\\"path\\\":\\\"README.md\\\"}\",\"status\":\"completed\"}}\n\n\
event: response.completed\n\
data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"model\":\"example/Small-Model\",\"status\":\"completed\",\"output\":[{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"file_read\",\"arguments\":\"{\\\"path\\\":\\\"README.md\\\"}\",\"status\":\"completed\"}],\"usage\":{\"input_tokens\":20,\"output_tokens\":9}}}\n\n";

/// The same answer, with every announcement and the terminal object naming `name` instead.
fn stream_calling(name: &str) -> String {
    let renamed = STREAM.replace("\"name\":\"file_read\"", &format!("\"name\":\"{name}\""));
    assert_eq!(renamed.matches(name).count(), 3, "fixture rename");
    renamed
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

/// A resolver that counts its calls, optionally waits first, and then answers or fails.
struct Resolver {
    calls: Arc<AtomicUsize>,
    delay: Duration,
    failure: Option<SecretError>,
}

impl SecretResolver for Resolver {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            if let Some(failure) = self.failure {
                return Err(failure);
            }
            Ok(ResolvedSecret {
                secret: Secret::new(TOKEN.as_bytes().to_vec())?,
                version: SecretVersion::new("generation-1".to_owned())?,
            })
        })
    }
}

struct Fixture {
    client: ResponsesClient,
    resolutions: Arc<AtomicUsize>,
}

fn fixture(url: &str, limits: Limits, delay: Duration) -> Fixture {
    let resolutions = Arc::new(AtomicUsize::new(0));
    let client = ResponsesClient::new(
        binding(url),
        HttpClient::new(limits).expect("bounded transport"),
        Arc::new(Resolver {
            calls: resolutions.clone(),
            delay,
            failure: None,
        }),
    )
    .expect("a Responses binding");
    Fixture {
        client,
        resolutions,
    }
}

fn limits(total: Duration) -> Limits {
    Limits {
        response_headers: Duration::from_secs(10),
        idle: Duration::from_secs(10),
        total,
    }
}

fn client(url: &str) -> ResponsesClient {
    fixture(url, limits(Duration::from_secs(10)), Duration::ZERO).client
}

fn tool(name: &str) -> ToolSpec {
    ToolSpec {
        name: ToolName::new(name).expect("tool name"),
        description: "A fixture tool".to_owned(),
        input_schema: json!({"type": "object"}),
    }
}

/// A turn that forces `file_read`.
fn forced_request(text: &str) -> TurnRequest {
    let mut request = TurnRequest::new("small", vec![Item::user(text)]);
    request.tools = vec![tool("file_read")];
    request.tool_choice = ToolChoice::Named(ToolName::new("file_read").expect("tool name"));
    request.max_output_tokens = Some(256);
    request
}

fn expected_call() -> ToolCall {
    ToolCall {
        call_id: CallId::new("call_1").expect("call id"),
        name: ToolName::new("file_read").expect("tool name"),
        arguments: json!({"path": "README.md"}),
    }
}

async fn listener() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
    (listener, url)
}

async fn accept(listener: &TcpListener) -> TcpStream {
    let socket = tokio::time::timeout(Duration::from_secs(10), listener.accept())
        .await
        .expect("the client never opened a connection")
        .expect("an accepted connection")
        .0;
    socket.set_nodelay(true).expect("nodelay");
    socket
}

async fn read_request(socket: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let length = socket.read(&mut buffer).await.expect("a readable socket");
        assert!(length > 0, "the client closed before sending a request");
        bytes.extend_from_slice(&buffer[..length]);
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

async fn assert_no_connection(listener: &TcpListener) {
    assert!(
        tokio::time::timeout(Duration::from_millis(150), listener.accept())
            .await
            .is_err(),
        "the client opened a connection"
    );
}

/// Serves one connection: reads the request, writes `head` and `body`, then holds the socket open
/// for `hold` before closing it.
fn serve_once(
    listener: TcpListener,
    head: &'static [u8],
    body: Vec<u8>,
    hold: Duration,
) -> tokio::task::JoinHandle<String> {
    tokio::spawn(async move {
        let mut socket = accept(&listener).await;
        let captured = read_request(&mut socket).await;
        socket.write_all(head).await.expect("head written");
        socket.write_all(&body).await.expect("body written");
        socket.flush().await.expect("flushed");
        tokio::time::sleep(hold).await;
        let _ = socket.shutdown().await;
        captured
    })
}

// ---------------------------------------------------------------------------------------------
// Error custody: a failure the transport raises before sending must be a valid unsent failure.
// ---------------------------------------------------------------------------------------------

/// A neutral request inside `MAX_REQUEST_BYTES` whose projected wire body is outside it.
///
/// `TurnRequest::validate` bounds the neutral encoding; `project_request` adds the fixed
/// Responses fields (`stream`, `store`, `include`, the message envelope) and checks no bound of
/// its own, so the transport is the first to refuse the body.
fn request_whose_body_exceeds_the_bound(provenance: &llm_core::Provenance) -> TurnRequest {
    let empty = TurnRequest::new("small", vec![Item::user("")]);
    let overhead = encoded_len(&empty, usize::MAX).expect("encodable");
    let request = TurnRequest::new(
        "small",
        vec![Item::user("a".repeat(MAX_REQUEST_BYTES - overhead))],
    );
    request
        .validate()
        .expect("the neutral request is inside its bound");
    let projection = llm_responses::Binding::new(provenance.clone(), id("example/Small-Model"));
    let body = serde_json::to_vec(&project_request(&projection, &request).expect("projects"))
        .expect("encodes");
    assert!(
        body.len() > MAX_REQUEST_BYTES,
        "precondition: the projected body ({}) exceeds the transport bound ({MAX_REQUEST_BYTES})",
        body.len()
    );
    request
}

/// `Error::validate_for` is what `llm-routing`'s fallback runs on every failed attempt
/// (`crates/llm-routing/src/fallback.rs`, `settle`): an error that fails it is recorded as
/// `AmbiguousDispatch` and halts fallback. A refusal raised before anything was sent must pass it.
#[tokio::test]
async fn an_oversized_projected_body_is_refused_as_a_valid_unsent_failure() {
    let (listener, url) = listener().await;
    let client = client(&url);
    let request = request_whose_body_exceeds_the_bound(client.provenance());
    let mut sink = VecSink::new(16, 4096);
    let error = client
        .turn(&request, &mut sink, &Cancel::new())
        .await
        .expect_err("a body over the transport bound is not sent");
    assert_no_connection(&listener).await;
    assert_eq!(error.code, ErrorCode::TooLarge, "{error}");
    assert_eq!(error.dispatch, Dispatch::NotSent, "{error}");
    assert_eq!(
        error.validate_for(client.provenance()),
        Ok(()),
        "the client returned an unsent failure that its own core contract refuses: \
         dispatch {:?} with an observation attached",
        error.dispatch
    );
}

/// `client.rs` says the projection is "Refused before any I/O, and before a credential is
/// resolved". A body the transport is certain to refuse should not cost a secret-store lookup;
/// `MessagesClient` bounds its encoded body before resolving.
#[tokio::test]
async fn an_oversized_projected_body_is_refused_before_the_credential_is_resolved() {
    let (listener, url) = listener().await;
    let Fixture {
        client,
        resolutions,
    } = fixture(&url, limits(Duration::from_secs(10)), Duration::ZERO);
    let request = request_whose_body_exceeds_the_bound(client.provenance());
    let mut sink = VecSink::new(16, 4096);
    let error = client
        .turn(&request, &mut sink, &Cancel::new())
        .await
        .expect_err("a body over the transport bound is not sent");
    assert_no_connection(&listener).await;
    assert_eq!(error.code, ErrorCode::TooLarge, "{error}");
    assert_eq!(
        resolutions.load(Ordering::SeqCst),
        0,
        "the credential was resolved for a body that was never going to be sent"
    );
}

// ---------------------------------------------------------------------------------------------
// The neutral Model contract its two sibling clients enforce.
// ---------------------------------------------------------------------------------------------

/// `MessagesClient` (`encode_request`) and the Chat client (`outgoing.rs`) both run
/// `TurnRequest::validate_for(provenance, capabilities)` before any I/O. A request asking for
/// more output than the binding declares must be refused unsent, not forwarded.
#[tokio::test]
async fn a_request_beyond_the_binding_capabilities_is_refused_before_sending() {
    let (listener, url) = listener().await;
    let client = client(&url);
    let mut request = TurnRequest::new("small", vec![Item::user("hello")]);
    request.max_output_tokens = Some(100_000);
    assert!(
        request
            .validate_for(client.provenance(), client.capabilities())
            .is_err(),
        "precondition: the core contract refuses this request for this binding"
    );
    let server = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(2), listener.accept())
            .await
            .is_ok()
    });
    let mut sink = VecSink::new(16, 4096);
    let result = client.turn(&request, &mut sink, &Cancel::new()).await;
    let connected = server.await.expect("the probe");
    assert!(
        !connected,
        "a request beyond the binding's declared capabilities was sent"
    );
    let error = result.expect_err("a request beyond the binding's capabilities");
    assert_eq!(error.code, ErrorCode::Unsupported, "{error}");
    assert_eq!(error.dispatch, Dispatch::NotSent, "{error}");
}

/// `MessagesClient` and the Chat client run `TurnOutcome::validate_for(request, target)` on every
/// outcome. A model that answers a turn forcing `file_read` with a call to another published
/// tool must be refused, not handed to the caller as the forced call's answer.
#[tokio::test]
async fn a_forced_tool_answered_with_another_tool_is_refused() {
    let (listener, url) = listener().await;
    let server = serve_once(
        listener,
        SSE_HEAD,
        stream_calling("shell_exec").into_bytes(),
        Duration::ZERO,
    );
    let client = client(&url);
    let mut request = forced_request("Read the readme");
    request.tools.push(tool("shell_exec"));
    let mut sink = VecSink::new(16, 4096);
    let result = client.turn(&request, &mut sink, &Cancel::new()).await;
    server.await.expect("the fixture server");
    let error = match result {
        Ok(outcome) => panic!(
            "the client returned a call to {:?} for a turn that forced file_read",
            outcome
                .tool_calls()
                .map(|call| call.name.as_str().to_owned())
                .collect::<Vec<_>>()
        ),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::Protocol, "{error}");
    assert_eq!(error.dispatch, Dispatch::Accepted, "{error}");
}

// ---------------------------------------------------------------------------------------------
// Reading stops at the terminal object.
// ---------------------------------------------------------------------------------------------

/// The server sends a complete answer and keeps the connection open. `client.rs` stops reading
/// at the first final event "so a server that keeps the connection open after its terminal object
/// does not hold the turn until the idle limit". No test in `tests/client.rs` holds a connection
/// open, so the `break` that makes this true is not covered there.
#[tokio::test]
async fn a_completed_response_on_a_connection_left_open_returns_without_waiting() {
    let (listener, url) = listener().await;
    let server = serve_once(
        listener,
        SSE_HEAD,
        STREAM.as_bytes().to_vec(),
        Duration::from_secs(6),
    );
    let client = fixture(
        &url,
        Limits {
            response_headers: Duration::from_secs(5),
            idle: Duration::from_secs(5),
            total: Duration::from_secs(5),
        },
        Duration::ZERO,
    )
    .client;
    let mut sink = VecSink::new(16, 4096);
    let started = Instant::now();
    let outcome = client
        .turn(
            &forced_request("Read the readme"),
            &mut sink,
            &Cancel::new(),
        )
        .await
        .expect("a completed turn, not a deadline");
    let elapsed = started.elapsed();
    server.abort();
    assert!(
        elapsed < Duration::from_secs(2),
        "the turn waited {elapsed:?} on a connection whose answer had completed"
    );
    assert_eq!(
        outcome.tool_calls().cloned().collect::<Vec<_>>(),
        [expected_call()]
    );
}

// ---------------------------------------------------------------------------------------------
// SSE framing through the client.
// ---------------------------------------------------------------------------------------------

/// CRLF line endings, comments and keep-alives between events, and the bytes delivered in
/// seven-byte writes so that events, lines and CR/LF pairs straddle chunk boundaries.
#[tokio::test]
async fn crlf_comments_and_arbitrary_chunking_decode_the_same_turn() {
    let (listener, url) = listener().await;
    let mut body = Vec::new();
    for event in STREAM.split("\n\n").filter(|event| !event.is_empty()) {
        body.extend_from_slice(b": keep-alive\r\n\r\n");
        body.extend_from_slice(event.replace('\n', "\r\n").as_bytes());
        body.extend_from_slice(b"\r\n\r\n");
    }
    let server = tokio::spawn(async move {
        let mut socket = accept(&listener).await;
        read_request(&mut socket).await;
        socket.write_all(SSE_HEAD).await.expect("head written");
        for chunk in body.chunks(7) {
            socket.write_all(chunk).await.expect("chunk written");
            socket.flush().await.expect("flushed");
            tokio::time::sleep(Duration::from_micros(200)).await;
        }
        let _ = socket.shutdown().await;
    });
    let client = client(&url);
    let mut sink = VecSink::new(16, 4096);
    let outcome = client
        .turn(
            &forced_request("Read the readme"),
            &mut sink,
            &Cancel::new(),
        )
        .await
        .expect("a decoded turn");
    server.await.expect("the fixture server");
    assert_eq!(outcome.stop_reason, StopReason::ToolCalls);
    assert_eq!(
        outcome.tool_calls().cloned().collect::<Vec<_>>(),
        [expected_call()]
    );
    assert_eq!(sink.events().len(), 3, "{:?}", sink.events());
}

/// An `error` event mid-stream: typed from its machine code, after dispatch, with the events
/// before it delivered, the provider's text not relayed, and the open connection not waited on.
#[tokio::test]
async fn an_error_event_mid_stream_is_a_typed_failure_after_the_events_before_it() {
    let (listener, url) = listener().await;
    let body = "event: response.created\n\
data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_2\",\"status\":\"in_progress\"}}\n\n\
event: response.output_text.delta\n\
data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"delta\":\"Hel\"}\n\n\
event: error\n\
data: {\"type\":\"error\",\"code\":\"rate_limit_exceeded\",\"message\":\"provider-prose fixture-token\",\"param\":null}\n\n";
    let server = serve_once(
        listener,
        SSE_HEAD,
        body.as_bytes().to_vec(),
        Duration::from_secs(6),
    );
    let client = client(&url);
    let mut sink = VecSink::new(16, 4096);
    let started = Instant::now();
    let error = client
        .turn(
            &TurnRequest::new("small", vec![Item::user("hi")]),
            &mut sink,
            &Cancel::new(),
        )
        .await
        .expect_err("an error event fails the turn");
    let elapsed = started.elapsed();
    server.abort();
    assert!(elapsed < Duration::from_secs(2), "waited {elapsed:?}");
    assert_eq!(error.code, ErrorCode::RateLimited, "{error}");
    assert_eq!(error.dispatch, Dispatch::Accepted, "{error}");
    assert!(
        !format!("{error:?}").contains("provider-prose"),
        "{error:?}"
    );
    assert!(!format!("{error:?}").contains(TOKEN), "{error:?}");
    assert_eq!(error.validate_for(client.provenance()), Ok(()));
    assert_eq!(
        sink.events(),
        [StreamEvent::TextDelta {
            text: "Hel".to_owned()
        }]
    );
}

#[tokio::test]
async fn an_incomplete_response_is_a_turn_that_says_why_it_stopped() {
    let (listener, url) = listener().await;
    let body = "event: response.output_text.delta\n\
data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"delta\":\"Hel\"}\n\n\
event: response.incomplete\n\
data: {\"type\":\"response.incomplete\",\"response\":{\"id\":\"resp_3\",\"model\":\"example/Small-Model\",\"status\":\"incomplete\",\"incomplete_details\":{\"reason\":\"max_output_tokens\"},\"output\":[{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Hel\"}]}],\"usage\":{\"input_tokens\":4,\"output_tokens\":1}}}\n\n";
    let server = serve_once(
        listener,
        SSE_HEAD,
        body.as_bytes().to_vec(),
        Duration::from_secs(6),
    );
    let client = client(&url);
    let mut sink = VecSink::new(16, 4096);
    let outcome = client
        .turn(
            &TurnRequest::new("small", vec![Item::user("hi")]),
            &mut sink,
            &Cancel::new(),
        )
        .await
        .expect("an incomplete response is still a turn");
    server.abort();
    assert_eq!(outcome.stop_reason, StopReason::MaxOutputTokens);
    assert_eq!(outcome.items, [Item::assistant("Hel")]);
    assert!(outcome.observation.final_usage);
}

#[tokio::test]
async fn a_failed_response_is_typed_and_keeps_its_counters() {
    let (listener, url) = listener().await;
    let body = "event: response.failed\n\
data: {\"type\":\"response.failed\",\"response\":{\"id\":\"resp_4\",\"model\":\"example/Small-Model\",\"status\":\"failed\",\"error\":{\"code\":\"server_error\",\"message\":\"provider-prose\"},\"usage\":{\"input_tokens\":4,\"output_tokens\":0}}}\n\n";
    let server = serve_once(
        listener,
        SSE_HEAD,
        body.as_bytes().to_vec(),
        Duration::from_secs(6),
    );
    let client = client(&url);
    let mut sink = VecSink::new(16, 4096);
    let error = client
        .turn(
            &TurnRequest::new("small", vec![Item::user("hi")]),
            &mut sink,
            &Cancel::new(),
        )
        .await
        .expect_err("a failed response");
    server.abort();
    assert_eq!(error.code, ErrorCode::Unavailable, "{error}");
    assert_eq!(error.dispatch, Dispatch::Accepted, "{error}");
    let observation = error.observation.as_deref().expect("evidence");
    assert!(observation.final_usage);
    assert_eq!(observation.response_id, Some(id("resp_4")));
    assert!(!format!("{error:?}").contains("provider-prose"));
}

// ---------------------------------------------------------------------------------------------
// HTTP status, error bodies and secret custody.
// ---------------------------------------------------------------------------------------------

/// Each status is typed, its body is neither read to the end nor relayed, and the bearer appears
/// in no diagnostic. The body here claims a hundred megabytes and never finishes.
#[tokio::test]
async fn error_statuses_are_typed_and_their_endless_bodies_are_not_read() {
    for (status, extra, code, dispatch, retry) in [
        (401, "", ErrorCode::Unauthorized, Dispatch::Rejected, None),
        (
            429,
            "Retry-After: 7\r\n",
            ErrorCode::RateLimited,
            Dispatch::Rejected,
            Some(7_000),
        ),
        (500, "", ErrorCode::Transport, Dispatch::Unknown, None),
    ] {
        let (listener, url) = listener().await;
        let head: &'static [u8] = Box::leak(
            format!(
                "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\n{extra}Content-Length: 100000000\r\n\r\n"
            )
            .into_bytes()
            .into_boxed_slice(),
        );
        let server = serve_once(
            listener,
            head,
            br#"{"error":{"message":"echo Bearer fixture-token"}}"#.to_vec(),
            Duration::from_secs(6),
        );
        let client = client(&url);
        let mut sink = VecSink::new(16, 4096);
        let started = Instant::now();
        let error = client
            .turn(&forced_request("hi"), &mut sink, &Cancel::new())
            .await
            .expect_err("an error status");
        let elapsed = started.elapsed();
        server.abort();
        assert!(
            elapsed < Duration::from_secs(2),
            "{status}: waited {elapsed:?}"
        );
        assert_eq!(error.code, code, "{status}: {error}");
        assert_eq!(error.dispatch, dispatch, "{status}: {error}");
        assert_eq!(error.retry_after_ms, retry, "{status}: {error}");
        let shown = format!("{error:?} {error}");
        assert!(!shown.contains(TOKEN), "{status}: {shown}");
        assert!(!shown.contains("echo"), "{status}: {shown}");
        assert_eq!(error.validate_for(client.provenance()), Ok(()), "{status}");
        assert!(sink.events().is_empty());
    }
}

// ---------------------------------------------------------------------------------------------
// Bounds.
// ---------------------------------------------------------------------------------------------

/// A stream of keep-alive comments and `keepalive` payloads that never terminates ends on the
/// transport's `total`, not on the idle limit, which every keep-alive resets.
#[tokio::test]
async fn a_stream_that_never_terminates_ends_on_the_total_limit() {
    let (listener, url) = listener().await;
    let server = tokio::spawn(async move {
        let mut socket = accept(&listener).await;
        read_request(&mut socket).await;
        socket.write_all(SSE_HEAD).await.expect("head written");
        for _ in 0..200 {
            if socket
                .write_all(b": keep-alive\n\ndata: {\"type\":\"keepalive\"}\n\n")
                .await
                .is_err()
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    });
    let client = fixture(&url, limits(Duration::from_millis(600)), Duration::ZERO).client;
    let mut sink = VecSink::new(16, 4096);
    let started = Instant::now();
    let error = client
        .turn(&forced_request("hi"), &mut sink, &Cancel::new())
        .await
        .expect_err("a stream that never ends is not a turn");
    let elapsed = started.elapsed();
    server.abort();
    assert_eq!(error.code, ErrorCode::Deadline, "{error}");
    assert_eq!(error.dispatch, Dispatch::Accepted, "{error}");
    assert!(elapsed < Duration::from_secs(3), "waited {elapsed:?}");
}

/// Credential resolution takes 400 ms of a 700 ms turn and the server answers 500 ms after the
/// request. A transport that started its own `total` after resolution would end at 1100 ms and
/// accept the answer at 900 ms; the documented turn bound ends the turn at 700 ms.
#[tokio::test]
async fn the_total_limit_counts_credential_resolution() {
    let (listener, url) = listener().await;
    let server = tokio::spawn(async move {
        let mut socket = accept(&listener).await;
        read_request(&mut socket).await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        let _ = socket.write_all(SSE_HEAD).await;
        let _ = socket.write_all(STREAM.as_bytes()).await;
        let _ = socket.shutdown().await;
    });
    let client = fixture(
        &url,
        limits(Duration::from_millis(700)),
        Duration::from_millis(400),
    )
    .client;
    let mut sink = VecSink::new(16, 4096);
    let error = client
        .turn(&forced_request("hi"), &mut sink, &Cancel::new())
        .await
        .expect_err("the turn outlived its total limit");
    server.abort();
    assert_eq!(error.code, ErrorCode::Deadline, "{error}");
}

#[tokio::test]
async fn a_resolver_that_never_answers_ends_on_the_total_limit_unsent() {
    let (listener, url) = listener().await;
    let client = fixture(
        &url,
        limits(Duration::from_millis(300)),
        Duration::from_secs(60),
    )
    .client;
    let mut sink = VecSink::new(16, 4096);
    let started = Instant::now();
    let error = client
        .turn(&forced_request("hi"), &mut sink, &Cancel::new())
        .await
        .expect_err("no credential within the bound");
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(error.code, ErrorCode::Deadline, "{error}");
    assert_eq!(error.dispatch, Dispatch::NotSent, "{error}");
    assert_eq!(error.validate_for(client.provenance()), Ok(()));
    assert_no_connection(&listener).await;
}

// ---------------------------------------------------------------------------------------------
// Concurrency.
// ---------------------------------------------------------------------------------------------

/// Two turns at once on one client: two requests, each carrying its own body and the bearer, and
/// two independent outcomes.
#[tokio::test]
async fn two_concurrent_turns_on_one_client_are_independent() {
    let (listener, url) = listener().await;
    let server = tokio::spawn(async move {
        let mut handlers = Vec::new();
        for _ in 0..2 {
            let mut socket = accept(&listener).await;
            handlers.push(tokio::spawn(async move {
                let captured = read_request(&mut socket).await;
                // Hold the first answer until the second request has had time to arrive.
                tokio::time::sleep(Duration::from_millis(100)).await;
                socket.write_all(SSE_HEAD).await.expect("head");
                socket.write_all(STREAM.as_bytes()).await.expect("stream");
                let _ = socket.shutdown().await;
                captured
            }));
        }
        let mut captured = Vec::new();
        for handler in handlers {
            captured.push(handler.await.expect("a handler"));
        }
        assert_no_connection(&listener).await;
        captured
    });
    let client = client(&url);
    let (mut first_sink, mut second_sink) = (VecSink::new(16, 4096), VecSink::new(16, 4096));
    let (first_request, second_request) = (forced_request("first"), forced_request("second"));
    let (cancel_a, cancel_b) = (Cancel::new(), Cancel::new());
    let (first, second) = tokio::join!(
        client.turn(&first_request, &mut first_sink, &cancel_a),
        client.turn(&second_request, &mut second_sink, &cancel_b),
    );
    let captured = server.await.expect("the fixture server");
    for outcome in [first.expect("first turn"), second.expect("second turn")] {
        assert_eq!(
            outcome.tool_calls().cloned().collect::<Vec<_>>(),
            [expected_call()]
        );
    }
    assert_eq!(first_sink.events().len(), 3);
    assert_eq!(second_sink.events().len(), 3);
    let bodies: Vec<Value> = captured
        .iter()
        .map(|request| {
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("authorization: bearer fixture-token"),
                "{request}"
            );
            serde_json::from_str(request.split_once("\r\n\r\n").expect("body").1).expect("JSON")
        })
        .collect();
    let mut texts: Vec<&str> = bodies
        .iter()
        .map(|body| {
            body["input"][0]["content"][0]["text"]
                .as_str()
                .expect("text")
        })
        .collect();
    texts.sort_unstable();
    assert_eq!(texts, ["first", "second"]);
}
