//! Adversary, wave 2026-10-21-w12, unit 1 (story:timings-before-the-last-chunk), pass 1:
//! llama-server's `timings` read only from the report that ends a stream.
//!
//! The unit's suite drives the decoder through `project_response_bytes` and a hand-fed
//! `StreamProjection`. These cases add what it leaves open: the evidence the client a caller
//! holds attaches to a failure after a cut (`client.rs` `retain`), the per-token layout without
//! a usage chunk, where the finish chunk is the report, and a chunk with an empty `choices` array
//! before the finish reason, which no case of the unit's suite sends. Local sockets and in-memory
//! bodies only; no provider is contacted.
mod common;

use common::{binding, binding_at};
use llm_chat::{ChatClient, project_response_bytes};
use llm_core::{
    Cancel, Capabilities, Dispatch, Error, Item, Model, Protocol, StopReason, TurnOutcome,
    TurnRequest, Usage, VecSink,
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

struct NoResolver;
impl SecretResolver for NoResolver {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> llm_core::BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async { Err(SecretError::Missing) })
    }
}

/// One turn through the client a caller holds, against a loopback server replaying `body` and
/// then closing the connection.
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

/// One llama-server chunk in the shape `server-task.cpp:1103-1121` at `7879491cf` writes.
fn chunk(extra: &Value) -> String {
    let mut chunk = json!({"created": 1_792_600_000, "id": "chatcmpl-adversaryw12",
        "model": "qwen3-1.7b", "system_fingerprint": "b11752-7879491cf",
        "object": "chat.completion.chunk"});
    for (key, value) in extra.as_object().expect("an object") {
        chunk[key] = value.clone();
    }
    format!("data: {chunk}\n\n")
}

/// The counters `timings_per_token` writes after `predicted` output tokens of a 35-token
/// prompt, none reused (`server-context.cpp:2222-2225`).
fn timings(predicted: u32) -> Value {
    json!({"cache_n": 0, "prompt_n": 35, "prompt_ms": 301.5,
        "prompt_per_token_ms": 8.614_285_714_285_714, "prompt_per_second": 116.086_235_489_220_56,
        "predicted_n": predicted, "predicted_ms": 70.1 * f64::from(predicted),
        "predicted_per_token_ms": 70.1, "predicted_per_second": 14.265_335_235_378_032})
}

/// A content chunk under `timings_per_token`: the delta and the counters so far.
fn per_token(text: &str, predicted: u32) -> String {
    chunk(&json!({
        "choices": [{"finish_reason": null, "index": 0, "delta": {"content": text}}],
        "timings": timings(predicted)
    }))
}

fn counted(output: u64) -> Usage {
    Usage {
        input_tokens: Some(35),
        output_tokens: Some(output),
        cached_input_tokens: Some(0),
        cache_creation_input_tokens: None,
        reasoning_output_tokens: None,
    }
}

/// A connection cut mid-stream under `timings_per_token`. The failure the client hands its
/// caller carries the evidence earned so far (`client.rs` `retain`), and the story says a
/// `timings` before the report that ends the stream is not the turn's count, not even as a
/// snapshot. The evidence travels — it names the model the chunks reported — and carries no
/// output count of 2 for a turn the server had not finished.
#[tokio::test]
async fn adversary_w12_a_cut_after_per_token_timings_reports_no_count_in_its_evidence() {
    let body = [
        chunk(&json!({"choices": [{"finish_reason": null, "index": 0,
            "delta": {"role": "assistant", "content": null}}]})),
        per_token("Hel", 1),
        per_token("lo", 2),
    ]
    .concat();

    let error = client_turn(body).await.expect_err("a cut stream fails");

    assert_eq!(error.dispatch, Dispatch::Accepted);
    let observation = error
        .observation
        .as_ref()
        .unwrap_or_else(|| panic!("the failure carries no evidence at all: {error:?}"));
    assert_eq!(
        observation.upstream_model.as_ref().map(ToString::to_string),
        Some("qwen3-1.7b".to_owned()),
        "{observation:?}"
    );
    assert!(!observation.final_usage, "{observation:?}");
    assert_eq!(observation.usage, None, "{observation:?}");
}

/// The other half of the same cut: once the chunk carrying the finish reason has arrived with
/// its `timings` (a request without `include_usage`, `server-task.cpp:508-510`), that report
/// ends the stream, and a cut before `[DONE]` keeps it as the snapshot: four output tokens, not
/// the two of an earlier chunk and not none, and never final.
#[tokio::test]
async fn adversary_w12_a_cut_after_the_finish_chunk_keeps_its_timings_as_the_snapshot() {
    let body = [
        per_token("Hel", 1),
        per_token("lo", 2),
        per_token(" there", 3),
        per_token(".", 4),
        chunk(&json!({
            "choices": [{"finish_reason": "stop", "index": 0, "delta": {}}],
            "timings": timings(4)
        })),
    ]
    .concat();

    let error = client_turn(body).await.expect_err("no terminal sentinel");

    let observation = error
        .observation
        .as_ref()
        .unwrap_or_else(|| panic!("the failure carries no evidence at all: {error:?}"));
    assert!(!observation.final_usage, "{observation:?}");
    assert_eq!(observation.usage, Some(counted(4)), "{observation:?}");
}

/// `timings_per_token` on a request without `include_usage`: every content chunk carries the
/// counters so far, and the chunk carrying the finish reason carries the final ones
/// (`server-task.cpp:494-510`: `timings` goes on the last delta, which is the finish chunk when
/// there is no usage chunk). The unit's per-token case has a usage chunk; this layout has none,
/// and the turn's count is the finish chunk's four.
#[test]
fn adversary_w12_per_token_timings_without_a_usage_chunk_count_from_the_finish_chunk() {
    let body = [
        chunk(&json!({"choices": [{"finish_reason": null, "index": 0,
            "delta": {"role": "assistant", "content": null}}]})),
        per_token("Hel", 1),
        per_token("lo", 2),
        per_token(" there", 3),
        per_token(".", 4),
        chunk(&json!({
            "choices": [{"finish_reason": "stop", "index": 0, "delta": {}}],
            "timings": timings(4)
        })),
        "data: [DONE]\n\n".to_owned(),
    ]
    .concat();
    let binding = binding();

    let outcome = project_response_bytes(body.as_bytes(), binding.provenance())
        .1
        .unwrap_or_else(|error| panic!("{error:?}"));

    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
    assert_eq!(outcome.items, vec![Item::assistant("Hello there.")]);
    assert!(outcome.observation.final_usage);
    assert_eq!(outcome.observation.usage, Some(counted(4)));
}

/// `finishes` asks whether any choice carries a finish reason. A chunk with an empty `choices`
/// array carries none, so it does not end the stream and its `timings` is dropped. No case of the
/// unit's suite sends such a chunk before the finish reason, so `any` replaced by `all` in
/// `finishes` (vacuously true on an empty array) leaves the whole suite green; this case is red
/// against that mutant and green against the unit.
#[test]
fn adversary_w12_a_chunk_with_no_choices_before_the_finish_reason_does_not_end_the_stream() {
    let body = [
        per_token("Hel", 1),
        chunk(&json!({"choices": [], "timings": timings(2)})),
        chunk(&json!({"choices": [{"finish_reason": null, "index": 0,
            "delta": {"content": "lo"}}]})),
        chunk(&json!({"choices": [{"finish_reason": "stop", "index": 0, "delta": {}}]})),
        "data: [DONE]\n\n".to_owned(),
    ]
    .concat();
    let binding = binding();

    let outcome = project_response_bytes(body.as_bytes(), binding.provenance())
        .1
        .unwrap_or_else(|error| panic!("{error:?}"));

    assert!(outcome.observation.final_usage);
    assert_eq!(outcome.observation.usage, None);
}
