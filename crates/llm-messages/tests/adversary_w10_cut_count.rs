//! Adversary, wave 2026-10-20-w10, unit messages-cut, pass 1: what a Messages stream counts as
//! output before a connection cut (story:messages-cut-after-silent-output-is-final).
//!
//! Messages counts any payload but `message_start` and `ping`. The unit's suite opens with both
//! together; these cases take the boundaries apart. Local sockets only.
mod support;

use llm_core::{
    Cancel, Dispatch, Error, ErrorCode, Item, Model, StopReason, TurnOutcome, TurnRequest, VecSink,
};
use llm_http::{HttpClient, Limits};
use llm_messages::MessagesClient;
use std::{sync::Arc, time::Duration};
use support::{StaticResolver, binding_with, capabilities};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const SSE_HEAD: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";

const CHUNKED_HEAD: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n";

const PARTIAL: &str = "event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\"";

const PING: &str = "event: ping\ndata: {\"type\":\"ping\"}\n\n";

const MESSAGE_START: &str = "event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_014a\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"example-model-20260201\",\"content\":[],\"usage\":{\"input_tokens\":11,\"output_tokens\":1}}}\n\n";

/// The message's stop reason and final counters, with no block before it.
const MESSAGE_DELTA: &str = "event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":1}}\n\n";

const TEXT_BLOCK: &str = "event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\n";

const MESSAGE_STOP: &str = "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";

fn client(url: &str) -> MessagesClient {
    let bound = Duration::from_secs(20);
    MessagesClient::new(
        binding_with(url, capabilities()),
        HttpClient::new(Limits {
            response_headers: bound,
            idle: bound,
            total: Duration::from_secs(30),
        })
        .expect("bounded transport"),
        Arc::new(StaticResolver::new("fixture-key")),
    )
    .expect("a Messages binding")
}

async fn turn(head: &'static [u8], body: String) -> (Result<TurnOutcome, Error>, VecSink) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("an accepted connection");
        let mut seen = Vec::new();
        let mut buffer = vec![0; 1 << 16];
        while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
            let length = socket.read(&mut buffer).await.expect("a readable socket");
            assert!(length > 0, "the client closed before sending a request");
            seen.extend_from_slice(&buffer[..length]);
        }
        socket.write_all(head).await.expect("head written");
        socket
            .write_all(body.as_bytes())
            .await
            .expect("body written");
        let _ = socket.shutdown().await;
    });
    let mut request = TurnRequest::new("internal-model", vec![Item::user("Summarise the log")]);
    request.max_output_tokens = Some(512);
    let mut sink = VecSink::new(64, 64 * 1024);
    let result = client(&url).turn(&request, &mut sink, &Cancel::new()).await;
    server.await.expect("the fixture server");
    (result, sink)
}

#[derive(Clone, Copy, Debug)]
enum Cut {
    InsideAnEvent,
    InsideAChunk,
}

impl Cut {
    const ALL: [Self; 2] = [Self::InsideAnEvent, Self::InsideAChunk];

    const fn code(self) -> ErrorCode {
        match self {
            Self::InsideAnEvent => ErrorCode::Protocol,
            Self::InsideAChunk => ErrorCode::Transport,
        }
    }

    async fn failed(self, before: &str) -> (Error, VecSink) {
        let (result, sink) = match self {
            Self::InsideAnEvent => turn(SSE_HEAD, format!("{before}{PARTIAL}")).await,
            Self::InsideAChunk => {
                let whole = if before.is_empty() {
                    String::new()
                } else {
                    format!("{:x}\r\n{before}\r\n", before.len())
                };
                turn(CHUNKED_HEAD, format!("{whole}400\r\n{PARTIAL}")).await
            }
        };
        (result.expect_err("a cut turn is a refusal"), sink)
    }
}

/// A keep-alive alone, before the message header, answers nothing.
#[tokio::test]
async fn adversary_w10_a_cut_after_a_keep_alive_alone_stays_retriable() {
    for cut in Cut::ALL {
        let (error, sink) = cut.failed(PING).await;
        assert_eq!(
            (error.code, error.dispatch),
            (cut.code(), Dispatch::Accepted),
            "{cut:?}"
        );
        assert_eq!(sink.events(), [], "{cut:?}");
        assert!(
            error.may_retry(),
            "{cut:?}: nothing was answered: {error:?}"
        );
    }
}

/// A `message_delta` with no block before it shows nothing, but it is the model's stop reason
/// and counters: the attempt answered. The unit's suite never cuts after one, so a mutant that
/// adds `message_delta` to `LIFECYCLE_EVENTS` (`crates/llm-messages/src/decode.rs:217`) passes it.
#[tokio::test]
async fn adversary_w10_a_cut_after_a_message_delta_without_any_block_is_final() {
    for cut in Cut::ALL {
        let (error, sink) = cut.failed(&format!("{MESSAGE_START}{MESSAGE_DELTA}")).await;
        assert_eq!(
            (error.code, error.dispatch),
            (cut.code(), Dispatch::Accepted),
            "{cut:?}"
        );
        assert_eq!(sink.events(), [], "{cut:?}: nothing shown");
        assert!(
            !error.retriable && !error.may_retry(),
            "{cut:?}: the attempt answered: {error:?}"
        );
    }
}

/// Control for the Chat case of the same name: a connection that drops after `message_stop`,
/// before the chunked body's terminating chunk, still returns the complete turn, because the
/// client stops reading at the terminal event (`crates/llm-messages/src/client.rs:197`).
#[tokio::test]
async fn adversary_w10_a_drop_after_the_terminal_event_returns_the_complete_turn() {
    let complete = format!("{MESSAGE_START}{PING}{TEXT_BLOCK}{MESSAGE_DELTA}{MESSAGE_STOP}");
    let body = format!("{:x}\r\n{complete}\r\n", complete.len());
    let (result, sink) = turn(CHUNKED_HEAD, body).await;
    assert!(!sink.events().is_empty(), "the text was shown");
    let outcome = result.expect("a turn whose message_stop arrived is complete");
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
    assert_eq!(outcome.items, [Item::assistant("Hi")]);
}
