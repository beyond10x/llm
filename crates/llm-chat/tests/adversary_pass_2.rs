//! Second adversarial pass over `story:chat-projection`, driven from this repository's own
//! contract documents. No production source is changed by this file.
//!
//! Each case names the sentence it drives.
//!
//! - `docs/chat.md` 70-75: "**Every one of those carries dispatch evidence that the request
//!   was served.** Nothing in this direction is reachable before the endpoint has sent bytes
//!   [...] The rule is applied once at each entry point rather than at each refusal, so one
//!   added later cannot escape it."
//! - `crates/llm-chat/tests/incoming.rs` 374-376: "both entry points route every `Err` through
//!   one place, so a refusal added inside cannot escape the rule even if this table is never
//!   extended." The decode direction has **three** public entry points, not two:
//!   `project_response_bytes`, `decode_completion` and `StreamProjection::accept`
//!   (`crates/llm-chat/src/lib.rs` 26, documented at `docs/chat.md` 50-51 as the entry point
//!   "for a caller that already has framed events"). Only the first two apply the rule.
//! - `docs/contract-v1.md` 53-54: "Adapters must attach valid evidence when reporting
//!   cancellation, transport, protocol or sink failure after dispatch."
//! - `crates/llm-core/tests/embedding.rs` 185-191: the repository's own reference `Model`
//!   answers a bounded sink refusing an event with `ErrorCode::TooLarge` and
//!   `Dispatch::Accepted`. `ChatClient` is the only production `impl Model` in the workspace.
mod common;

use common::{binding, binding_at};
use llm_chat::{ChatClient, StreamProjection};
use llm_core::{
    Cancel, Capabilities, Dispatch, ErrorCode, Item, Model, Protocol, TurnRequest, VecSink,
};
use llm_credentials::{ResolvedSecret, SecretError, SecretRef, SecretResolver};
use llm_http::{HttpClient, Limits, SseEvent};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

/// Three text deltas and a terminated stream, in the shape the pinned `OpenAI` fixture is
/// served in. A caller whose sink admits fewer events than the stream produces is the
/// ordinary case, not a contrived one: `VecSink` has no unbounded constructor.
const OPENAI_TEXT: &[u8] = include_bytes!("../fixtures/openai-text-and-usage.sse");

/// A stream that reports its identity and its model, delivers text, and then names a finish
/// reason outside the published subset. The refusal is raised by `StreamProjection::accept`
/// while the stream is still being read, not by `StreamProjection::finish`.
const UNSUPPORTED_FINISH_REASON_MID_STREAM: &[u8] = concat!(
    "data: {\"id\":\"chatcmpl-adv2\",\"model\":\"Qwen/Qwen3-8B\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Guten \"},\"finish_reason\":null}]}\n\n",
    "data: {\"id\":\"chatcmpl-adv2\",\"model\":\"Qwen/Qwen3-8B\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"function_call\"}]}\n\n",
    "data: [DONE]\n\n"
)
.as_bytes();

/// An anonymous binding must never reach a resolver; this one proves it by refusing.
struct NoResolver;
impl SecretResolver for NoResolver {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> llm_core::BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async { Err(SecretError::Missing) })
    }
}

/// Accepts one request and replays pinned bytes over a loopback socket.
async fn serve_once(body: &'static [u8]) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bound");
    let port = listener.local_addr().expect("address").port();
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accepted");
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let read = socket.read(&mut buffer).await.expect("read");
            request.extend_from_slice(&buffer[..read]);
            let text = String::from_utf8_lossy(&request).into_owned();
            let complete = text.split_once("\r\n\r\n").is_some_and(|(head, rest)| {
                head.to_ascii_lowercase()
                    .split_once("content-length:")
                    .and_then(|(_, tail)| tail.split(['\r', ';']).next())
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .is_some_and(|length| rest.len() >= length)
            });
            if complete || read == 0 {
                break;
            }
        }
        let mut response = Vec::from(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
        );
        response.extend_from_slice(body);
        // The client may close mid-stream; that is the case under test, not a server fault.
        let _ = socket.write_all(&response).await;
        let _ = socket.shutdown().await;
    });
    (format!("http://127.0.0.1:{port}/v1"), handle)
}

fn client_at(base_url: &str) -> ChatClient {
    ChatClient::new(
        binding_at(
            Protocol::ChatCompletions,
            Capabilities::text(32_768, 4_096),
            base_url,
        ),
        HttpClient::new(Limits::default()).expect("client"),
        Arc::new(NoResolver),
    )
}

/// `docs/chat.md` 70-75 states the rule for every refusal in the decode direction, and
/// `crates/llm-chat/src/incoming.rs` 330-335 restates it: "Applying the rule at each public
/// entry point rather than at each `return` means a refusal added inside cannot escape it."
///
/// `StreamProjection::accept` is a public entry point of that direction and does not apply
/// it. Every refusal below is reachable only from bytes an endpoint already served, and every
/// one of them reports that the request was never sent.
#[test]
fn the_third_public_entry_point_also_refuses_without_claiming_nothing_was_sent() {
    let binding = binding();
    let cases: &[(&str, Value)] = &[
        (
            "a second choice",
            json!({"choices": [{"index": 1, "delta": {"content": "x"}}]}),
        ),
        (
            "an unknown finish reason",
            json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "function_call"}]}),
        ),
        (
            "a tool call delta with no index",
            json!({"choices": [{"index": 0, "delta": {"tool_calls": [{"id": "c1"}]}}]}),
        ),
        (
            "arguments before their identifier",
            json!({"choices": [{"index": 0, "delta": {"tool_calls": [
                {"index": 0, "function": {"arguments": "{}"}}]}}]}),
        ),
        (
            "an unusable response identifier",
            json!({"id": "has space", "choices": []}),
        ),
        (
            "a counter that is not a count",
            json!({"choices": [], "usage": {"prompt_tokens": "many"}}),
        ),
    ];
    for (case, data) in cases {
        let mut projection = StreamProjection::new(binding.provenance().clone());
        let error = projection
            .accept(&SseEvent::Payload {
                event: None,
                data: data.clone(),
            })
            .expect_err("refused");
        assert_ne!(
            error.dispatch,
            Dispatch::NotSent,
            "{case}: {}",
            error.message
        );
    }
}

/// The same defect reached the way a caller reaches it: `ChatClient::turn`
/// (`crates/llm-chat/src/client.rs` 75) calls `accept` on every event, so a refusal raised
/// while the stream is being read leaves the client reporting that nothing was dispatched —
/// for a turn the endpoint served, streamed text for, and may already have billed.
#[tokio::test]
async fn a_refusal_raised_while_reading_the_stream_is_not_evidence_that_nothing_was_sent() {
    let (base_url, server) = serve_once(UNSUPPORTED_FINISH_REASON_MID_STREAM).await;
    let client = client_at(&base_url);
    let request = TurnRequest::new("small", vec![Item::user("Hallo")]);
    let mut sink = VecSink::new(64, 64 * 1024);
    let model: &dyn Model = &client;
    let error = model
        .turn(&request, &mut sink, &Cancel::new())
        .await
        .expect_err("the endpoint named a finish reason outside the subset");
    let _ = server.await;

    assert_eq!(sink.text(), "Guten ", "the text reached the caller");
    assert_ne!(error.dispatch, Dispatch::NotSent, "{}", error.message);
}

/// `crates/llm-chat/src/client.rs` 111-112: "A failure after dispatch keeps the last valid
/// bound evidence." The endpoint reported its identity and its model before the refusal, and
/// the snapshot validates, so there is valid evidence to keep. `retain` discards it because
/// it reads the refusal's own `not-sent` as proof that nothing was dispatched.
#[tokio::test]
async fn a_refusal_raised_while_reading_the_stream_keeps_the_evidence_it_earned() {
    let (base_url, server) = serve_once(UNSUPPORTED_FINISH_REASON_MID_STREAM).await;
    let client = client_at(&base_url);
    let request = TurnRequest::new("small", vec![Item::user("Hallo")]);
    let mut sink = VecSink::new(64, 64 * 1024);
    let model: &dyn Model = &client;
    let error = model
        .turn(&request, &mut sink, &Cancel::new())
        .await
        .expect_err("the endpoint named a finish reason outside the subset");
    let _ = server.await;

    let observation = error
        .observation
        .as_ref()
        .expect("a failure after dispatch retains the bound evidence it earned");
    assert_eq!(
        observation.upstream_model.as_ref().map(ToString::to_string),
        Some("Qwen/Qwen3-8B".to_owned())
    );
}

/// `docs/contract-v1.md` 53-54 names sink failure after dispatch alongside cancellation,
/// transport and protocol failure, and `crates/llm-core/tests/embedding.rs` 185-191 pins the
/// answer the repository's reference `Model` gives: `TooLarge` and `Accepted`. `ChatClient` is
/// the only production `impl Model` in the workspace and it answers `TooLarge` and `not-sent`.
///
/// `VecSink` has no unbounded constructor, so every caller of this port has a bound; the
/// crate's own tests pick 64 events, which an ordinary answer exceeds.
#[tokio::test]
async fn a_sink_failure_after_dispatch_reports_that_the_request_was_served() {
    let (base_url, server) = serve_once(OPENAI_TEXT).await;
    let client = client_at(&base_url);
    let request = TurnRequest::new("small", vec![Item::user("Hallo")]);
    let mut sink = VecSink::new(1, 64 * 1024);
    let model: &dyn Model = &client;
    let error = model
        .turn(&request, &mut sink, &Cancel::new())
        .await
        .expect_err("the sink admits fewer events than the stream produced");
    let _ = server.await;

    assert_eq!(error.code, ErrorCode::TooLarge);
    assert_eq!(error.dispatch, Dispatch::Accepted, "{}", error.message);
}

/// `docs/chat.md` 65 lists "a second choice" among the refusals, and
/// `crates/llm-chat/tests/incoming.rs` 179 asserts it "is refused rather than silently
/// dropped". The guard at `crates/llm-chat/src/incoming.rs` 299 reads an index it cannot
/// parse — absent, or a JSON number that is not a whole count — as agreement with index
/// zero, so a chunk carrying two such choices is accepted and two separate completions are
/// concatenated into one assistant turn.
#[test]
fn a_second_choice_is_refused_however_the_chunk_numbers_it() {
    let binding = binding();
    let chunks: &[(&str, Value)] = &[
        (
            "no index at all",
            json!({"choices": [{"delta": {"content": "a"}}, {"delta": {"content": "b"}}]}),
        ),
        (
            "an index that is not a whole number",
            json!({"choices": [
                {"index": 0.0, "delta": {"content": "a"}},
                {"index": 1.0, "delta": {"content": "b"}}]}),
        ),
    ];
    for (case, data) in chunks {
        let mut projection = StreamProjection::new(binding.provenance().clone());
        let produced = projection.accept(&SseEvent::Payload {
            event: None,
            data: data.clone(),
        });
        assert!(
            produced.is_err(),
            "{case}: two choices were accepted as one, producing {:?}",
            produced.unwrap_or_default()
        );
    }
}

/// A complete, terminated, consistent stream in which the model proposed a tool the request
/// never published — ordinary model misbehaviour, not server misbehaviour. The endpoint
/// reported forty prompt and twelve completion tokens for it.
const UNPUBLISHED_TOOL_CALL: &[u8] = concat!(
    "data: {\"id\":\"chatcmpl-adv3\",\"model\":\"Qwen/Qwen3-8B\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"weather.lookup\",\"arguments\":\"{}\"}}]},\"finish_reason\":null}]}\n\n",
    "data: {\"id\":\"chatcmpl-adv3\",\"model\":\"Qwen/Qwen3-8B\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
    "data: {\"id\":\"chatcmpl-adv3\",\"model\":\"Qwen/Qwen3-8B\",\"choices\":[],\"usage\":{\"prompt_tokens\":40,\"completion_tokens\":12}}\n\n",
    "data: [DONE]\n\n"
)
.as_bytes();

/// `crates/llm-chat/src/client.rs` 92-93, immediately above the call this drives: "Output
/// already reached the caller's sink, so a refusal here is also a failure after dispatch and
/// keeps the same evidence."
///
/// `TurnOutcome::validate_for` (`crates/llm-core/src/turn.rs` 338-384) builds every one of its
/// refusals with `Error::protocol` and `Error::too_large`, which default to `not-sent`, and
/// `retain` hands a `not-sent` error straight back. So the refusal keeps neither the dispatch
/// nor the evidence, and the counters the endpoint reported reach nobody.
#[tokio::test]
async fn a_refusal_of_the_finished_outcome_is_not_evidence_that_nothing_was_sent() {
    let (base_url, server) = serve_once(UNPUBLISHED_TOOL_CALL).await;
    let client = client_at(&base_url);
    // The request publishes no tools, so the proposed call is one the caller never offered.
    let request = TurnRequest::new("small", vec![Item::user("Wie ist das Wetter?")]);
    let mut sink = VecSink::new(64, 64 * 1024);
    let model: &dyn Model = &client;
    let error = model
        .turn(&request, &mut sink, &Cancel::new())
        .await
        .expect_err("the model proposed a tool the request never published");
    let _ = server.await;

    assert_eq!(
        error.message,
        "model output contains a duplicate call or unpublished tool"
    );
    assert_ne!(error.dispatch, Dispatch::NotSent, "{}", error.message);
}
