//! Adversarial pass 2: the absolute turn deadline against the HTTP exchange.
//!
//! Three documents say the same thing. The story's implementation contract: "Caller cancellation
//! and absolute turn deadline cover secret resolution, HTTP and blocked sinks."
//! `MessagesClient::with_turn_limit` (src/client.rs:61): "Bound the whole turn — credential
//! resolution, **the HTTP exchange** and a sink that is not accepting — by one absolute instant
//! taken when the turn starts." `StreamDecoder::deadline` (src/decode.rs:202): "A blocked sink is
//! bounded by it exactly as the HTTP exchange is." `docs/messages.md:117`: "The absolute deadline
//! covers all three *when the client is given one*."
//!
//! The turn's instant is passed to exactly two waits: `bounded(...)` around credential resolution
//! and `StreamDecoder::with_deadline` around the sink. Neither `post_sse` nor `SseStream::next` is
//! ever told it. Those two waits are bounded by the transport's own clock, which starts when
//! `post_sse` is entered and runs for the `Limits` the `HttpClient` was built with — a different
//! instant and, whenever the caller states a shorter turn, a different length.
//!
//! Both cases below state a 300 ms turn limit on a transport built with 10 s limits, which is the
//! configuration the accessor added in `crates/llm-http/src/transport.rs` exists to make possible.
//! The turn must end at 300 ms. It ends at the transport's 10 s instead, so a 3 s outer bound
//! catches it still running.
mod support;

use llm_core::{Cancel, ErrorCode, Item, Model, TurnRequest, VecSink};
use llm_http::{HttpClient, Limits};
use llm_messages::MessagesClient;
use std::{sync::Arc, time::Duration};
use support::{StaticResolver, binding_with, capabilities};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const TRANSPORT_LIMIT: Duration = Duration::from_secs(10);
const TURN_LIMIT: Duration = Duration::from_millis(300);
/// Ten times the turn limit and a third of the transport's: only one of the two can be meant.
const OUTER_BOUND: Duration = Duration::from_secs(3);

const MESSAGE_START: &[u8] = b"event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_014a\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"example-model-20260201\",\"content\":[],\"usage\":{\"input_tokens\":11,\"cache_read_input_tokens\":4,\"cache_creation_input_tokens\":6,\"output_tokens\":1}}}\n\n";

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
                return String::from_utf8_lossy(&bytes).into_owned();
            }
        }
    }
}

async fn listener() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    (listener, url)
}

/// A client whose stated turn bound is genuinely shorter than the transport's own, which is the
/// case the accessor and the derivation in `MessagesClient::new` were added to support.
fn short_turn_client(url: &str) -> MessagesClient {
    MessagesClient::new(
        binding_with(url, capabilities()),
        HttpClient::new(Limits {
            response_headers: TRANSPORT_LIMIT,
            idle: TRANSPORT_LIMIT,
            total: TRANSPORT_LIMIT,
        })
        .unwrap(),
        Arc::new(StaticResolver::new("fixture-key")),
    )
    .expect("a Messages binding")
    .with_turn_limit(TURN_LIMIT)
    .expect("a positive turn limit")
}

fn request() -> TurnRequest {
    let mut request = TurnRequest::new("internal-model", vec![Item::user("Summarise the log")]);
    request.max_output_tokens = Some(512);
    request
}

/// The exchange is live and then stops arriving. The caller accepts everything, so nothing here
/// is a blocked sink: the only wait left is `SseStream::next`, which the turn's instant never
/// reaches.
#[tokio::test]
async fn the_turn_deadline_bounds_a_stream_that_stops_arriving() {
    let (listener, url) = listener().await;
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        socket.write_all(MESSAGE_START).await.unwrap();
        // The route accepted the turn and then went quiet. It never closes the connection.
        tokio::time::sleep(Duration::from_secs(60)).await;
    });
    let client = short_turn_client(&url);
    let mut sink = VecSink::new(16, 4096);
    let request = request();
    let result = tokio::time::timeout(
        OUTER_BOUND,
        client.turn(&request, &mut sink, &Cancel::new()),
    )
    .await;
    server.abort();
    let error = result
        .expect(
            "a 300 ms turn limit did not end a stream that stopped arriving; the turn was still \
             running after 3 s, bounded by the transport's own 10 s clock rather than by the \
             absolute instant the turn started on",
        )
        .expect_err("a turn whose stream stopped arriving");
    assert_eq!(error.code, ErrorCode::Deadline);
}

/// The request was sent and the response headers never come. `post_sse` waits on the transport's
/// `response_headers` limit; the turn's own instant is not among the things that can end it.
#[tokio::test]
async fn the_turn_deadline_bounds_a_response_that_never_arrives() {
    let (listener, url) = listener().await;
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        // The request was accepted and no status line is ever written.
        tokio::time::sleep(Duration::from_secs(60)).await;
    });
    let client = short_turn_client(&url);
    let mut sink = VecSink::new(16, 4096);
    let request = request();
    let result = tokio::time::timeout(
        OUTER_BOUND,
        client.turn(&request, &mut sink, &Cancel::new()),
    )
    .await;
    server.abort();
    let error = result
        .expect(
            "a 300 ms turn limit did not end a request whose response headers never arrived; the \
             turn was still running after 3 s, bounded by the transport's own 10 s clock rather \
             than by the absolute instant the turn started on",
        )
        .expect_err("a turn whose response never arrived");
    assert_eq!(error.code, ErrorCode::Deadline);
}
