//! An outgoing call against a local vLLM-compatible endpoint fixture.
//!
//! The server below replays pinned bytes over a loopback socket. It is a local fixture and
//! establishes no live vLLM, provider or hosting qualification.
mod common;

use common::binding_at;
use llm_chat::ChatClient;
use llm_core::{
    ADVISORY_OMITTED, Cancel, Capabilities, Dispatch, ErrorCode, Item, Model, PromptCache,
    Protocol, ReasoningSummary, StopReason, StreamEvent, TurnRequest, VecSink, Verbosity,
};
use llm_credentials::{AnonymousResolver, ResolvedSecret, SecretError, SecretRef, SecretResolver};
use llm_http::{HttpClient, Limits};
use std::sync::Arc;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const VLLM_TEXT: &[u8] = include_bytes!("../fixtures/vllm-text-no-usage.sse");

/// A complete, terminated stream whose reported counters contradict each other: the cached
/// subset exceeds the input total. Every byte of it arrived from the endpoint.
const CONTRADICTORY_COUNTERS: &[u8] = concat!(
    "data: {\"id\":\"chatcmpl-c1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"}}]}\n\n",
    "data: {\"id\":\"chatcmpl-c1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
    "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"prompt_tokens_details\":{\"cached_tokens\":9}}}\n\n",
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

/// Accepts one request, returns the pinned stream, and hands back what the client sent.
async fn serve_once(body: &'static [u8]) -> (String, tokio::task::JoinHandle<String>) {
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
        String::from_utf8_lossy(&request).into_owned()
    });
    (format!("http://127.0.0.1:{port}/v1"), handle)
}

#[tokio::test]
async fn a_local_vllm_compatible_endpoint_streams_through_the_neutral_port() {
    let (base_url, server) = serve_once(VLLM_TEXT).await;
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
    let outcome = model
        .turn(&request, &mut sink, &Cancel::new())
        .await
        .expect("the local endpoint answered");

    assert_eq!(sink.text(), "Guten Tag");
    assert!(sink.events().iter().any(|event| matches!(
        event,
        StreamEvent::ReasoningDelta { text } if text == "The user greets me."
    )));
    assert_eq!(outcome.items, vec![Item::assistant("Guten Tag")]);
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
    assert_eq!(outcome.observation.usage, None);
    assert!(outcome.observation.final_usage);
    assert_eq!(
        outcome
            .observation
            .upstream_model
            .as_ref()
            .map(ToString::to_string),
        Some("Qwen/Qwen3-8B".to_owned())
    );
    outcome
        .validate_for(&request, client.provenance())
        .expect("the neutral outcome is valid for this request and binding");

    let sent = server.await.expect("server finished");
    let (head, body) = sent.split_once("\r\n\r\n").expect("a complete request");
    assert!(head.starts_with("POST /v1/chat/completions "), "{head}");
    // An explicitly anonymous binding sends no credential header at all.
    assert!(
        !head.to_ascii_lowercase().contains("authorization"),
        "{head}"
    );
    let body: serde_json::Value = serde_json::from_str(body).expect("a JSON body");
    assert_eq!(body["model"], serde_json::json!("Qwen/Qwen3-8B"));
    assert_eq!(body["stream"], serde_json::json!(true));
    assert_eq!(
        body["stream_options"],
        serde_json::json!({"include_usage": true})
    );
}

/// A caller that builds the client itself, without the catalog, hands an anonymous binding the
/// resolver llm exports for it. The turn is served, which a resolve reaching that resolver would
/// have refused, and no credential header is sent.
#[tokio::test]
async fn a_client_built_with_the_exported_anonymous_resolver_runs_a_turn_and_sends_no_credential() {
    let (base_url, server) = serve_once(VLLM_TEXT).await;
    let client = ChatClient::new(
        binding_at(
            Protocol::ChatCompletions,
            Capabilities::text(32_768, 4_096),
            &base_url,
        ),
        HttpClient::new(Limits::default()).expect("client"),
        Arc::new(AnonymousResolver),
    );

    let request = TurnRequest::new("small", vec![Item::user("Hallo")]);
    let mut sink = VecSink::new(64, 64 * 1024);
    let model: &dyn Model = &client;
    let outcome = model
        .turn(&request, &mut sink, &Cancel::new())
        .await
        .expect("the local endpoint answered");
    assert_eq!(sink.text(), "Guten Tag");
    assert_eq!(outcome.items, vec![Item::assistant("Guten Tag")]);
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);

    let sent = server.await.expect("server finished");
    let (head, _) = sent.split_once("\r\n\r\n").expect("a complete request");
    assert!(head.starts_with("POST /v1/chat/completions "), "{head}");
    assert!(
        !head.to_ascii_lowercase().contains("authorization"),
        "{head}"
    );
}

/// The snapshot a failure carries is re-validated by `Error::validate_for`, so an invalid one
/// does not merely lose the counters — it makes the failure itself unreportable. When the
/// evidence an attempt earned cannot pass that check, the dispatch travels alone.
#[tokio::test]
async fn a_snapshot_that_cannot_validate_does_not_travel_with_its_failure() {
    let (base_url, server) = serve_once(CONTRADICTORY_COUNTERS).await;
    let binding = binding_at(
        Protocol::ChatCompletions,
        Capabilities::text(32_768, 4_096),
        &base_url,
    );
    let target = binding.provenance().clone();
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
        .expect_err("the endpoint reported counters that contradict each other");
    let _ = server.await;

    // The request was served, and the refusal says so.
    assert_eq!(error.dispatch, Dispatch::Accepted);
    assert!(error.observation.is_none(), "{:?}", error.observation);
    // The point of dropping it: the failure a caller receives is one the core will accept.
    error
        .validate_for(&target)
        .expect("the failure is itself reportable");
}

// ---------------------------------------------------------------- advisory warnings

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

fn advisory_warnings(events: &[StreamEvent]) -> Vec<&StreamEvent> {
    events
        .iter()
        .filter(
            |event| matches!(event, StreamEvent::Warning { code, .. } if code == ADVISORY_OMITTED),
        )
        .collect()
}

/// A request carrying a hint this wire has no field for, and one it sends.
fn hinted_request() -> TurnRequest {
    let mut request = TurnRequest::new("small", vec![Item::user("Hallo")]);
    request.controls.hints.prompt_cache = Some(PromptCache { ttl: None });
    request.controls.hints.reasoning_summary = Some(ReasoningSummary::Auto);
    request.controls.hints.prompt_cache_key = Some("session-7".to_owned());
    request
}

#[tokio::test]
async fn a_successful_turn_ends_with_one_warning_naming_the_omitted_hints() {
    let (base_url, server) = serve_once(VLLM_TEXT).await;
    let client = client_at(&base_url);
    let request = hinted_request();
    let mut sink = VecSink::new(64, 64 * 1024);
    let model: &dyn Model = &client;
    model
        .turn(&request, &mut sink, &Cancel::new())
        .await
        .expect("the local endpoint answered");

    let events = sink.events();
    assert_eq!(advisory_warnings(events).len(), 1, "{events:?}");
    // After the turn's own output, never before it.
    let Some(StreamEvent::Warning { code, message }) = events.last() else {
        panic!("the warning is not the last event: {events:?}");
    };
    assert_eq!(code, ADVISORY_OMITTED);
    assert_eq!(
        message,
        "Chat Completions carries no prompt-cache, reasoning-summary; the advisory hint was not sent"
    );

    let sent = server.await.expect("server finished");
    let (_, body) = sent.split_once("\r\n\r\n").expect("a complete request");
    let body: serde_json::Value = serde_json::from_str(body).expect("a JSON body");
    assert_eq!(body["prompt_cache_key"], serde_json::json!("session-7"));
}

#[tokio::test]
async fn a_turn_that_omits_nothing_emits_no_warning() {
    let (base_url, server) = serve_once(VLLM_TEXT).await;
    let client = client_at(&base_url);
    let mut request = TurnRequest::new("small", vec![Item::user("Hallo")]);
    request.controls.hints.prompt_cache_key = Some("session-7".to_owned());
    request.controls.hints.verbosity = Some(Verbosity::Low);
    let mut sink = VecSink::new(64, 64 * 1024);
    let model: &dyn Model = &client;
    model
        .turn(&request, &mut sink, &Cancel::new())
        .await
        .expect("the local endpoint answered");
    let _ = server.await;
    assert!(
        advisory_warnings(sink.events()).is_empty(),
        "{:?}",
        sink.events()
    );
}

#[tokio::test]
async fn a_failed_turn_emits_no_advisory_warning() {
    let (base_url, server) = serve_once(CONTRADICTORY_COUNTERS).await;
    let client = client_at(&base_url);
    let request = hinted_request();
    let mut sink = VecSink::new(64, 64 * 1024);
    let model: &dyn Model = &client;
    model
        .turn(&request, &mut sink, &Cancel::new())
        .await
        .expect_err("the endpoint reported counters that contradict each other");
    let _ = server.await;
    assert!(
        advisory_warnings(sink.events()).is_empty(),
        "{:?}",
        sink.events()
    );
}

/// A sink with room for the turn's own output and nothing more refuses the warning, and the
/// caller receives that refusal. The turn was served, so it carries dispatch evidence.
#[tokio::test]
async fn a_sink_refusing_the_warning_fails_the_turn_with_its_own_error() {
    let (base_url, server) = serve_once(VLLM_TEXT).await;
    let mut sink = VecSink::new(64, 64 * 1024);
    let model: &dyn Model = &client_at(&base_url);
    model
        .turn(
            &TurnRequest::new("small", vec![Item::user("Hallo")]),
            &mut sink,
            &Cancel::new(),
        )
        .await
        .expect("the local endpoint answered");
    let _ = server.await;
    let output_events = sink.events().len();

    let (base_url, server) = serve_once(VLLM_TEXT).await;
    let mut sink = VecSink::new(output_events, 64 * 1024);
    let model: &dyn Model = &client_at(&base_url);
    let error = model
        .turn(&hinted_request(), &mut sink, &Cancel::new())
        .await
        .expect_err("the sink refused the warning");
    let _ = server.await;
    assert_eq!(error.code, ErrorCode::TooLarge);
    assert_eq!(error.message, "stream sink event bound reached");
    assert_eq!(error.dispatch, Dispatch::Accepted);
    assert_eq!(sink.events().len(), output_events);
}
