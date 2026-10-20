//! Adversary, wave 2026-10-20-w11, unit 5 (story:a-llama-server-response-fixture), pass 1:
//! llama-server's `timings` against the `usage` report beside it.
//!
//! Two cases the unit's own suite leaves open. A refusal of `timings` that contradict `usage`
//! says neither report is believed (`crates/llm-chat/src/incoming.rs:527-528`, `docs/chat.md`),
//! yet the failure a caller receives can still carry one side's figure as its evidence. And
//! the cached pair is compared on its own only if a case moves it without moving the input it is
//! part of; the unit's case moves both. Local sockets and in-memory bodies only; no provider is
//! contacted.
mod common;

use common::{binding, binding_at};
use llm_chat::{ChatClient, decode_completion, project_response_bytes};
use llm_core::{
    Cancel, Capabilities, Dispatch, Error, ErrorCode, Item, Model, Protocol, TurnOutcome,
    TurnRequest, VecSink,
};
use llm_credentials::{ResolvedSecret, SecretError, SecretRef, SecretResolver};
use llm_http::{HttpClient, Limits};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const SSE_HEAD: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";

const CONTRADICTED: &str = "chat completion timings contradict its usage report";

struct NoResolver;
impl SecretResolver for NoResolver {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> llm_core::BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async { Err(SecretError::Missing) })
    }
}

/// One turn through the client a caller holds, against a loopback server replaying `body`.
async fn client_turn(body: String) -> Result<TurnOutcome, Error> {
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
        socket.write_all(SSE_HEAD).await.expect("head written");
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
    let model: &dyn Model = &client;
    let result = model
        .turn(
            &TurnRequest::new("small", vec![Item::user("Hallo")]),
            &mut sink,
            &Cancel::new(),
        )
        .await;
    server.await.expect("the fixture server");
    result
}

/// A llama-server-shaped stream: one text chunk, the finish chunk, then the usage chunk with
/// `usage` and `timings` beside it, as `server-task.cpp:494-510` at `7879491cf` orders them.
fn stream_with(usage: &Value, timings: &Value) -> String {
    let chunk = |extra: Value| {
        let mut chunk = json!({"id": "chatcmpl-adversary", "model": "qwen3-1.7b",
            "object": "chat.completion.chunk"});
        for (key, value) in extra.as_object().expect("an object") {
            chunk[key] = value.clone();
        }
        format!("data: {chunk}\n\n")
    };
    [
        chunk(json!({"choices": [{"finish_reason": null, "index": 0,
            "delta": {"content": "hi"}}]})),
        chunk(json!({"choices": [{"finish_reason": "stop", "index": 0, "delta": {}}]})),
        chunk(json!({"choices": [], "usage": usage, "timings": timings})),
        "data: [DONE]\n\n".to_owned(),
    ]
    .concat()
}

/// The refusal says a counter both reports name and disagree on has no truthful reading and
/// that neither is believed over the other (`incoming.rs:527-528`; `docs/chat.md`, "a pair
/// that differs is refused ... rather than one of the two believed"). The failure the client
/// hands its caller carries evidence (`client.rs` `retain`), and for the counters' other
/// contradiction, a cached subset above its total, that evidence is dropped
/// (`local_endpoint.rs::a_snapshot_that_cannot_validate_does_not_travel_with_its_failure`,
/// `error.observation.is_none()`). Here it is
/// kept, and it reports `usage`'s 14 output tokens as known: the one report the refusal said it
/// would not believe.
#[tokio::test]
async fn adversary_w11_a_contradiction_refusal_does_not_report_the_contradicted_count() {
    let usage = json!({"completion_tokens": 14, "prompt_tokens": 23, "total_tokens": 37,
        "prompt_tokens_details": {"cached_tokens": 8}});
    // Every counter agrees except the output: 14 in `usage`, 13 in `timings`.
    let timings = json!({"cache_n": 8, "prompt_n": 15, "predicted_n": 13});

    let error = client_turn(stream_with(&usage, &timings))
        .await
        .expect_err("timings that contradict usage are refused");

    assert_eq!(error.code, ErrorCode::Protocol);
    assert_eq!(error.dispatch, Dispatch::Accepted);
    assert_eq!(error.message, CONTRADICTED);
    let reported_output = error
        .observation
        .as_ref()
        .and_then(|observation| observation.usage.as_ref())
        .and_then(|usage| usage.output_tokens);
    assert_eq!(
        reported_output, None,
        "the failure's evidence reports an output count the refusal said neither report could \
         be believed on: {:?}",
        error.observation
    );
}

/// `agree` compares the cached pair on its own (`incoming.rs:533`), but the unit's case moves
/// `cache_n` and with it the input it is part of, so the input pair alone already refuses it.
/// Here the input totals agree (14 + 9 = 23) and only the cached count differs (9 against 8).
/// It must be refused on both wires; with the cached comparison deleted from `agree`, every case
/// of the unit's suite still passes and this one does not.
#[test]
fn adversary_w11_a_cached_count_alone_contradicting_usage_is_refused() {
    let usage = json!({"completion_tokens": 14, "prompt_tokens": 23, "total_tokens": 37,
        "prompt_tokens_details": {"cached_tokens": 8}});
    let timings = json!({"cache_n": 9, "prompt_n": 14, "predicted_n": 14});
    let binding = binding();

    let streamed = project_response_bytes(
        stream_with(&usage, &timings).as_bytes(),
        binding.provenance(),
    )
    .1;
    let whole = decode_completion(
        &json!({
            "choices": [{"index": 0, "message": {"content": "hi"}, "finish_reason": "stop"}],
            "usage": usage,
            "timings": timings
        }),
        binding.provenance(),
    );

    for (wire, result) in [("streamed", streamed), ("non-streamed", whole)] {
        let error = result.expect_err(wire);
        assert_eq!(error.code, ErrorCode::Protocol, "{wire}");
        assert_eq!(error.dispatch, Dispatch::Accepted, "{wire}");
        assert_eq!(error.message, CONTRADICTED, "{wire}");
    }
}
