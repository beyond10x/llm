//! Adversary pass 1, wave 2026-10-05-w31: live delivery (R30) and kept text (R36). Local
//! sockets only; no provider is contacted.
//!
//! Each case drives a claim the unit wrote about itself: `spec/domains/responses.yaml` ("Live
//! delivery", "Retry agreement", "Kept text"), the `decode_stream` doc comment ("the events and
//! the result are the ones a live caller would get"), and the R36 row ("the turn keeps the text
//! the caller was shown when the terminal output omits it").

use llm_core::{
    AuthKind, BillingKind, BoxFuture, CallId, Cancel, Capabilities, Error, Id, Item, Model,
    Protocol, Provenance, StreamEvent, ToolCall, ToolName, ToolSpec, TurnRequest, VecSink,
};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_http::{HttpClient, Limits};
use llm_providers::{
    Account, BaseUrl, BindingDocument, Endpoint, Provider, ServedModel, ServingModel,
};
use llm_responses::{ResponsesClient, decode_stream};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const UPSTREAM_MODEL: &str = "example/Small-Model";
const SSE_HEAD: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";

fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
}

fn provider_binding(base_url: &str) -> llm_providers::Binding {
    BindingDocument::new(
        Provider {
            id: id("my-lab"),
            category: id("hosted"),
        },
        Account {
            id: id("local"),
            provider_id: id("my-lab"),
            auth_kind: AuthKind::Bearer,
            billing_kind: BillingKind::Metered,
            secret_reference_id: Some(SecretRef::new("responses-key").expect("reference")),
            api_key_header: None,
        },
        Endpoint {
            id: id("local-endpoint"),
            account_id: id("local"),
            base_url: BaseUrl::new(base_url).expect("fixture URL"),
        },
        ServedModel {
            id: id("small"),
            upstream_name: id(UPSTREAM_MODEL),
        },
        ServingModel {
            id: id("serving"),
            endpoint_id: id("local-endpoint"),
            model_id: id("small"),
            protocol: Protocol::Responses,
            capabilities: Capabilities {
                tools: true,
                tool_choice: true,
                temperature: true,
                top_p: true,
                reasoning_efforts: vec!["medium".to_owned()],
                context_window: 32_768,
                max_output_tokens: 2_048,
            },
        },
    )
    .bind()
    .expect("a valid fixture binding")
}

fn decoder_binding(provenance: &Provenance) -> llm_responses::Binding {
    llm_responses::Binding::new(provenance.clone(), id(UPSTREAM_MODEL))
}

fn provenance() -> Provenance {
    Provenance {
        protocol: Protocol::Responses,
        provider: id("my-lab"),
        account: id("local"),
        endpoint: id("local-endpoint"),
        model: id("small"),
        binding_revision: id("rev-1"),
    }
}

struct Resolver;

impl SecretResolver for Resolver {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move {
            Ok(ResolvedSecret {
                secret: Secret::new(b"fixture-token".to_vec())?,
                version: SecretVersion::new("generation-1".to_owned())?,
            })
        })
    }
}

fn client(url: &str) -> ResponsesClient {
    let limit = Duration::from_secs(10);
    ResponsesClient::new(
        provider_binding(url),
        HttpClient::new(Limits {
            response_headers: limit,
            idle: limit,
            total: limit,
        })
        .expect("bounded transport"),
        Arc::new(Resolver),
    )
    .expect("a Responses binding")
}

fn request() -> TurnRequest {
    let mut request = TurnRequest::new("small", vec![Item::user("Say hello")]);
    request.tools = vec![ToolSpec {
        name: ToolName::new("file_read").expect("tool name"),
        description: "A fixture tool.".to_owned(),
        input_schema: json!({"type": "object"}),
    }];
    request
}

fn created() -> Value {
    json!({"type": "response.created",
        "response": {"id": "resp_1", "status": "in_progress", "output": []}})
}

fn text_delta(item_id: &str, output_index: u64, delta: &str) -> Value {
    json!({"type": "response.output_text.delta", "item_id": item_id,
        "output_index": output_index, "content_index": 0, "delta": delta})
}

fn completed(output: &Value) -> Value {
    json!({"type": "response.completed", "response": {
        "id": "resp_1", "status": "completed", "model": UPSTREAM_MODEL, "output": output,
        "usage": {"input_tokens": 20, "output_tokens": 9}}})
}

fn message_done(item_id: &str, output_index: u64, text: &str) -> Value {
    json!({"type": "response.output_item.done", "output_index": output_index,
        "item": {"type": "message", "id": item_id, "role": "assistant", "status": "completed",
            "content": [{"type": "output_text", "text": text}]}})
}

fn call_done(output_index: u64) -> Value {
    json!({"type": "response.output_item.done", "output_index": output_index,
        "item": {"type": "function_call", "id": "fc_1", "call_id": "call_1",
            "name": "file_read", "arguments": "{\"path\":\"README.md\"}", "status": "completed"}})
}

fn call_item() -> Item {
    Item::ToolCall(ToolCall {
        call_id: CallId::new("call_1").expect("call id"),
        name: ToolName::new("file_read").expect("tool name"),
        arguments: json!({"path": "README.md"}),
    })
}

fn sse(events: &[Value]) -> Vec<u8> {
    let mut body = Vec::new();
    for event in events {
        let kind = event["type"].as_str().expect("an event type");
        body.extend_from_slice(format!("event: {kind}\ndata: {event}\n\n").as_bytes());
    }
    body
}

/// Serves `head` and `body` to the one connection, then closes it.
async fn serve(body: Vec<u8>) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("an accepted connection");
        let mut seen = Vec::new();
        let mut buffer = vec![0; 1 << 16];
        loop {
            let length = socket.read(&mut buffer).await.expect("a readable socket");
            assert!(length > 0, "the client closed before sending a request");
            seen.extend_from_slice(&buffer[..length]);
            if let Some(end) = seen.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&seen[..end]).to_ascii_lowercase();
                let length: usize = head
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .map_or(0, |value| value.trim().parse().expect("a numeric length"));
                if seen.len() >= end + 4 + length {
                    break;
                }
            }
        }
        let _ = socket.write_all(SSE_HEAD).await;
        let _ = socket.write_all(&body).await;
        let _ = socket.shutdown().await;
    });
    (url, server)
}

// ---------------------------------------------------------------------------------------------
// decode_stream is documented as the live client's decoder "fed the whole sequence at once":
// "the events and the result are the ones a live caller would get". The spec says the sink sees
// "the same sequence `DecodeStream` reports as `stream_events` for the whole stream".
// ---------------------------------------------------------------------------------------------

/// A server that sends one more delta after `response.completed`. The client stops reading at
/// the terminal object; `decode_stream` reads on, shows the extra delta and keeps it in the
/// turn's text. The conformance suite drives `decode_stream`, so it certifies a turn the client
/// never returns.
#[tokio::test]
async fn decode_stream_and_the_live_client_agree_on_a_stream_that_continues_past_its_terminal_object()
 {
    let payloads = vec![
        created(),
        text_delta("msg_1", 0, "Hel"),
        completed(&json!([])),
        text_delta("msg_1", 0, "lo"),
    ];
    let (url, server) = serve(sse(&payloads)).await;
    let model = client(&url);
    let mut sink = VecSink::new(64, 64 * 1024);
    let live = model
        .turn(&request(), &mut sink, &Cancel::new())
        .await
        .expect("a completed turn");
    server.await.expect("the fixture server");

    let decoded = decode_stream(&decoder_binding(model.provenance()), &payloads);
    let decoded_outcome = decoded.result.expect("a decoded turn");
    assert_eq!(
        (sink.events(), live.items.as_slice()),
        (decoded.events.as_slice(), decoded_outcome.items.as_slice()),
        "decode_stream reports events and a kept text the live client never shows or returns"
    );
}

// ---------------------------------------------------------------------------------------------
// R36: "the turn keeps the text the caller was shown when the terminal output omits it".
// ---------------------------------------------------------------------------------------------

/// Two messages streamed, the first finished with `response.output_item.done`, the second only
/// as deltas, then an empty terminal `output` (the Codex shape). The terminal object carries no
/// text at all, yet the second answer the caller watched arrive is dropped: the kept-text rule
/// is skipped because a *streamed* item carries assistant text, though the spec's reason for
/// skipping it is that "the terminal object stays authoritative".
#[test]
fn a_second_message_shown_only_as_deltas_is_kept_when_the_terminal_output_is_empty() {
    let payloads = vec![
        created(),
        text_delta("msg_1", 0, "Working on it."),
        message_done("msg_1", 0, "Working on it."),
        text_delta("msg_2", 1, "The answer is 42."),
        completed(&json!([])),
    ];
    let decoding = decode_stream(&decoder_binding(&provenance()), &payloads);
    let outcome = decoding.result.expect("a completed turn");
    assert_eq!(
        outcome.items,
        vec![
            Item::assistant("Working on it."),
            Item::assistant("The answer is 42.")
        ],
        "the caller was shown text the turn does not carry"
    );
}

/// Kept text is "placed at the `output_index` of its first delta". A message at index 1 whose
/// first delta arrives before the one at index 0, beside a call at index 2: the kept messages
/// must come out in output order, ahead of the call.
#[test]
fn kept_messages_are_placed_in_output_order_whatever_order_their_first_deltas_arrived_in() {
    let payloads = vec![
        created(),
        text_delta("msg_b", 1, "B"),
        text_delta("msg_a", 0, "A"),
        call_done(2),
        completed(&json!([])),
    ];
    let decoding = decode_stream(&decoder_binding(&provenance()), &payloads);
    let outcome = decoding.result.expect("a completed turn");
    assert_eq!(
        outcome.items,
        vec![Item::assistant("A"), Item::assistant("B"), call_item()],
        "kept text misplaced against its output_index"
    );
}

// ---------------------------------------------------------------------------------------------
// Retry agreement: a `keepalive` produces no output and shows nothing, so routing's
// visible-output rule would retry; the client's answered rule marks the cut final.
// ---------------------------------------------------------------------------------------------

/// Harness keeps a `keepalive` as progress that "advances no turn"
/// (`harness-responses/src/lib.rs:421`, test `:854`) and retries any attempt that showed the
/// caller nothing (`provider_emulated.rs:521`). A cut after `response.created` and a keepalive
/// has answered nothing.
#[tokio::test]
async fn a_cut_after_only_a_keepalive_stays_retriable() {
    let mut body = sse(&[created(), json!({"type": "keepalive"})]);
    body.extend_from_slice(b"event: response.output_text.delta\ndata: {\"type\":\"response.output");
    let (url, server) = serve(body).await;
    let model = client(&url);
    let mut sink = VecSink::new(64, 64 * 1024);
    let error: Error = model
        .turn(&request(), &mut sink, &Cancel::new())
        .await
        .expect_err("the stream was cut before a terminal object");
    server.await.expect("the fixture server");
    assert_eq!(sink.events(), [] as [StreamEvent; 0]);
    assert!(
        error.may_retry(),
        "a keepalive answered nothing, yet the cut was made final: {error:?}"
    );
}
