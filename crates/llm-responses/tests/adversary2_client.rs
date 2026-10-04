//! Adversary pass 2 on `ResponsesClient`: the request and outcome validation the pass-1 fixes
//! added, the body bound placed before the credential, and the evidence every refusal carries.
//! Local sockets only; no provider is contacted.

use llm_core::{
    AuthKind, BillingKind, BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item,
    MAX_REQUEST_BYTES, Model, Protocol, Provenance, StreamEvent, StreamSink, ToolChoice, ToolName,
    ToolSpec, TurnRequest, Usage, VecSink,
};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_http::{HttpClient, Limits};
use llm_providers::{
    Account, BaseUrl, Binding, BindingDocument, Endpoint, Provider, ServedModel, ServingModel,
};
use llm_responses::{ResponsesClient, project_request};
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const TOKEN: &str = "fixture-token";

/// One Responses answer carrying a single `function_call` to `file_read`, billed 20 in / 9 out.
const CALL_STREAM: &str = "event: response.created\n\
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

/// One text answer, `Hello`, billed 20 in / 9 out.
const TEXT_STREAM: &str = "event: response.created\n\
data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"status\":\"in_progress\"}}\n\n\
event: response.output_text.delta\n\
data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"delta\":\"Hel\"}\n\n\
event: response.output_text.delta\n\
data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"delta\":\"lo\"}\n\n\
event: response.completed\n\
data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"model\":\"example/Small-Model\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Hello\"}]}],\"usage\":{\"input_tokens\":20,\"output_tokens\":9}}}\n\n";

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

struct Resolver {
    calls: Arc<AtomicUsize>,
    failure: Option<SecretError>,
}

impl SecretResolver for Resolver {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
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

fn client_with(url: &str, failure: Option<SecretError>) -> (ResponsesClient, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let client = ResponsesClient::new(
        binding(url),
        HttpClient::new(Limits {
            response_headers: Duration::from_secs(20),
            idle: Duration::from_secs(20),
            total: Duration::from_secs(30),
        })
        .expect("bounded transport"),
        Arc::new(Resolver {
            calls: calls.clone(),
            failure,
        }),
    )
    .expect("a Responses binding");
    (client, calls)
}

fn client(url: &str) -> ResponsesClient {
    client_with(url, None).0
}

fn tool(name: &str) -> ToolSpec {
    ToolSpec {
        name: ToolName::new(name).expect("tool name"),
        description: "A fixture tool".to_owned(),
        input_schema: json!({"type": "object"}),
    }
}

fn forced_request() -> TurnRequest {
    let mut request = TurnRequest::new("small", vec![Item::user("Read the readme")]);
    request.tools = vec![tool("file_read")];
    request.tool_choice = ToolChoice::Named(ToolName::new("file_read").expect("tool name"));
    request
}

fn billed() -> Usage {
    Usage {
        input_tokens: Some(20),
        output_tokens: Some(9),
        ..Usage::default()
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
    let socket = tokio::time::timeout(Duration::from_secs(20), listener.accept())
        .await
        .expect("the client never opened a connection")
        .expect("an accepted connection")
        .0;
    socket.set_nodelay(true).expect("nodelay");
    socket
}

/// Reads one request and returns its declared body length and the number of body bytes read.
async fn read_request(socket: &mut TcpStream) -> (usize, usize) {
    let mut bytes = Vec::new();
    let mut buffer = vec![0; 1 << 16];
    let mut head_end = None;
    loop {
        let length = socket.read(&mut buffer).await.expect("a readable socket");
        assert!(length > 0, "the client closed before sending a request");
        bytes.extend_from_slice(&buffer[..length]);
        if head_end.is_none() {
            head_end = bytes.windows(4).position(|w| w == b"\r\n\r\n");
        }
        if let Some(end) = head_end {
            let head = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let declared: usize = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .expect("a bounded body")
                .trim()
                .parse()
                .expect("a numeric content-length");
            if bytes.len() >= end + 4 + declared {
                return (declared, bytes.len() - end - 4);
            }
        }
    }
}

fn serve_once(
    listener: TcpListener,
    head: &'static [u8],
    body: Vec<u8>,
) -> tokio::task::JoinHandle<(usize, usize)> {
    tokio::spawn(async move {
        let mut socket = accept(&listener).await;
        let captured = read_request(&mut socket).await;
        socket.write_all(head).await.expect("head written");
        socket.write_all(&body).await.expect("body written");
        socket.flush().await.expect("flushed");
        let _ = socket.shutdown().await;
        captured
    })
}

async fn assert_no_connection(listener: &TcpListener) {
    assert!(
        tokio::time::timeout(Duration::from_millis(150), listener.accept())
            .await
            .is_err(),
        "the client opened a connection"
    );
}

/// What routing's fallback (`crates/llm-routing/src/fallback.rs`, `settle`) requires of every
/// failed attempt before it believes the dispatch claim.
fn assert_valid_failure(error: &Error, target: &Provenance, dispatch: Dispatch, case: &str) {
    assert_eq!(
        error.validate_for(target),
        Ok(()),
        "{case}: Error::validate_for refuses {error:?}"
    );
    assert_eq!(error.dispatch, dispatch, "{case}: {error:?}");
}

// ---------------------------------------------------------------------------------------------
// Billed evidence on a failure after the stream completed.
// ---------------------------------------------------------------------------------------------

/// The decoder reads the whole stream before a single event reaches the sink, so when the sink
/// refuses, the provider has already reported the turn's final counters. `MessagesClient` and the
/// Chat client attach their last valid usage snapshot to a sink failure; routing's fallback
/// records `error.observation` as the attempt's evidence (`fallback.rs`, `failed`). A sink that
/// refuses after its bound — `llm_core::VecSink` does exactly that — must not erase the 20 in /
/// 9 out the attempt was billed for.
#[tokio::test]
async fn a_sink_refusal_after_the_stream_completed_keeps_the_billed_counters() {
    let (listener, url) = listener().await;
    let server = serve_once(listener, SSE_HEAD, CALL_STREAM.as_bytes().to_vec());
    let client = client(&url);
    // One event accepted, the second refused: the fixture streams three.
    let mut sink = VecSink::new(1, 4096);
    let error = client
        .turn(&forced_request(), &mut sink, &Cancel::new())
        .await
        .expect_err("the sink refused its second event");
    server.await.expect("the fixture server");
    assert_eq!(error.code, ErrorCode::TooLarge, "{error:?}");
    assert_valid_failure(
        &error,
        client.provenance(),
        Dispatch::Accepted,
        "sink refusal",
    );
    let observation = error.observation.as_deref().expect("an observation");
    assert_eq!(
        observation.usage,
        Some(billed()),
        "a sink refusal after a completed, billed stream dropped the counters the provider \
         reported: {observation:?}"
    );
}

/// The same, with the caller cancelling while events are being handed over.
#[tokio::test]
async fn a_cancel_while_events_are_handed_over_keeps_the_billed_counters() {
    struct CancellingSink {
        cancel: Cancel,
        seen: usize,
    }
    impl StreamSink for CancellingSink {
        fn emit(&mut self, _event: StreamEvent) -> BoxFuture<'_, Result<(), Error>> {
            Box::pin(async move {
                self.seen += 1;
                self.cancel.cancel();
                Ok(())
            })
        }
    }
    let (listener, url) = listener().await;
    let server = serve_once(listener, SSE_HEAD, CALL_STREAM.as_bytes().to_vec());
    let client = client(&url);
    let cancel = Cancel::new();
    let mut sink = CancellingSink {
        cancel: cancel.clone(),
        seen: 0,
    };
    let error = client
        .turn(&forced_request(), &mut sink, &cancel)
        .await
        .expect_err("the caller cancelled during hand-over");
    server.await.expect("the fixture server");
    assert_eq!(sink.seen, 1, "the cancel landed after the first event");
    assert_eq!(error.code, ErrorCode::Cancelled, "{error:?}");
    assert_valid_failure(&error, client.provenance(), Dispatch::Accepted, "cancel");
    let observation = error.observation.as_deref().expect("an observation");
    assert_eq!(
        observation.usage,
        Some(billed()),
        "a cancel after a completed, billed stream dropped the counters the provider reported: \
         {observation:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// Outcome validation: the refusal carries what was billed.
// ---------------------------------------------------------------------------------------------

/// The pass-1 case `a_forced_tool_answered_with_another_tool_is_refused` asserts code and
/// dispatch only. The refusal is raised after the provider reported final counters; it must carry
/// them, as the outcome's own observation, not the bare binding.
#[tokio::test]
async fn an_outcome_refusal_carries_the_terminal_counters_it_was_billed_for() {
    let (listener, url) = listener().await;
    let stream = CALL_STREAM.replace("\"name\":\"file_read\"", "\"name\":\"shell_exec\"");
    let server = serve_once(listener, SSE_HEAD, stream.into_bytes());
    let client = client(&url);
    let mut request = forced_request();
    request.tools.push(tool("shell_exec"));
    let mut sink = VecSink::new(16, 4096);
    let error = client
        .turn(&request, &mut sink, &Cancel::new())
        .await
        .expect_err("a forced tool answered with another tool");
    server.await.expect("the fixture server");
    assert_eq!(error.code, ErrorCode::Protocol, "{error:?}");
    assert_valid_failure(&error, client.provenance(), Dispatch::Accepted, "outcome");
    let observation = error.observation.as_deref().expect("an observation");
    assert_eq!(observation.usage, Some(billed()), "{observation:?}");
    assert!(observation.final_usage, "{observation:?}");
    assert_eq!(observation.response_id, Some(id("resp_1")));
    assert_eq!(observation.upstream_model, Some(id("example/Small-Model")));
}

/// `ToolChoice::Required` answered with text and a normal end: refused after dispatch, with the
/// counters, and the text already relayed is not taken back.
#[tokio::test]
async fn a_required_tool_answered_with_text_is_refused_with_its_counters() {
    let (listener, url) = listener().await;
    let server = serve_once(listener, SSE_HEAD, TEXT_STREAM.as_bytes().to_vec());
    let client = client(&url);
    let mut request = forced_request();
    request.tool_choice = ToolChoice::Required;
    let mut sink = VecSink::new(16, 4096);
    let error = client
        .turn(&request, &mut sink, &Cancel::new())
        .await
        .expect_err("a required tool answered with text");
    server.await.expect("the fixture server");
    assert_eq!(error.code, ErrorCode::Protocol, "{error:?}");
    assert_valid_failure(&error, client.provenance(), Dispatch::Accepted, "required");
    let observation = error.observation.as_deref().expect("an observation");
    assert_eq!(observation.usage, Some(billed()), "{observation:?}");
    assert_eq!(sink.text(), "Hello");
}

/// A completed response whose counters contradict themselves (reasoning above output). The
/// decoder refuses it with no observation at all, and the client returns that as is, while every
/// other post-dispatch failure it returns carries at least the binding (`attach`), and the
/// decoder's own `bound` says a refusal without one "says nothing about which serving model it
/// came from".
#[tokio::test]
async fn a_contradictory_usage_report_is_refused_with_the_binding_attached() {
    let (listener, url) = listener().await;
    let stream = TEXT_STREAM.replace(
        "\"usage\":{\"input_tokens\":20,\"output_tokens\":9}",
        "\"usage\":{\"input_tokens\":20,\"output_tokens\":9,\"output_tokens_details\":{\"reasoning_tokens\":50}}",
    );
    assert_ne!(stream, TEXT_STREAM, "fixture rewrite");
    let server = serve_once(listener, SSE_HEAD, stream.into_bytes());
    let client = client(&url);
    let mut sink = VecSink::new(16, 4096);
    let error = client
        .turn(
            &TurnRequest::new("small", vec![Item::user("hi")]),
            &mut sink,
            &Cancel::new(),
        )
        .await
        .expect_err("contradictory counters");
    server.await.expect("the fixture server");
    assert_eq!(error.code, ErrorCode::Protocol, "{error:?}");
    assert_valid_failure(&error, client.provenance(), Dispatch::Accepted, "usage");
    let observation = error
        .observation
        .as_deref()
        .expect("a refusal after dispatch names the binding that served it");
    assert_eq!(&observation.binding, client.provenance());
    assert_eq!(observation.usage, None, "invalid counters are not carried");
}

// ---------------------------------------------------------------------------------------------
// Every refusal through Error::validate_for, with its dispatch.
// ---------------------------------------------------------------------------------------------

/// Refusals raised before anything is sent: `not-sent`, no observation, no connection.
#[tokio::test]
async fn every_unsent_refusal_is_a_valid_not_sent_failure() {
    let (listener, url) = listener().await;
    let (client, _) = client_with(&url, None);
    let target = client.provenance().clone();
    let mut sink = VecSink::new(16, 4096);

    let wrong_model = TurnRequest::new("other", vec![Item::user("hi")]);
    let mut temperature = TurnRequest::new("small", vec![Item::user("hi")]);
    temperature.sampling.temperature = Some(3.0);
    let mut effort = TurnRequest::new("small", vec![Item::user("hi")]);
    effort.sampling.reasoning_effort = Some("extreme".to_owned());
    let mut unpublished = TurnRequest::new("small", vec![Item::user("hi")]);
    unpublished.tool_choice = ToolChoice::Named(ToolName::new("nope").expect("name"));
    for (case, request, code) in [
        ("model", wrong_model, ErrorCode::InvalidRequest),
        ("temperature", temperature, ErrorCode::InvalidRequest),
        ("effort", effort, ErrorCode::Unsupported),
        ("unpublished", unpublished, ErrorCode::InvalidRequest),
    ] {
        let error = client
            .turn(&request, &mut sink, &Cancel::new())
            .await
            .expect_err(case);
        assert_eq!(error.code, code, "{case}: {error:?}");
        assert!(error.observation.is_none(), "{case}: {error:?}");
        assert_valid_failure(&error, &target, Dispatch::NotSent, case);
    }

    let cancelled = Cancel::new();
    cancelled.cancel();
    let error = client
        .turn(
            &TurnRequest::new("small", vec![Item::user("hi")]),
            &mut sink,
            &cancelled,
        )
        .await
        .expect_err("cancelled before the turn");
    assert_eq!(error.code, ErrorCode::Cancelled, "{error:?}");
    assert_valid_failure(&error, &target, Dispatch::NotSent, "cancelled");

    for (failure, code) in [
        (SecretError::Missing, ErrorCode::Unauthorized),
        (SecretError::Unavailable, ErrorCode::Unavailable),
    ] {
        let (client, _) = client_with(&url, Some(failure));
        let error = client
            .turn(
                &TurnRequest::new("small", vec![Item::user("hi")]),
                &mut sink,
                &Cancel::new(),
            )
            .await
            .expect_err("a resolver failure");
        assert_eq!(error.code, code, "{failure:?}: {error:?}");
        assert_valid_failure(&error, &target, Dispatch::NotSent, "resolver");
    }
    assert_no_connection(&listener).await;
    assert!(sink.events().is_empty());
}

/// Refusals raised after the request left, one server behaviour each.
#[tokio::test]
async fn every_dispatched_refusal_is_valid_and_names_its_dispatch() {
    let cases: [(&str, &'static [u8], &str, ErrorCode, Dispatch); 5] = [
        (
            "401",
            b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "",
            ErrorCode::Unauthorized,
            Dispatch::Rejected,
        ),
        (
            "503",
            b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "",
            ErrorCode::Transport,
            Dispatch::Unknown,
        ),
        (
            "not SSE",
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n",
            "{}",
            ErrorCode::Protocol,
            Dispatch::Accepted,
        ),
        (
            "failed",
            SSE_HEAD,
            "event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"id\":\"resp_1\",\"status\":\"failed\",\"error\":{\"code\":\"server_error\"},\"usage\":{\"input_tokens\":20,\"output_tokens\":9}}}\n\n",
            ErrorCode::Unavailable,
            Dispatch::Accepted,
        ),
        (
            "clean close",
            SSE_HEAD,
            "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"status\":\"in_progress\"}}\n\n",
            ErrorCode::Protocol,
            Dispatch::Unknown,
        ),
    ];
    for (case, head, body, code, dispatch) in cases {
        let (listener, url) = listener().await;
        let server = serve_once(listener, head, body.as_bytes().to_vec());
        let client = client(&url);
        let mut sink = VecSink::new(16, 4096);
        let error = client
            .turn(
                &TurnRequest::new("small", vec![Item::user("hi")]),
                &mut sink,
                &Cancel::new(),
            )
            .await
            .expect_err(case);
        server.await.expect("the fixture server");
        assert_eq!(error.code, code, "{case}: {error:?}");
        assert_valid_failure(&error, client.provenance(), dispatch, case);
        let observation = error.observation.as_deref().expect(case);
        assert_eq!(&observation.binding, client.provenance(), "{case}");
    }
}

// ---------------------------------------------------------------------------------------------
// The body bound, at the boundary.
// ---------------------------------------------------------------------------------------------

/// A neutral request inside its own bound whose projected body is `MAX_REQUEST_BYTES + extra`.
fn request_with_projected_body(provenance: &Provenance, extra: usize) -> TurnRequest {
    let projection = llm_responses::Binding::new(provenance.clone(), id("example/Small-Model"));
    let length = |request: &TurnRequest| {
        serde_json::to_vec(&project_request(&projection, request).expect("projects"))
            .expect("encodes")
            .len()
    };
    let overhead = length(&TurnRequest::new("small", vec![Item::user("")]));
    let request = TurnRequest::new(
        "small",
        vec![Item::user("a".repeat(MAX_REQUEST_BYTES + extra - overhead))],
    );
    request
        .validate_for(provenance, &capabilities())
        .expect("the neutral request is inside its own bound");
    assert_eq!(length(&request), MAX_REQUEST_BYTES + extra, "precondition");
    request
}

/// Exactly `MAX_REQUEST_BYTES` is inside the bound: it is sent, whole, and answered.
#[tokio::test]
async fn a_projected_body_of_exactly_the_bound_is_sent() {
    let (listener, url) = listener().await;
    let server = serve_once(listener, SSE_HEAD, TEXT_STREAM.as_bytes().to_vec());
    let client = client(&url);
    let request = request_with_projected_body(client.provenance(), 0);
    let mut sink = VecSink::new(16, 4096);
    let outcome = client
        .turn(&request, &mut sink, &Cancel::new())
        .await
        .expect("a body of exactly the bound is sent");
    let (declared, received) = server.await.expect("the fixture server");
    assert_eq!(declared, MAX_REQUEST_BYTES);
    assert_eq!(received, MAX_REQUEST_BYTES);
    assert_eq!(outcome.items, [Item::assistant("Hello")]);
}

/// One byte over is refused before the credential and before any connection, as a valid
/// unsent failure.
#[tokio::test]
async fn a_projected_body_one_byte_over_the_bound_is_refused_unsent() {
    let (listener, url) = listener().await;
    let (client, resolutions) = client_with(&url, None);
    let request = request_with_projected_body(client.provenance(), 1);
    let mut sink = VecSink::new(16, 4096);
    let error = client
        .turn(&request, &mut sink, &Cancel::new())
        .await
        .expect_err("one byte over the bound");
    assert_no_connection(&listener).await;
    assert_eq!(resolutions.load(Ordering::SeqCst), 0);
    assert_eq!(error.code, ErrorCode::TooLarge, "{error:?}");
    assert!(error.observation.is_none(), "{error:?}");
    assert_valid_failure(&error, client.provenance(), Dispatch::NotSent, "over");
}
