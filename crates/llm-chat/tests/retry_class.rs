//! The retry class of a Chat Completions turn cut mid-stream, as on Responses and Messages
//! (story:messages-cut-after-silent-output-is-final). The transport marks a cut retriable. A cut
//! before any output keeps that mark: the chunk that opens the stream, `role` with an empty or
//! null `content`, answers nothing. After any output the attempt has answered, whether the caller
//! was shown it or not, so a cut is final: another attempt would answer, and bill, again. Output
//! no event shows is a tool call whose identifier arrived before its name, and a finish reason.
//! Local sockets only.
mod common;

use common::binding_at;
use llm_chat::ChatClient;
use llm_core::{
    Cancel, Capabilities, Dispatch, Error, ErrorCode, Item, Model, Protocol, TurnRequest, VecSink,
};
use llm_credentials::{ResolvedSecret, SecretError, SecretRef, SecretResolver};
use llm_http::{HttpClient, Limits};
use std::sync::Arc;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const SSE_HEAD: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";

/// A chunked stream head: a chunk the server stops writing is a body that failed in transit.
const CHUNKED_HEAD: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n";

/// The start of the next chunk, cut before its payload ends.
const PARTIAL: &str = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"lo";

/// One streamed chunk whose only choice carries `delta` and `finish_reason`.
fn chunk(delta: &str, finish_reason: &str) -> String {
    format!(
        "data: {{\"id\":\"chatcmpl-cut\",\"object\":\"chat.completion.chunk\",\"model\":\"Qwen/Qwen3-8B\",\"choices\":[{{\"index\":0,\"delta\":{delta},\"finish_reason\":{finish_reason}}}]}}\n\n"
    )
}

/// The chunks that open a stream, as `fixtures/openai-text-and-usage.sse` and
/// `fixtures/vllm-tool-call-and-usage.sse` open: nothing answered yet.
fn openings() -> [(&'static str, String); 3] {
    [
        ("nothing", String::new()),
        (
            "a role with empty content",
            chunk(r#"{"role":"assistant","content":""}"#, "null"),
        ),
        (
            "a role with null content",
            chunk(r#"{"role":"assistant","content":null}"#, "null"),
        ),
    ]
}

/// Output after the opening chunk, and whether the caller is shown an event for it.
fn outputs() -> [(&'static str, String, bool); 5] {
    [
        (
            "a tool call identifier without its name",
            chunk(
                r#"{"tool_calls":[{"index":0,"id":"call_7Yq","type":"function"}]}"#,
                "null",
            ),
            false,
        ),
        ("a finish reason", chunk("{}", "\"stop\""), false),
        ("a text delta", chunk(r#"{"content":"Hel"}"#, "null"), true),
        (
            "a reasoning delta",
            chunk(r#"{"reasoning_content":"Checking"}"#, "null"),
            true,
        ),
        (
            "a tool call announcement",
            chunk(
                r#"{"tool_calls":[{"index":0,"id":"call_7Yq","type":"function","function":{"name":"lookup","arguments":""}}]}"#,
                "null",
            ),
            true,
        ),
    ]
}

/// An anonymous binding never reaches a resolver; this one proves it by refusing.
struct NoResolver;
impl SecretResolver for NoResolver {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> llm_core::BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async { Err(SecretError::Missing) })
    }
}

/// Answers one request with `head` and `body`, closes, and returns the turn's refusal and what
/// the caller was shown.
async fn failed_turn(head: &'static [u8], body: String) -> (Error, VecSink) {
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
    let client = ChatClient::new(
        binding_at(
            Protocol::ChatCompletions,
            Capabilities::text(32_768, 4_096),
            &url,
        ),
        HttpClient::new(Limits::default()).expect("bounded transport"),
        Arc::new(NoResolver),
    );
    let mut sink = VecSink::new(64, 64 * 1024);
    let error = client
        .turn(
            &TurnRequest::new("small", vec![Item::user("Hallo")]),
            &mut sink,
            &Cancel::new(),
        )
        .await
        .expect_err("a cut turn is a refusal");
    server.await.expect("the fixture server");
    (error, sink)
}

/// The two ways a connection is cut mid-turn, each read by the transport as a retriable failure
/// with dispatch `accepted`.
#[derive(Clone, Copy, Debug)]
enum Cut {
    /// The body ends inside an event: `protocol`.
    InsideAnEvent,
    /// A chunked body ends inside a chunk: `transport`.
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

    /// Serves `before` whole, then cuts the connection inside the next event.
    async fn turn(self, before: &str) -> (Error, VecSink) {
        match self {
            Self::InsideAnEvent => failed_turn(SSE_HEAD, format!("{before}{PARTIAL}")).await,
            Self::InsideAChunk => {
                // A zero-length chunk would end the body cleanly, so an empty prefix sends none.
                let whole = if before.is_empty() {
                    String::new()
                } else {
                    format!("{:x}\r\n{before}\r\n", before.len())
                };
                // The last chunk promises 1024 bytes and carries a few.
                failed_turn(CHUNKED_HEAD, format!("{whole}400\r\n{PARTIAL}")).await
            }
        }
    }
}

/// H8 on this wire: a cut after nothing but the opening chunk keeps the transport's retry class.
#[tokio::test]
async fn a_cut_before_any_output_stays_retriable() {
    for cut in Cut::ALL {
        for (label, opening) in openings() {
            let (error, sink) = cut.turn(&opening).await;
            assert_eq!(
                (error.code, error.dispatch),
                (cut.code(), Dispatch::Accepted),
                "{cut:?} after {label}"
            );
            assert!(
                error.may_retry(),
                "{cut:?} after {label}: nothing was answered yet: {error:?}"
            );
            assert_eq!(sink.events(), [], "{cut:?} after {label}");
        }
    }
}

/// H8 and H18 on this wire: a cut after any output, shown or not, is final. The failure keeps
/// the attempt's evidence.
#[tokio::test]
async fn a_cut_after_silent_or_visible_output_is_final() {
    for cut in Cut::ALL {
        for (opening_label, opening) in openings() {
            for (label, output, visible) in outputs() {
                let (error, sink) = cut.turn(&format!("{opening}{output}")).await;
                let case = format!("{cut:?} after {opening_label} and {label}");
                assert_eq!(
                    (error.code, error.dispatch),
                    (cut.code(), Dispatch::Accepted),
                    "{case}"
                );
                assert_eq!(!sink.events().is_empty(), visible, "{case}");
                assert!(
                    !error.retriable && !error.may_retry(),
                    "{case}: an answered attempt cannot replay: {error:?}"
                );
                assert!(error.observation.is_some(), "{case}");
            }
        }
    }
}
