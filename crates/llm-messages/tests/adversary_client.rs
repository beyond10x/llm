//! Adversarial client cases over local sockets. No provider is contacted and nothing is edited.
//!
//! Both cases drive the Implementation contract line "Caller cancellation and absolute turn
//! deadline cover secret resolution, HTTP and blocked sinks", restated in `docs/messages.md`
//! lines 81-82 as "Cancellation and the absolute deadline cover secret resolution, the HTTP
//! exchange and a sink that is not accepting."
//!
//! The shipped suite tests the *cancellation* half of both (`tests/client.rs`
//! `an_unresolvable_credential_never_reaches_the_network` and
//! `cancellation_reaches_a_caller_whose_sink_never_accepts`). Neither half of the *deadline*
//! claim is tested, and the deadline is the only bound that exists when the caller supplies no
//! cancellation token.
mod support;

use llm_core::{
    BoxFuture, Cancel, Error, ErrorCode, Item, Model, StreamEvent, StreamSink, TurnRequest, VecSink,
};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_http::{HttpClient, Limits};
use llm_messages::MessagesClient;
use std::{sync::Arc, time::Duration};
use support::{binding_with, capabilities};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const STREAM: &[u8] = b"event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_014a\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"example-model-20260201\",\"content\":[],\"usage\":{\"input_tokens\":11,\"cache_read_input_tokens\":4,\"cache_creation_input_tokens\":6,\"output_tokens\":1}}}\n\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n";

/// A shorter absolute deadline than any case here waits for.
fn limits() -> Limits {
    Limits {
        response_headers: Duration::from_millis(300),
        idle: Duration::from_millis(300),
        total: Duration::from_millis(300),
    }
}

fn request() -> TurnRequest {
    let mut request = TurnRequest::new("internal-model", vec![Item::user("Summarise the log")]);
    request.max_output_tokens = Some(512);
    request
}

fn client(url: &str, resolver: Arc<dyn SecretResolver>) -> MessagesClient {
    MessagesClient::new(
        binding_with(url, capabilities()),
        HttpClient::new(limits()).expect("bounded HTTP limits"),
        resolver,
    )
    .expect("a Messages binding")
}

/// A secret store that has stopped answering: the shape `llm-providers` already models in its own
/// suite as `PendingResolver` (`crates/llm-providers/tests/bindings.rs:382`), and the shape a
/// remote keychain or `CoordinatedResolver` takes under a partition.
struct PendingResolver;
impl SecretResolver for PendingResolver {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(std::future::pending())
    }
}

struct Fixture;
impl SecretResolver for Fixture {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move {
            Ok(ResolvedSecret {
                secret: Secret::new(b"fixture-key".to_vec())?,
                version: SecretVersion::new("generation-1".to_owned())?,
            })
        })
    }
}

struct BlockedSink;
impl StreamSink for BlockedSink {
    fn emit(&mut self, _event: StreamEvent) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(std::future::pending())
    }
}

/// `MessagesClient::run` awaits `Binding::prepare_auth` (`src/client.rs:62`) before it enters the
/// HTTP client, and `prepare_auth` selects on the cancellation token only
/// (`crates/llm-providers/src/auth.rs:50`). The absolute turn deadline lives inside `HttpClient`,
/// which has not been entered yet, so a secret store that never answers holds the turn open for
/// as long as the caller is willing to wait.
#[tokio::test]
async fn the_absolute_turn_deadline_bounds_credential_resolution() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let client = client(&url, Arc::new(PendingResolver));
    let mut sink = VecSink::new(16, 4096);
    let request = request();
    let outcome = tokio::time::timeout(
        Duration::from_secs(3),
        client.turn(&request, &mut sink, &Cancel::new()),
    )
    .await;
    let result = outcome.expect(
        "the 300ms absolute turn deadline never fired: credential resolution is bounded by \
         cancellation only, and this caller cancelled nothing",
    );
    assert_eq!(
        result
            .expect_err("a turn that never resolved a credential")
            .code,
        ErrorCode::Deadline,
    );
}

/// The deadline is checked inside `SseStream::next` (`crates/llm-http/src/transport.rs:169`).
/// `StreamDecoder::apply` awaits `sink.emit` between two `next` calls
/// (`crates/llm-messages/src/decode.rs:562`) under a `select!` whose only other branch is the
/// cancellation token, so a sink that never accepts keeps control out of `next` forever and the
/// deadline is never reached.
#[tokio::test]
async fn the_absolute_turn_deadline_bounds_a_sink_that_never_accepts() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        drain(&mut socket).await;
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        socket.write_all(STREAM).await.unwrap();
        // Hold the response open. The turn must end on its own deadline.
        tokio::time::sleep(Duration::from_secs(30)).await;
    });
    let client = client(&url, Arc::new(Fixture));
    let mut sink = BlockedSink;
    let request = request();
    let outcome = tokio::time::timeout(
        Duration::from_secs(3),
        client.turn(&request, &mut sink, &Cancel::new()),
    )
    .await;
    server.abort();
    let result = outcome.expect(
        "the 300ms absolute turn deadline never fired: a sink that never accepts is bounded by \
         cancellation only, and this caller cancelled nothing",
    );
    assert_eq!(
        result
            .expect_err("a turn whose caller never accepted a delta")
            .code,
        ErrorCode::Deadline,
    );
}

async fn drain(socket: &mut TcpStream) {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        let length = socket.read(&mut buffer).await.unwrap();
        assert!(length > 0, "the client closed before sending a request");
        bytes.extend_from_slice(&buffer[..length]);
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
                return;
            }
        }
    }
}
