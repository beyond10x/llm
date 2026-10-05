//! Harness parity M32 and M46: a live connection that keeps sending after `message_stop`
//! (`harness-messages/src/lib.rs:439`, `:802`-`806`, test `:1079`), and a cancel while the server
//! is still sending to a sink that accepts every event (`harness-messages/tests/
//! provider_emulated.rs:565`). Local sockets only; no provider is contacted.
mod support;

use llm_core::{
    BoxFuture, Cancel, Dispatch, Error, ErrorCode, Item, Model, StreamEvent, StreamSink,
    TurnRequest, VecSink,
};
use llm_http::{HttpClient, Limits};
use llm_messages::MessagesClient;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use support::{StaticResolver, binding_with, capabilities};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
};

const HEAD: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";

const OPENING: &[u8] = b"event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_014a\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"example-model-20260201\",\"content\":[],\"usage\":{\"input_tokens\":11,\"output_tokens\":1}}}\n\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n";

const DELTA: &[u8] = b"event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n";

const CLOSING: &[u8] = b"event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":8}}\n\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\n";

/// A payload after the terminal event. Read, it would be refused as a stream this subset does not
/// describe; delivered, the caller would see text from outside the turn.
const AFTER_STOP: &[u8] = b"event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"never delivered\"}}\n\n";

/// How long the server keeps writing before it concludes the client never let go.
const PATIENCE: Duration = Duration::from_secs(3);

async fn read_request(socket: &mut TcpStream) {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        let length = socket.read(&mut buffer).await.unwrap();
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
                .unwrap();
            if bytes.len() >= end + 4 + length {
                return;
            }
        }
    }
}

/// Writes `chunk` every few milliseconds until a write fails, which is the client having closed
/// the connection. True when that happened within [`PATIENCE`].
async fn keep_sending(socket: &mut TcpStream, chunk: &[u8]) -> bool {
    let started = Instant::now();
    while started.elapsed() < PATIENCE {
        if socket.write_all(chunk).await.is_err() || socket.flush().await.is_err() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    false
}

/// The server half: one accepted connection, the given opening, then `chunk` until the client
/// lets go. Reports whether it let go, and whether a second connection followed.
fn serve(
    listener: TcpListener,
    opening: Vec<u8>,
    chunk: &'static [u8],
) -> tokio::task::JoinHandle<(bool, bool)> {
    tokio::spawn(async move {
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(10), listener.accept())
            .await
            .expect("the client never opened a connection")
            .unwrap();
        read_request(&mut socket).await;
        socket.write_all(HEAD).await.unwrap();
        socket.write_all(&opening).await.unwrap();
        let released = keep_sending(&mut socket, chunk).await;
        let resent = tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_ok();
        (released, resent)
    })
}

async fn listener() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    (listener, url)
}

fn client(url: &str) -> MessagesClient {
    let bound = Duration::from_secs(10);
    MessagesClient::new(
        binding_with(url, capabilities()),
        HttpClient::new(Limits {
            response_headers: bound,
            idle: bound,
            total: bound,
        })
        .unwrap(),
        Arc::new(StaticResolver::new("fixture-key")),
    )
    .expect("a Messages binding")
}

fn request() -> TurnRequest {
    let mut request = TurnRequest::new("internal-model", vec![Item::user("Summarise the log")]);
    request.max_output_tokens = Some(512);
    request
}

/// M32. The server sends a whole turn, then a payload after `message_stop`, then keeps the
/// connection open and keeps writing. The turn ends at the terminal event: it neither waits for
/// the server to close nor reads what follows, nothing after the terminal event reaches the
/// caller, and the connection is released rather than left to the server.
#[tokio::test]
async fn a_connection_that_keeps_sending_after_message_stop_ends_the_turn_at_the_stop() {
    let (listener, url) = listener().await;
    let opening = [OPENING, DELTA, CLOSING, AFTER_STOP].concat();
    let server = serve(listener, opening, b": still here\n\n");
    let client = client(&url);
    let mut sink = VecSink::new(16, 4096);
    let outcome = tokio::time::timeout(
        Duration::from_secs(2),
        client.turn(&request(), &mut sink, &Cancel::new()),
    )
    .await
    .expect("the turn waited for a server that never closes")
    .expect("bytes after the terminal event are not part of this turn");
    assert_eq!(
        sink.events(),
        [StreamEvent::TextDelta {
            text: "Hello".to_owned()
        }]
    );
    assert!(outcome.observation.final_usage);
    let (released, resent) = server.await.unwrap();
    assert!(released, "the connection was held after the turn ended");
    assert!(!resent, "the turn was sent twice");
}

/// A sink that accepts every event at once, and says when the third text delta arrived.
struct AcceptingSink {
    cancel: Cancel,
    deltas: usize,
    after_cancel: usize,
    third: Option<oneshot::Sender<()>>,
}

impl StreamSink for AcceptingSink {
    fn emit(&mut self, event: StreamEvent) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            if self.cancel.is_cancelled() {
                self.after_cancel += 1;
            }
            if matches!(event, StreamEvent::TextDelta { .. }) {
                self.deltas += 1;
                if self.deltas == 3
                    && let Some(third) = self.third.take()
                {
                    let _ = third.send(());
                }
            }
            Ok(())
        })
    }
}

/// M46. The server is still sending text deltas, the sink takes each one at once, and the caller
/// cancels part-way through. The turn ends as cancelled with dispatch `accepted`, nothing reaches
/// the sink after the cancel, the connection is closed while the server is still writing, and
/// the request is not sent again.
#[tokio::test]
async fn a_cancel_while_the_server_still_sends_to_an_accepting_sink_stops_the_turn() {
    let (listener, url) = listener().await;
    let server = serve(listener, OPENING.to_vec(), DELTA);
    let client = client(&url);
    let cancel = Cancel::new();
    let (third, third_rx) = oneshot::channel();
    let mut sink = AcceptingSink {
        cancel: cancel.clone(),
        deltas: 0,
        after_cancel: 0,
        third: Some(third),
    };
    let request = request();
    let canceller = async {
        third_rx.await.expect("three deltas arrived");
        cancel.cancel();
        Instant::now()
    };
    let (cancelled_at, result) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(canceller, client.turn(&request, &mut sink, &cancel))
    })
    .await
    .expect("the cancel did not end a turn whose server was still sending");
    let error = result.expect_err("a cancelled turn has no outcome");
    assert!(
        cancelled_at.elapsed() < Duration::from_secs(1),
        "the turn outlived its cancel by {:?}",
        cancelled_at.elapsed()
    );
    assert_eq!(error.code, ErrorCode::Cancelled, "{error}");
    assert_eq!(error.dispatch, Dispatch::Accepted, "{error}");
    assert!(sink.deltas >= 3, "{}", sink.deltas);
    assert_eq!(
        sink.after_cancel, 0,
        "events reached the sink after the cancel"
    );
    let (released, resent) = server.await.unwrap();
    assert!(released, "the connection stayed open after the cancel");
    assert!(!resent, "the cancelled turn was sent again");
}
