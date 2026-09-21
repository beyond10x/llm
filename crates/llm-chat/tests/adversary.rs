//! Adversarial cases driven from this repository's own contract documents.
//!
//! Each case names the sentence it drives. No production source is changed by this file.
//!
//! - `docs/contract-v1.md`: "Adapters must attach valid evidence when reporting cancellation,
//!   transport, protocol or sink failure after dispatch." and "Dispatch evidence is
//!   independent: `not-sent`, `rejected`, `unknown`, or `accepted`."
//! - `crates/llm-chat/src/client.rs`: "A failure after dispatch keeps the last valid bound
//!   evidence, including the partial usage of an interrupted stream."
//! - `docs/chat.md`: a reported cache-write counter "travels under
//!   `prompt_tokens_details.cache_creation_input_tokens` rather than being deleted".
mod common;

use common::{binding, binding_at};
use llm_chat::{ChatClient, decode_completion, encode_ingress_completion, project_response_bytes};
use llm_core::{
    Cancel, Capabilities, Dispatch, Item, Model, Protocol, StopReason, TurnObservation,
    TurnOutcome, TurnRequest, Usage, VecSink,
};
use llm_credentials::{ResolvedSecret, SecretError, SecretRef, SecretResolver};
use llm_http::{HttpClient, Limits};
use serde_json::json;
use std::sync::Arc;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

/// A complete, terminated stream whose reported counters contradict each other. Every byte
/// of it arrived from the endpoint, and it names two counters the attempt consumed.
const CONTRADICTORY_COUNTERS: &str = concat!(
    "data: {\"id\":\"chatcmpl-adv-1\",\"model\":\"Qwen/Qwen3-8B\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
    "data: {\"id\":\"chatcmpl-adv-1\",\"model\":\"Qwen/Qwen3-8B\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
    "data: {\"id\":\"chatcmpl-adv-1\",\"model\":\"Qwen/Qwen3-8B\",\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":1,\"prompt_tokens_details\":{\"cached_tokens\":9}}}\n\n",
    "data: [DONE]\n\n"
);

/// A complete, terminated stream whose one tool call never carried its name. The reported
/// counters are consistent, so the evidence this attempt earned is valid on its own.
const TOOL_CALL_WITHOUT_A_NAME: &str = concat!(
    "data: {\"id\":\"chatcmpl-adv-2\",\"model\":\"Qwen/Qwen3-8B\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"arguments\":\"{}\"}}]},\"finish_reason\":null}]}\n\n",
    "data: {\"id\":\"chatcmpl-adv-2\",\"model\":\"Qwen/Qwen3-8B\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
    "data: {\"id\":\"chatcmpl-adv-2\",\"model\":\"Qwen/Qwen3-8B\",\"choices\":[],\"usage\":{\"prompt_tokens\":40,\"completion_tokens\":12}}\n\n",
    "data: [DONE]\n\n"
);

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
        socket.write_all(&response).await.expect("written");
        socket.shutdown().await.expect("closed");
    });
    (format!("http://127.0.0.1:{port}/v1"), handle)
}

/// `docs/contract-v1.md`: dispatch evidence is `not-sent`, `rejected`, `unknown` or
/// `accepted`, and "a transport failure after dispatch is unknown, not proof of a free
/// retry". `StreamProjection::finish` stamps `Accepted` on its truncation and
/// missing-finish-reason refusals and on none of the others, so a refusal that can only be
/// reached after the whole response arrived reports that nothing was ever sent.
#[test]
fn a_contradictory_counter_refusal_is_not_evidence_that_nothing_was_sent() {
    let binding = binding();
    let (_, outcome) =
        project_response_bytes(CONTRADICTORY_COUNTERS.as_bytes(), binding.provenance());
    let error = outcome.expect_err("refused");
    assert_eq!(error.message, "reported usage subsets exceed totals");
    assert_eq!(error.dispatch, Dispatch::Accepted);
}

/// The same defect on the other refusal `finish` can only reach after a terminated stream.
#[test]
fn an_incomplete_tool_call_refusal_is_not_evidence_that_nothing_was_sent() {
    let binding = binding();
    let (_, outcome) =
        project_response_bytes(TOOL_CALL_WITHOUT_A_NAME.as_bytes(), binding.provenance());
    let error = outcome.expect_err("refused");
    assert_eq!(error.message, "chat tool call carries no name");
    assert_eq!(error.dispatch, Dispatch::Accepted);
}

/// End to end over a real socket, which is the path `tests/local_endpoint.rs` already
/// drives. `crates/llm-chat/src/client.rs` says "A failure after dispatch keeps the last
/// valid bound evidence"; `docs/contract-v1.md` says an adapter "must attach valid evidence
/// when reporting cancellation, transport, protocol or sink failure after dispatch". The
/// endpoint reported 40 prompt and 12 completion tokens and they reach nobody.
#[tokio::test]
async fn a_protocol_failure_after_dispatch_keeps_the_counters_the_endpoint_reported() {
    let (base_url, server) = serve_once(TOOL_CALL_WITHOUT_A_NAME.as_bytes()).await;
    let binding = binding_at(
        Protocol::ChatCompletions,
        Capabilities::text(32_768, 4_096),
        &base_url,
    );
    let client = ChatClient::new(
        binding,
        HttpClient::new(Limits::default()).expect("client"),
        Arc::new(NoResolver),
    );
    let request = TurnRequest::new("small", vec![Item::user("Hallo")]);
    let mut sink = VecSink::new(64, 64 * 1024);
    let model: &dyn Model = &client;
    let error = model
        .turn(&request, &mut sink, &Cancel::new())
        .await
        .expect_err("the endpoint answered with an unusable tool call");
    server.await.expect("server finished");

    let observation = error
        .observation
        .as_ref()
        .expect("a failure after dispatch retains the bound evidence it earned");
    assert_eq!(
        observation.usage,
        Some(Usage {
            input_tokens: Some(40),
            output_tokens: Some(12),
            cached_input_tokens: None,
            cache_creation_input_tokens: None,
            reasoning_output_tokens: None,
        })
    );
}

/// `docs/chat.md`: a reported cache-write counter "travels under
/// `prompt_tokens_details.cache_creation_input_tokens` rather than being deleted". This
/// crate writes that field and is the only reader of this wire the repository ships, and it
/// deletes it: a counter the upstream reported does not survive its own wire.
#[test]
fn the_cache_write_counter_this_wire_names_survives_being_read_back() {
    let binding = binding();
    let mut observation = TurnObservation::new(binding.provenance().clone());
    observation.usage = Some(Usage {
        input_tokens: Some(10),
        output_tokens: None,
        cached_input_tokens: None,
        cache_creation_input_tokens: Some(4),
        reasoning_output_tokens: None,
    });
    observation.final_usage = true;
    let outcome = TurnOutcome {
        stop_reason: StopReason::EndTurn,
        items: vec![Item::assistant("Hi")],
        observation,
    };

    let body =
        encode_ingress_completion(&outcome, "chatcmpl-adv-4", 1_772_000_000).expect("encoded");
    assert_eq!(
        body["usage"]["prompt_tokens_details"]["cache_creation_input_tokens"],
        json!(4)
    );

    let read_back = decode_completion(&body, binding.provenance()).expect("decoded");
    assert_eq!(
        read_back
            .observation
            .usage
            .and_then(|usage| usage.cache_creation_input_tokens),
        Some(4)
    );
}

/// The counter every fixture in this unit reports. No case in the suite reads a `usage`
/// object that omits `prompt_tokens`, so an input count invented on the decode side is
/// unobserved. This case is the one that would see it.
#[test]
fn a_usage_report_that_omits_the_input_counter_leaves_it_unknown() {
    let binding = binding();
    let body = json!({
        "id": "chatcmpl-adv-5",
        "object": "chat.completion",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "ok"},
            "finish_reason": "stop"
        }],
        "usage": {"completion_tokens": 9, "total_tokens": 9}
    });
    let outcome = decode_completion(&body, binding.provenance()).expect("decoded");
    assert_eq!(
        outcome.observation.usage,
        Some(Usage {
            input_tokens: None,
            output_tokens: Some(9),
            cached_input_tokens: None,
            cache_creation_input_tokens: None,
            reasoning_output_tokens: None,
        })
    );
}
