//! Adversarial cases, second pass, for "every event is final" on Responses
//! (`story:text-deltas-are-final`, correction 61f80713).
//!
//! The pairing under attack (`spec/domains/responses.yaml` "Kept text", `docs/responses.md`):
//! by `id`, then by `output_index`, then by order only where no coordinate tells the two apart;
//! different ids or different indexes never pair, "so a message no delta showed keeps the
//! server's text, with no warning, wherever it sits". The port rule above it
//! (`crates/llm-core/src/port.rs`): each message carries exactly the deltas shown for it, and a
//! message no delta was shown for carries the server's text. No provider is contacted.

use llm_core::{
    ADVISORY_OMITTED, AuthKind, BillingKind, BoxFuture, Cancel, Capabilities, Id, Item, Model,
    Protocol, Provenance, StreamEvent, TurnRequest, Unforwarded, VecSink,
};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_http::{HttpClient, Limits};
use llm_providers::{
    Account, BaseUrl, BindingDocument, Endpoint, Provider, ServedModel, ServingModel,
};
use llm_responses::{Binding, ResponsesClient, decode_stream};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const UPSTREAM_MODEL: &str = "example/Small-Model";
const SHOWN_TEXT_KEPT: &str = "shown-text-kept";

fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
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

fn binding() -> Binding {
    Binding::new(provenance(), id(UPSTREAM_MODEL))
}

fn created() -> Value {
    json!({"type": "response.created",
        "response": {"id": "resp_1", "status": "in_progress", "output": []}})
}

/// A text delta naming `item_id` and `output_index` where given, and omitting each where not.
fn delta(item_id: Option<&str>, output_index: Option<u64>, text: &str) -> Value {
    let mut event = json!({"type": "response.output_text.delta", "content_index": 0,
        "delta": text});
    if let Some(item_id) = item_id {
        event["item_id"] = json!(item_id);
    }
    if let Some(output_index) = output_index {
        event["output_index"] = json!(output_index);
    }
    event
}

fn message(id: Option<&str>, text: &str) -> Value {
    let mut message = json!({"type": "message", "role": "assistant", "status": "completed",
        "content": [{"type": "output_text", "text": text, "annotations": []}]});
    if let Some(id) = id {
        message["id"] = json!(id);
    }
    message
}

/// `response.output_item.done` for `item`, naming `output_index` where given.
fn done(output_index: Option<u64>, item: &Value) -> Value {
    let mut event = json!({"type": "response.output_item.done", "item": item});
    if let Some(output_index) = output_index {
        event["output_index"] = json!(output_index);
    }
    event
}

/// Harness's emulator reasoning item (`harness-responses/tests/fixtures/fake_responses.py`
/// `reasoning_item`).
fn reasoning_item() -> Value {
    json!({"id": "rs_b10x_001", "type": "reasoning", "summary": [],
        "encrypted_content": "OPAQUE-REASONING-BLOB"})
}

fn completed(output: &Value) -> Value {
    json!({"type": "response.completed", "response": {
        "id": "resp_1", "status": "completed", "model": UPSTREAM_MODEL, "output": output,
        "usage": {"input_tokens": 20, "output_tokens": 9}}})
}

fn warning_codes(events: &[StreamEvent]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::Warning { code, .. } => Some(code.as_str()),
            _ => None,
        })
        .collect()
}

/// One stream, the items its turn must carry, and whether a server's own text gave way.
type Case = (&'static str, Vec<Value>, Vec<Item>, bool);

/// Decodes each case and holds it to the port's rule: the literal items, and the warning exactly
/// when a server's text gave way.
fn hold_to_the_rule(cases: Vec<Case>) {
    let mut failures = Vec::new();
    for (case, payloads, items, replaced) in cases {
        let decoded = decode_stream(&binding(), &payloads);
        let outcome = decoded
            .result
            .unwrap_or_else(|error| panic!("{case}: {error:?}"));
        let codes = warning_codes(&decoded.events);
        let expected: &[&str] = if replaced { &[SHOWN_TEXT_KEPT] } else { &[] };
        if outcome.items != items || codes != expected {
            failures.push(format!(
                "{case}: carried {:?}, warnings {codes:?}; expected {items:?} with warnings \
                 {expected:?}",
                outcome.items,
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The terminal object's reasoning item as the decoder binds it.
fn reasoning() -> Item {
    Item::Opaque {
        provenance: provenance(),
        payload: reasoning_item(),
    }
}

/// Harness's own emulator gives a delta an `output_index` that is not the message's position in
/// the terminal `output`: `text_events("done", extra_output=[reasoning_item()])`
/// (`harness-responses/tests/fixtures/fake_responses.py:120`-`146`, used at `:701`) streams the
/// message at `output_index` 0 and completes with `output: [reasoning, message]`. Its ids agree,
/// so `id` pairs them. Take away either id, as Harness's pinned `openai-responses/2026-08-31.1`
/// wire does for messages, and the index rule ("different indexes ... never pair") splits one
/// message into two: the shown text is kept as a message of its own beside the server's, and the
/// turn carries the answer twice. The base (0d444964) and d52e6476 both carried it once.
#[test]
fn an_index_the_deltas_and_the_terminal_output_disagree_on_does_not_carry_the_answer_twice() {
    hold_to_the_rule(vec![
        (
            "the emulator's turn, the message naming no id",
            vec![
                created(),
                delta(Some("msg_b10x_001"), Some(0), "do"),
                delta(Some("msg_b10x_001"), Some(0), "ne"),
                done(Some(0), &message(None, "done")),
                completed(&json!([reasoning_item(), message(None, "done")])),
            ],
            vec![reasoning(), Item::assistant("done")],
            false,
        ),
        (
            "the emulator's turn, the deltas naming no item",
            vec![
                created(),
                delta(None, Some(0), "do"),
                delta(None, Some(0), "ne"),
                done(Some(0), &message(Some("msg_b10x_001"), "done")),
                completed(&json!([
                    reasoning_item(),
                    message(Some("msg_b10x_001"), "done")
                ])),
            ],
            vec![reasoning(), Item::assistant("done")],
            false,
        ),
    ]);
}

/// Harness's pinned `openai-responses/2026-08-31.1` `turn-stream.sse` names neither an
/// `item_id` nor an `output_index` on its text deltas, and no `id` on its message. With two
/// messages in that shape, every delta is one shown text (`AB`), and the order pass writes it
/// into the first message: the turn carries `AB` and `B` and warns about a server whose text
/// equalled its deltas. The correction's scenario covers only deltas that name an index.
#[test]
fn deltas_naming_neither_item_nor_index_do_not_put_two_messages_text_into_the_first() {
    hold_to_the_rule(vec![(
        "two messages in the shape of openai-responses/2026-08-31.1",
        vec![
            created(),
            json!({"type": "response.content_part.added"}),
            delta(None, None, "A"),
            json!({"type": "response.output_text.done"}),
            json!({"type": "response.content_part.done"}),
            done(None, &message(None, "A")),
            json!({"type": "response.content_part.added"}),
            delta(None, None, "B"),
            json!({"type": "response.output_text.done"}),
            json!({"type": "response.content_part.done"}),
            done(None, &message(None, "B")),
            completed(&json!([message(None, "A"), message(None, "B")])),
        ],
        vec![Item::assistant("A"), Item::assistant("B")],
        false,
    )]);
}

/// `docs/responses.md`: "a message no delta showed keeps the server's text, with no warning,
/// wherever it sits". Where the deltas name an item but no index and the messages name no id,
/// the order pass pairs the shown text with the first message, the one no delta showed: it is
/// overwritten, the answer is carried twice, and the caller is warned about a replacement.
#[test]
fn a_message_no_delta_showed_keeps_its_text_when_the_deltas_name_no_index() {
    hold_to_the_rule(vec![(
        "deltas naming an item and no index, messages naming no id",
        vec![
            created(),
            delta(Some("msg_2"), None, "answer"),
            completed(&json!([message(None, "preamble"), message(None, "answer")])),
        ],
        vec![Item::assistant("preamble"), Item::assistant("answer")],
        false,
    )]);
}

/// Controls for the correction's own claims: the streamed route pairs by the index
/// `response.output_item.done` named, two ids never pair, and two indexes never pair when the
/// terminal `output` is positionally consistent with them.
#[test]
fn the_correction_pairs_by_the_coordinates_it_names() {
    hold_to_the_rule(vec![
        (
            "streamed messages, deltas naming no item, an unshown message first",
            vec![
                created(),
                done(Some(0), &message(None, "preamble")),
                delta(None, Some(1), "answer"),
                done(Some(1), &message(None, "the final text")),
                completed(&json!([])),
            ],
            vec![Item::assistant("preamble"), Item::assistant("answer")],
            true,
        ),
        (
            "two different ids never pair",
            vec![
                created(),
                delta(Some("msg_2"), Some(1), "B"),
                completed(&json!([message(Some("msg_1"), "A")])),
            ],
            vec![Item::assistant("A"), Item::assistant("B")],
            false,
        ),
        (
            "two different indexes never pair",
            vec![
                created(),
                delta(None, Some(1), "B"),
                completed(&json!([message(None, "A")])),
            ],
            vec![Item::assistant("A"), Item::assistant("B")],
            false,
        ),
        (
            "the coordinate-less pinned shape with one message",
            vec![
                created(),
                delta(None, None, "Reading the readme."),
                done(None, &message(None, "Reading the readme.")),
                completed(&json!([
                    reasoning_item(),
                    message(None, "Reading the readme.")
                ])),
            ],
            vec![reasoning(), Item::assistant("Reading the readme.")],
            false,
        ),
    ]);
}

// ---------------------------------------------------------------------------------------------
// The live client: shown-text-kept after the turn's checks, before advisory-omitted.
// ---------------------------------------------------------------------------------------------

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
    let bound = BindingDocument::new(
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
            base_url: BaseUrl::new(url).expect("fixture URL"),
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
                output_streaming: true,
                input_during_output: false,
            },
        },
    )
    .bind()
    .expect("a valid fixture binding");
    let limit = Duration::from_secs(10);
    ResponsesClient::new(
        bound,
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

async fn accept(listener: &TcpListener) -> TcpStream {
    let (mut socket, _) = tokio::time::timeout(Duration::from_secs(10), listener.accept())
        .await
        .expect("the client never connected")
        .expect("an accepted connection");
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let length = socket.read(&mut buffer).await.expect("a readable socket");
        assert!(length > 0, "the client closed before sending a request");
        bytes.extend_from_slice(&buffer[..length]);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let length: usize = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .map_or(0, |value| value.trim().parse().expect("a numeric length"));
            if bytes.len() >= end + 4 + length {
                return socket;
            }
        }
    }
}

/// `docs/responses.md`: one `shown-text-kept` "once the turn has passed its checks against the
/// request, before any `advisory-omitted` warning and before the turn is returned", after every
/// delta.
#[tokio::test]
async fn the_client_says_its_shown_text_was_kept_after_every_delta_and_before_the_advisory() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
    let server = tokio::spawn(async move {
        let mut socket = accept(&listener).await;
        let mut body =
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
                .to_vec();
        for event in [
            created(),
            delta(Some("msg_1"), Some(0), "a dr"),
            delta(Some("msg_1"), Some(0), "aft"),
            completed(&json!([message(Some("msg_1"), "the final text")])),
        ] {
            let kind = event["type"].as_str().expect("an event type");
            body.extend_from_slice(format!("event: {kind}\ndata: {event}\n\n").as_bytes());
        }
        socket
            .write_all(&body)
            .await
            .expect("fixture bytes written");
        socket.flush().await.expect("fixture bytes flushed");
        let _ = socket.shutdown().await;
    });
    let mut request = TurnRequest::new("small", vec![Item::user("Say hello")]);
    request.controls.hints.unforwarded = [Unforwarded::ClientMetadata].into_iter().collect();
    let model = client(&url);
    let mut sink = VecSink::new(64, 64 * 1024);
    let outcome = model
        .turn(&request, &mut sink, &Cancel::new())
        .await
        .expect("a completed turn");
    server.await.expect("the fixture server");
    assert_eq!(outcome.items, vec![Item::assistant("a draft")]);
    let shape: Vec<String> = sink
        .events()
        .iter()
        .map(|event| match event {
            StreamEvent::TextDelta { text } => format!("text:{text}"),
            StreamEvent::Warning { code, .. } => format!("warning:{code}"),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(
        shape,
        [
            "text:a dr".to_owned(),
            "text:aft".to_owned(),
            format!("warning:{SHOWN_TEXT_KEPT}"),
            format!("warning:{ADVISORY_OMITTED}"),
        ]
    );
}
