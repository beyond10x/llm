//! The single-attempt Messages client over local sockets. No provider is contacted.
mod support;

use llm_core::{
    BoxFuture, Cancel, Dispatch, Error, ErrorCode, Item, Model, StreamEvent, StreamSink, ToolName,
    ToolSpec, TurnRequest, VecSink,
};
use llm_credentials::{SecretError, SecretResolver};
use llm_http::{HttpClient, Limits};
use llm_messages::{ANTHROPIC_VERSION, MessagesClient, VERSION_HEADER};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use support::{StaticResolver, binding_with, capabilities};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
};

const STREAM: &[u8] = b"event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_014a\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"example-model-20260201\",\"content\":[],\"usage\":{\"input_tokens\":11,\"cache_read_input_tokens\":4,\"cache_creation_input_tokens\":6,\"output_tokens\":1}}}\n\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"input_tokens\":11,\"cache_read_input_tokens\":4,\"cache_creation_input_tokens\":6,\"output_tokens\":8}}\n\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\n";

async fn read_request(socket: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        let length = socket.read(&mut buffer).await.unwrap();
        assert!(length > 0, "the client closed before sending a request");
        bytes.extend_from_slice(&buffer[..length]);
        assert!(bytes.len() < 256 * 1024);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let header = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let length: usize = header
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .expect("a bounded body")
                .trim()
                .parse()
                .unwrap();
            if bytes.len() >= end + 4 + length {
                return String::from_utf8(bytes).expect("UTF-8 request");
            }
        }
    }
}

fn limits(total: Duration) -> Limits {
    Limits {
        response_headers: total,
        idle: total,
        total,
    }
}

fn client(url: &str, resolver: StaticResolver) -> MessagesClient {
    bounded_client(url, resolver, Duration::from_secs(10), None)
}

/// The client a caller builds when it states the turn bound as well as the transport bound.
fn bounded_client(
    url: &str,
    resolver: StaticResolver,
    total: Duration,
    turn: Option<Duration>,
) -> MessagesClient {
    let client = MessagesClient::new(
        binding_with(url, capabilities()),
        HttpClient::new(limits(total)).unwrap(),
        Arc::new(resolver),
    )
    .expect("a Messages binding");
    match turn {
        None => client,
        Some(turn) => client.with_turn_limit(turn).expect("a positive turn limit"),
    }
}

fn request() -> TurnRequest {
    let mut request = TurnRequest::new("internal-model", vec![Item::user("Summarise the log")]);
    request.max_output_tokens = Some(512);
    request
}

/// Bounded so that a client which never connects fails the test instead of hanging it.
async fn accept(listener: &TcpListener) -> (TcpStream, std::net::SocketAddr) {
    tokio::time::timeout(Duration::from_secs(10), listener.accept())
        .await
        .expect("the client never opened a connection")
        .unwrap()
}

async fn listener() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    (listener, url)
}

#[tokio::test]
async fn a_turn_presents_its_credential_at_request_time_and_streams_the_declared_subset() {
    let (listener, url) = listener().await;
    let server = tokio::spawn(async move {
        let (mut socket, _) = accept(&listener).await;
        let captured = read_request(&mut socket).await;
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        socket.write_all(STREAM).await.unwrap();
        socket.shutdown().await.unwrap();
        captured
    });
    let client = client(&url, StaticResolver::new("fixture-key"));
    let mut sink = VecSink::new(16, 4096);
    let outcome = client
        .turn(&request(), &mut sink, &Cancel::new())
        .await
        .expect("a decoded turn");
    let captured = server.await.unwrap();

    let head = captured.to_ascii_lowercase();
    assert!(head.starts_with("post /v1/messages "), "{captured}");
    assert!(
        head.contains(&format!("x-api-key: {}", "fixture-key")),
        "{captured}"
    );
    assert!(
        head.contains(&format!(
            "{}: {}",
            VERSION_HEADER.to_ascii_lowercase(),
            ANTHROPIC_VERSION
        )),
        "{captured}"
    );
    assert!(head.contains("accept: text/event-stream"), "{captured}");
    assert!(
        head.contains("content-type: application/json"),
        "{captured}"
    );
    assert!(!head.contains("authorization:"), "{captured}");

    let body: Value =
        serde_json::from_str(captured.split("\r\n\r\n").nth(1).expect("a body")).unwrap();
    assert_eq!(
        body,
        json!({"model":"example/Model-Revision","max_tokens":512,"stream":true,
            "messages":[{"role":"user","content":[{"type":"text","text":"Summarise the log",
                "cache_control":{"type":"ephemeral"}}]}]})
    );
    assert_eq!(
        sink.events(),
        [StreamEvent::TextDelta {
            text: "Hello".to_owned()
        }]
    );
    assert_eq!(
        outcome.observation.usage.expect("usage").input_tokens,
        Some(21)
    );
    assert!(outcome.observation.final_usage);
}

#[tokio::test]
async fn a_truncated_stream_keeps_its_partial_usage_and_is_not_sent_again() {
    let (listener, url) = listener().await;
    let server = tokio::spawn(async move {
        let (mut socket, _) = accept(&listener).await;
        read_request(&mut socket).await;
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        let end = STREAM
            .windows(13)
            .position(|w| w == b"message_delta")
            .expect("a terminal half to cut off");
        socket.write_all(&STREAM[..end - 7]).await.unwrap();
        socket.shutdown().await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(150), listener.accept())
                .await
                .is_err(),
            "the client attempted the request a second time"
        );
    });
    let client = client(&url, StaticResolver::new("fixture-key"));
    let mut sink = VecSink::new(16, 4096);
    let error = client
        .turn(&request(), &mut sink, &Cancel::new())
        .await
        .expect_err("EOF never manufactures success");
    server.await.unwrap();
    assert_eq!(error.code, ErrorCode::Protocol);
    assert_eq!(error.dispatch, Dispatch::Accepted);
    let observation = error.observation.expect("the last snapshot is retained");
    assert!(!observation.final_usage);
    assert_eq!(
        observation.usage.expect("partial usage").output_tokens,
        Some(1)
    );
    assert_eq!(
        sink.events(),
        [StreamEvent::TextDelta {
            text: "Hello".to_owned()
        }]
    );
}

#[tokio::test]
async fn a_refused_status_carries_no_upstream_body_text() {
    let (listener, url) = listener().await;
    let server = tokio::spawn(async move {
        let (mut socket, _) = accept(&listener).await;
        read_request(&mut socket).await;
        let body = b"{\"error\":{\"type\":\"authentication_error\",\"message\":\"key sk-secret-1234 is revoked\"}}";
        socket
            .write_all(
                format!(
                    "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        socket.write_all(body).await.unwrap();
        socket.shutdown().await.unwrap();
    });
    let client = client(&url, StaticResolver::new("fixture-key"));
    let mut sink = VecSink::new(16, 4096);
    let error = client
        .turn(&request(), &mut sink, &Cancel::new())
        .await
        .expect_err("refused");
    server.await.unwrap();
    assert_eq!(error.code, ErrorCode::Unauthorized);
    assert_eq!(error.dispatch, Dispatch::Rejected);
    assert!(!error.message.contains("sk-secret"), "{}", error.message);
    assert_eq!(sink.events(), []);
}

#[tokio::test]
async fn an_unresolvable_credential_never_reaches_the_network() {
    let (listener, url) = listener().await;
    let client = client(&url, StaticResolver::failing(SecretError::Missing));
    let mut sink = VecSink::new(16, 4096);
    let error = client
        .turn(&request(), &mut sink, &Cancel::new())
        .await
        .expect_err("refused");
    assert_eq!(error.code, ErrorCode::Unauthorized);
    assert_eq!(error.dispatch, Dispatch::NotSent);
    assert!(error.observation.is_none());
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err(),
        "an unresolved credential still opened a connection"
    );
}

/// A fixture credential that counts how often the client asked for it.
struct CountingResolver {
    inner: StaticResolver,
    calls: Arc<AtomicUsize>,
}
impl SecretResolver for CountingResolver {
    fn resolve<'a>(
        &'a self,
        reference: &'a llm_credentials::SecretRef,
    ) -> BoxFuture<'a, Result<llm_credentials::ResolvedSecret, SecretError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.resolve(reference)
    }
}

/// Harness parity M19, M21 and M39 (`harness-messages/src/lib.rs:692`-`699`, test `:1249`).
///
/// The refusals are unit-tested against `encode_request`; this is the wiring. A client that
/// stopped projecting before it resolved a credential and connected would still pass every one of
/// those, and would be answered by the far side in its own field names instead of the caller's.
/// So each refusal goes through `MessagesClient::turn` against a listening server, and the case
/// asserts that no credential was resolved and no connection was opened. The last case is the
/// control: a request the route can carry does reach the same server through the same client,
/// which is what makes "nothing arrived" mean something.
#[tokio::test]
async fn every_pre_flight_refusal_is_reached_through_the_client_before_anything_is_sent() {
    let tool = |name: &str| ToolSpec {
        name: ToolName::new(name).expect("a neutral tool name"),
        description: "Look up a record".to_owned(),
        input_schema: json!({"type":"object"}),
    };
    let mut dotted = request();
    dotted.tools = vec![tool("workspace.read")];
    let mut long = request();
    long.tools = vec![tool(&"t".repeat(129))];
    let mut opens_with_the_model = request();
    opens_with_the_model.items = vec![Item::assistant("Looking"), Item::user("Go on")];
    let mut empty = request();
    empty.items.clear();
    let mut hot = request();
    hot.sampling.temperature = Some(1.5);
    let cases = [
        (
            "M19: a tool name outside this route's character class",
            dotted,
            "Messages tool name is outside the supported character or length bound",
        ),
        (
            "M19: a tool name one byte over this route's length cap",
            long,
            "Messages tool name is outside the supported character or length bound",
        ),
        (
            "M21: a conversation that opens with the model",
            opens_with_the_model,
            "Messages requires an initial user message",
        ),
        (
            "M21: a conversation with nothing in it",
            empty,
            "Messages requires an initial user message",
        ),
        (
            "M39: a temperature above this route's maximum",
            hot,
            "Messages temperature exceeds one",
        ),
    ];
    let (listener, url) = listener().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let client = MessagesClient::new(
        binding_with(&url, capabilities()),
        HttpClient::new(limits(Duration::from_secs(10))).unwrap(),
        Arc::new(CountingResolver {
            inner: StaticResolver::new("fixture-key"),
            calls: Arc::clone(&calls),
        }),
    )
    .expect("a Messages binding");
    for (name, request, message) in cases {
        let mut sink = VecSink::new(16, 4096);
        let error = client
            .turn(&request, &mut sink, &Cancel::new())
            .await
            .expect_err(name);
        assert_eq!(error.code, ErrorCode::Unsupported, "{name}: {error:?}");
        assert_eq!(error.message, message, "{name}");
        assert_eq!(error.dispatch, Dispatch::NotSent, "{name}");
        assert!(error.observation.is_none(), "{name}");
        assert!(sink.events().is_empty(), "{name}");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "{name} resolved a credential"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err(),
            "{name} opened a connection"
        );
    }

    let server = tokio::spawn(async move {
        let (mut socket, _) = accept(&listener).await;
        read_request(&mut socket).await;
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        socket.write_all(STREAM).await.unwrap();
        socket.shutdown().await.unwrap();
    });
    let mut sink = VecSink::new(16, 4096);
    client
        .turn(&request(), &mut sink, &Cancel::new())
        .await
        .expect("the control reaches the same server");
    server.await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1, "the control resolved once");
}

/// A sink that never accepts an event, and says when the first one was offered. From then on the
/// client has read the response head and the turn is waiting on its caller.
struct BlockedSink {
    offered: Option<oneshot::Sender<()>>,
}

impl BlockedSink {
    fn new() -> (Self, oneshot::Receiver<()>) {
        let (offered, receiver) = oneshot::channel();
        let offered = Some(offered);
        (Self { offered }, receiver)
    }
}

impl StreamSink for BlockedSink {
    fn emit(&mut self, _event: StreamEvent) -> BoxFuture<'_, Result<(), Error>> {
        if let Some(offered) = self.offered.take() {
            // Nobody may be waiting for it any more.
            let _ = offered.send(());
        }
        Box::pin(std::future::pending())
    }
}

/// The caller cancels on two signals and on no clock. The server says when it wrote the response
/// head, which alone proves only that the bytes left the server: a cancel before the client read
/// them ends the turn with dispatch `unknown`. The sink says when it was offered an event, which
/// is the client having read the head and waiting on its caller, the wait this case is about.
#[tokio::test]
async fn cancellation_reaches_a_caller_whose_sink_never_accepts() {
    let (listener, url) = listener().await;
    let (head_written, head) = oneshot::channel();
    let server = tokio::spawn(async move {
        // Not bounded here: a client that never connects ends the turn on its own limits, and
        // the case then fails on what the turn ended as.
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        // Nobody may be waiting for it any more.
        let _ = head_written.send(());
        socket.write_all(STREAM).await.unwrap();
        // Hold the response open; the turn must end because the caller cancelled.
        tokio::time::sleep(Duration::from_secs(30)).await;
    });
    let client = client(&url, StaticResolver::new("fixture-key"));
    let cancel = Cancel::new();
    let (mut sink, offered) = BlockedSink::new();
    let request = request();
    let turn = client.turn(&request, &mut sink, &cancel);
    let stop = async {
        head.await.expect("the server wrote the response head");
        offered.await.expect("the sink was offered an event");
        cancel.cancel();
    };
    tokio::pin!(turn, stop);
    let result = tokio::select! {
        biased;
        // The turn ended before it was cancelled: the assertions below say what it ended as.
        result = &mut turn => result,
        () = &mut stop => turn.await,
    };
    server.abort();
    let error = result.expect_err("the blocked sink was cancelled");
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_eq!(error.dispatch, Dispatch::Accepted);
    assert!(error.observation.is_some(), "the snapshot is retained");
}

/// A secret store that has stopped answering. Cancellation is not the only thing that must end
/// this turn: a caller that supplied no token still has the turn's own deadline.
struct PendingResolver;
impl llm_credentials::SecretResolver for PendingResolver {
    fn resolve<'a>(
        &'a self,
        _reference: &'a llm_credentials::SecretRef,
    ) -> BoxFuture<'a, Result<llm_credentials::ResolvedSecret, SecretError>> {
        Box::pin(std::future::pending())
    }
}

#[tokio::test]
async fn the_turn_deadline_bounds_credential_resolution() {
    let (listener, url) = listener().await;
    let client = MessagesClient::new(
        binding_with(&url, capabilities()),
        HttpClient::new(limits(Duration::from_millis(300))).unwrap(),
        Arc::new(PendingResolver),
    )
    .expect("a Messages binding")
    .with_turn_limit(Duration::from_millis(300))
    .expect("a positive turn limit");
    let mut sink = VecSink::new(16, 4096);
    let request = request();
    let error = tokio::time::timeout(
        Duration::from_secs(3),
        client.turn(&request, &mut sink, &Cancel::new()),
    )
    .await
    .expect("the turn deadline ended a resolution that never answered")
    .expect_err("a turn that never resolved a credential");
    assert_eq!(error.code, ErrorCode::Deadline);
    assert_eq!(error.dispatch, Dispatch::NotSent);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err(),
        "an unresolved credential still opened a connection"
    );
}

/// The transport's own bound in the deadline case below. Every turn limit the case states is
/// shorter, so the turn's deadline is what ends the turn.
const TRANSPORT_TOTAL: Duration = Duration::from_secs(10);

/// How long past its limit a turn may run before the case calls it unbounded. With the first
/// limit of 300 ms this is a bound of 3 s.
const OVERRUN: Duration = Duration::from_millis(2_700);

/// One turn with a turn limit of `limit` and a sink that never accepts, against a server that
/// sends the whole stream and then holds the response open. Returns how the turn ended, and
/// whether the sink was offered an event before it ended.
async fn turn_into_a_blocked_sink(limit: Duration) -> (Error, bool) {
    let (listener, url) = listener().await;
    let server = tokio::spawn(async move {
        let (mut socket, _) = accept(&listener).await;
        read_request(&mut socket).await;
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        socket.write_all(STREAM).await.unwrap();
        // Hold the response open: the turn must end on its own deadline, not on the server.
        tokio::time::sleep(Duration::from_secs(30)).await;
    });
    let client = bounded_client(
        &url,
        StaticResolver::new("fixture-key"),
        TRANSPORT_TOTAL,
        Some(limit),
    );
    let (mut sink, mut offered) = BlockedSink::new();
    let request = request();
    let started = Instant::now();
    let error = tokio::time::timeout(
        limit + OVERRUN,
        client.turn(&request, &mut sink, &Cancel::new()),
    )
    .await
    .expect("the turn deadline ended a wait on a sink that never accepted")
    .expect_err("a turn whose caller never accepted a delta");
    let lasted = started.elapsed();
    server.abort();
    assert!(
        lasted >= limit,
        "the turn ended after {lasted:?}, before its limit of {limit:?}"
    );
    (error, offered.try_recv().is_ok())
}

/// The deadline has to pass while the sink holds an event, and the client takes it from the
/// clock when the turn starts, so no signal can place it. A turn whose deadline passed before the
/// sink was offered an event bounded another wait (the one for the response head ends with
/// dispatch `unknown`) and decides nothing here: the case asks again with twice the limit. The
/// three assertions are made on the turn whose sink was offered an event.
#[tokio::test]
async fn the_turn_deadline_bounds_a_sink_that_never_accepts() {
    let mut limit = Duration::from_millis(300);
    let error = loop {
        let (error, offered) = turn_into_a_blocked_sink(limit).await;
        if offered {
            break error;
        }
        assert_eq!(
            error.code,
            ErrorCode::Deadline,
            "a turn that never reached the sink still ends on its deadline"
        );
        limit *= 2;
        assert!(
            limit < TRANSPORT_TOTAL,
            "the sink was offered no event before any turn deadline below the transport's \
             {TRANSPORT_TOTAL:?}"
        );
    };
    assert_eq!(error.code, ErrorCode::Deadline);
    assert_eq!(error.dispatch, Dispatch::Accepted);
    assert!(
        error.observation.is_some(),
        "the last snapshot is retained across a deadline"
    );
}

/// A request carrying hints this wire omits (a cache key and client metadata) and the one it
/// carries (the prompt cache).
fn hinted_request() -> TurnRequest {
    let mut request = request();
    let hints = &mut request.controls.hints;
    hints.prompt_cache = Some(llm_core::PromptCache { ttl: None });
    hints.prompt_cache_key = Some("conversation-1".to_owned());
    hints
        .unforwarded
        .insert(llm_core::Unforwarded::ClientMetadata);
    request
}

/// Serves `STREAM`, or the first `cut` bytes of it, to one connection.
async fn serve_once(listener: TcpListener, cut: Option<usize>) {
    let (mut socket, _) = accept(&listener).await;
    read_request(&mut socket).await;
    socket
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
    socket
        .write_all(&STREAM[..cut.unwrap_or(STREAM.len())])
        .await
        .unwrap();
    socket.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_succeeded_turn_ends_with_one_warning_naming_the_hints_this_wire_omitted() {
    let (listener, url) = listener().await;
    let server = tokio::spawn(serve_once(listener, None));
    let client = client(&url, StaticResolver::new("fixture-key"));
    let mut sink = VecSink::new(16, 4096);
    client
        .turn(&hinted_request(), &mut sink, &Cancel::new())
        .await
        .expect("a decoded turn");
    server.await.unwrap();
    assert_eq!(
        sink.events(),
        [
            StreamEvent::TextDelta {
                text: "Hello".to_owned()
            },
            StreamEvent::Warning {
                code: llm_core::ADVISORY_OMITTED.to_owned(),
                message: "Messages carries no prompt-cache-key, client-metadata; the advisory \
                          hint was not sent"
                    .to_owned(),
            },
        ]
    );
}

#[tokio::test]
async fn a_turn_whose_hints_this_wire_all_carries_emits_no_warning() {
    let (listener, url) = listener().await;
    let server = tokio::spawn(serve_once(listener, None));
    let client = client(&url, StaticResolver::new("fixture-key"));
    let mut sink = VecSink::new(16, 4096);
    let mut request = request();
    request.controls.hints.prompt_cache = Some(llm_core::PromptCache {
        ttl: Some(llm_core::CacheTtl::OneHour),
    });
    client
        .turn(&request, &mut sink, &Cancel::new())
        .await
        .expect("a decoded turn");
    server.await.unwrap();
    assert_eq!(
        sink.events(),
        [StreamEvent::TextDelta {
            text: "Hello".to_owned()
        }]
    );
}

#[tokio::test]
async fn a_failed_turn_emits_no_advisory_warning() {
    let (listener, url) = listener().await;
    let end = STREAM
        .windows(13)
        .position(|w| w == b"message_delta")
        .expect("a terminal half to cut off");
    let server = tokio::spawn(serve_once(listener, Some(end - 7)));
    let client = client(&url, StaticResolver::new("fixture-key"));
    let mut sink = VecSink::new(16, 4096);
    client
        .turn(&hinted_request(), &mut sink, &Cancel::new())
        .await
        .expect_err("a truncated stream");
    server.await.unwrap();
    assert_eq!(
        sink.events(),
        [StreamEvent::TextDelta {
            text: "Hello".to_owned()
        }]
    );
}

#[tokio::test]
async fn a_sink_that_refuses_the_warning_fails_the_turn_with_its_own_error() {
    let (listener, url) = listener().await;
    let server = tokio::spawn(serve_once(listener, None));
    let client = client(&url, StaticResolver::new("fixture-key"));
    // Room for the one text delta and nothing more.
    let mut sink = VecSink::new(1, 4096);
    let error = client
        .turn(&hinted_request(), &mut sink, &Cancel::new())
        .await
        .expect_err("the sink refused the warning");
    server.await.unwrap();
    assert_eq!(error.code, ErrorCode::TooLarge);
    assert_eq!(error.message, "stream sink event bound reached");
}
