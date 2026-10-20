//! Adversary, wave 2026-10-20-w10, unit messages-cut, pass 1: where a Chat Completions stream's
//! count of output begins, before a connection cut (story:messages-cut-after-silent-output-is-final).
//!
//! Two boundaries the unit's own suite leaves open. Chunks that carry nothing the model wrote, an
//! empty `tool_calls` array among them, keep a cut retriable. An opening `role` chunk whose
//! `content` is whitespace shows that whitespace, so a cut after it is final. Each case serves
//! whole chunks over a local socket, then cuts the body inside the next chunk. Local sockets only;
//! no provider is contacted.
mod common;

use common::binding_at;
use llm_chat::ChatClient;
use llm_core::{
    Cancel, Capabilities, Dispatch, Error, ErrorCode, Item, Model, Protocol, TurnOutcome,
    TurnRequest, VecSink,
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

const CHUNKED_HEAD: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n";

/// The start of the next chunk, cut before its payload ends.
const PARTIAL: &str = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"lo";

/// The opening chunk exactly as `fixtures/openai-text-and-usage.sse` pins it: `role`, empty
/// `content` and a null `refusal`.
const OPENAI_OPENING: &str = "data: {\"id\":\"chatcmpl-Bp1x2QfAkT\",\"object\":\"chat.completion.chunk\",\"created\":1772000000,\"model\":\"gpt-4o-mini-2024-07-18\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"\",\"refusal\":null},\"logprobs\":null,\"finish_reason\":null}]}\n\n";

fn chunk(delta: &str, finish_reason: &str) -> String {
    format!(
        "data: {{\"id\":\"chatcmpl-cut\",\"object\":\"chat.completion.chunk\",\"model\":\"Qwen/Qwen3-8B\",\"choices\":[{{\"index\":0,\"delta\":{delta},\"finish_reason\":{finish_reason}}}]}}\n\n"
    )
}

struct NoResolver;
impl SecretResolver for NoResolver {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> llm_core::BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async { Err(SecretError::Missing) })
    }
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
    let result = client
        .turn(
            &TurnRequest::new("small", vec![Item::user("Hallo")]),
            &mut sink,
            &Cancel::new(),
        )
        .await;
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

/// Chunks that carry nothing the caller is shown and nothing the model wrote: the brief's list
/// (an empty `tool_calls` array, empty `content` with a null finish reason, vLLM's null
/// `reasoning_content`, a usage-only chunk). A cut after them keeps the transport's class.
///
/// The empty `tool_calls` array pins `self.answered |= !calls.is_empty()`
/// (`crates/llm-chat/src/incoming.rs:156`): the unit's own openings never send one, so a mutant
/// that counts every `tool_calls` key passes the unit's suite.
#[tokio::test]
async fn adversary_w10_chunks_without_output_keep_a_cut_retriable() {
    let silent = [
        (
            "an empty tool_calls array",
            chunk(
                r#"{"role":"assistant","content":"","tool_calls":[]}"#,
                "null",
            ),
        ),
        ("empty content, null finish reason", chunk(r#"{"content":""}"#, "null")),
        (
            "vLLM's null reasoning_content",
            chunk(
                r#"{"role":"assistant","content":null,"reasoning_content":null}"#,
                "null",
            ),
        ),
        (
            "a usage-only chunk",
            "data: {\"id\":\"chatcmpl-cut\",\"object\":\"chat.completion.chunk\",\"model\":\"Qwen/Qwen3-8B\",\"choices\":[],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":0,\"total_tokens\":9}}\n\n".to_owned(),
        ),
    ];
    for cut in Cut::ALL {
        for (label, before) in &silent {
            let (error, sink) = cut.failed(&format!("{OPENAI_OPENING}{before}")).await;
            assert_eq!(
                (error.code, error.dispatch),
                (cut.code(), Dispatch::Accepted),
                "{cut:?} after {label}"
            );
            assert_eq!(sink.events(), [], "{cut:?} after {label}");
            assert!(
                error.may_retry(),
                "{cut:?} after {label}: nothing was answered, so the cut keeps its class: {error:?}"
            );
        }
    }
}

/// A `role` chunk whose `content` is whitespace shows that whitespace: visible output, final.
#[tokio::test]
async fn adversary_w10_an_opening_chunk_with_whitespace_content_is_output() {
    for cut in Cut::ALL {
        let (error, sink) = cut
            .failed(&chunk(r#"{"role":"assistant","content":" "}"#, "null"))
            .await;
        assert!(
            !sink.events().is_empty(),
            "{cut:?}: the whitespace is shown"
        );
        assert!(
            !error.may_retry(),
            "{cut:?}: shown output is final: {error:?}"
        );
    }
}
