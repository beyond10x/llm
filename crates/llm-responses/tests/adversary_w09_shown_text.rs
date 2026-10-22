//! Adversarial cases for "every event is final" on Responses (`story:text-deltas-are-final`).
//!
//! The claim under attack is the one `StreamEvent`'s documentation now makes
//! (`crates/llm-core/src/port.rs`): on a successful turn each assistant message carries exactly
//! the text deltas shown for it, a message no delta was shown for carries the text the server
//! sent, and `shown-text-kept` says so exactly when a server's text was replaced, before the
//! turn is returned. `spec/domains/responses.yaml` "Kept text" adds the pairing rule: by `id`
//! where both sides name one, otherwise by order. No provider is contacted.

use llm_core::{
    AuthKind, BillingKind, BoxFuture, Cancel, Capabilities, Id, Item, Model, Protocol, Provenance,
    StreamEvent, ToolChoice, ToolName, ToolSpec, TurnRequest, VecSink,
};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_http::{HttpClient, Limits};
use llm_providers::{
    Account, BaseUrl, BindingDocument, Endpoint, Provider, ServedModel, ServingModel,
};
use llm_responses::{Binding, IngressStream, ResponsesClient, decode_stream};
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

/// A text delta naming `item_id`, or naming no item when it is `None`.
fn delta(item_id: Option<&str>, output_index: u64, text: &str) -> Value {
    let mut event = json!({"type": "response.output_text.delta",
        "output_index": output_index, "content_index": 0, "delta": text});
    if let Some(item_id) = item_id {
        event["item_id"] = json!(item_id);
    }
    event
}

fn message(id: Option<&str>, text: &str) -> Value {
    let mut message = json!({"type": "message", "role": "assistant",
        "content": [{"type": "output_text", "text": text}]});
    if let Some(id) = id {
        message["id"] = json!(id);
    }
    message
}

fn completed(output: &Value) -> Value {
    json!({"type": "response.completed", "response": {
        "id": "resp_1", "status": "completed", "model": UPSTREAM_MODEL, "output": output,
        "usage": {"input_tokens": 20, "output_tokens": 9}}})
}

fn shown_text(events: &[StreamEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
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
                "{case}: shown {:?}, carried {:?}, warnings {codes:?}; expected {items:?} with \
                 warnings {expected:?}",
                shown_text(&decoded.events),
                outcome.items,
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Port rule: a message carries "exactly the concatenation of the `TextDelta`s shown for it".
/// Spec "Kept text": shown text is one output item per `item_id`, so every delta that names no
/// item is joined into one shown text, and the pairing pass then writes that whole text into the
/// first message. Two messages shown as `A` then `B` come back as `AB` and `B`: the turn carries
/// `ABB`, text the caller was never shown, and a `shown-text-kept` warning for a server whose
/// text equalled its deltas.
#[test]
fn deltas_that_name_no_item_do_not_put_two_messages_text_into_the_first() {
    hold_to_the_rule(vec![
        (
            "id-less deltas, id-less terminal messages",
            vec![
                created(),
                delta(None, 0, "A"),
                delta(None, 1, "B"),
                completed(&json!([message(None, "A"), message(None, "B")])),
            ],
            vec![Item::assistant("A"), Item::assistant("B")],
            false,
        ),
        (
            "id-less deltas, terminal messages with ids",
            vec![
                created(),
                delta(None, 0, "A"),
                delta(None, 1, "B"),
                completed(&json!([
                    message(Some("msg_a"), "A"),
                    message(Some("msg_b"), "B")
                ])),
            ],
            vec![Item::assistant("A"), Item::assistant("B")],
            false,
        ),
    ]);
}

/// Port rule: "a message no delta was shown for carries the text the server sent". Pairing by
/// order takes the first unpaired message, so when a message no delta showed comes before the
/// shown one and ids cannot pair them, the unshown message is overwritten with the shown text
/// and the shown one keeps its own: the turn carries the answer twice, loses the first message,
/// and warns about a replacement that did not happen to the message that was shown. The first
/// case is the shape the pinned scenario
/// `kept-text-a-terminal-message-gives-way-to-the-shown-deltas` declares a server can send
/// (deltas naming an item, terminal messages naming none). Each delta carries the
/// `output_index` that would place it.
#[test]
fn a_message_no_delta_showed_keeps_its_own_text_when_ids_cannot_pair() {
    hold_to_the_rule(vec![
        (
            "deltas naming an item, terminal messages naming none",
            vec![
                created(),
                delta(Some("msg_2"), 1, "answer"),
                completed(&json!([message(None, "preamble"), message(None, "answer")])),
            ],
            vec![Item::assistant("preamble"), Item::assistant("answer")],
            false,
        ),
        (
            "deltas naming no item, terminal messages with ids",
            vec![
                created(),
                delta(None, 1, "answer"),
                completed(&json!([
                    message(Some("msg_1"), "preamble"),
                    message(Some("msg_2"), "answer")
                ])),
            ],
            vec![Item::assistant("preamble"), Item::assistant("answer")],
            false,
        ),
    ]);
}

/// Controls for the two cases above: where ids pair, an unshown message before a shown one
/// keeps its text, a rewritten shown one takes its deltas with one warning, and text split
/// across deltas at multi-byte and combining-mark boundaries is compared byte for byte.
#[test]
fn where_ids_pair_each_message_keeps_exactly_what_was_shown_for_it() {
    hold_to_the_rule(vec![
        (
            "an unshown message before a shown one, ids pairing",
            vec![
                created(),
                delta(Some("msg_2"), 1, "answer"),
                completed(&json!([
                    message(Some("msg_1"), "preamble"),
                    message(Some("msg_2"), "answer")
                ])),
            ],
            vec![Item::assistant("preamble"), Item::assistant("answer")],
            false,
        ),
        (
            "an unshown message before a rewritten shown one",
            vec![
                created(),
                delta(Some("msg_2"), 1, "answer"),
                completed(&json!([
                    message(Some("msg_1"), "preamble"),
                    message(Some("msg_2"), "the final text")
                ])),
            ],
            vec![Item::assistant("preamble"), Item::assistant("answer")],
            true,
        ),
        (
            "a message no delta showed beside shown text naming another item",
            vec![
                created(),
                delta(Some("msg_2"), 1, "B"),
                completed(&json!([message(Some("msg_1"), "A")])),
            ],
            vec![Item::assistant("A"), Item::assistant("B")],
            false,
        ),
        (
            "a reasoning delta between two deltas of one message",
            vec![
                created(),
                delta(Some("msg_1"), 0, "Let me "),
                json!({"type": "response.reasoning_summary_text.delta", "item_id": "rs_1",
                    "output_index": 1, "summary_index": 0, "delta": "weighing"}),
                delta(Some("msg_1"), 0, "check"),
                completed(&json!([message(Some("msg_1"), "Let me check")])),
            ],
            vec![Item::assistant("Let me check")],
            false,
        ),
        (
            "multi-byte text split across deltas, server text equal",
            vec![
                created(),
                delta(Some("msg_1"), 0, "Gr\u{fc}"),
                delta(Some("msg_1"), 0, "\u{df}e \u{1f600}"),
                completed(&json!([message(
                    Some("msg_1"),
                    "Gr\u{fc}\u{df}e \u{1f600}"
                )])),
            ],
            vec![Item::assistant("Gr\u{fc}\u{df}e \u{1f600}")],
            false,
        ),
        (
            "the same text, decomposed by the server",
            vec![
                created(),
                delta(Some("msg_1"), 0, "Gr\u{fc}\u{df}e"),
                completed(&json!([message(Some("msg_1"), "Gru\u{308}\u{df}e")])),
            ],
            vec![Item::assistant("Gr\u{fc}\u{df}e")],
            true,
        ),
    ]);
}

/// Design decision 2.4: "feeding the same events to the protocol's ingress encoder,
/// `close(outcome)` succeeds". The unit's own relay case puts a reasoning delta between its two
/// messages. Two messages shown back to back, with nothing between them, decode to the turn the
/// port rule asks for, and the relay of those events refuses it: the neutral events carry no
/// message boundary, so the encoder streamed one message `AB` and the turn carries `A` and `B`.
///
/// That refusal is the encoder's, it predates this unit, and `story:ingress-block-boundaries`
/// owns it (correction round 1, F3). This case pins today's refusal so it cannot change
/// unnoticed; it flips to asserting that `close` succeeds when that story lands. Formerly
/// `two_messages_shown_back_to_back_relay_and_close_on_the_turn`.
#[test]
fn two_messages_shown_back_to_back_are_refused_by_the_relay_until_ingress_block_boundaries() {
    let payloads = vec![
        created(),
        delta(Some("msg_a"), 0, "A"),
        delta(Some("msg_b"), 1, "B"),
        completed(&json!([
            message(Some("msg_a"), "A"),
            message(Some("msg_b"), "B")
        ])),
    ];
    let decoded = decode_stream(&binding(), &payloads);
    let outcome = decoded.result.expect("a completed turn");
    assert_eq!(
        outcome.items,
        vec![Item::assistant("A"), Item::assistant("B")]
    );
    assert_eq!(warning_codes(&decoded.events), [] as [&str; 0]);
    let mut relay = IngressStream::new("resp_gateway_1", 1_700_000_000);
    for event in &decoded.events {
        relay.chunk(event);
    }
    let refused = relay.close(&outcome).expect_err(
        "the relay closed on two back-to-back messages: story:ingress-block-boundaries has \
         landed, so flip this case to assert that close succeeds",
    );
    assert_eq!(
        refused.message, "model outcome contradicts what was already streamed",
        "today's refusal changed before story:ingress-block-boundaries landed: {refused:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// The live client: the warning is for a turn that is returned.
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

/// `stream.rs` says of the warning: "Once, and only for a turn that is returned: the caller is
/// told before it holds the turn that the text it carries is the shown text". The client hands
/// the decoder's events to the sink and only then checks the turn against the request, as the
/// advisory warning beside it is careful not to (`client.rs`: "Once, after the turn succeeded
/// and never before"). A request that forces a tool, answered with rewritten text and no call,
/// is refused, and the caller has already been told that a turn it never receives kept its
/// shown text.
#[tokio::test]
async fn a_refused_turn_does_not_tell_the_caller_its_shown_text_was_kept() {
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
            delta(Some("msg_1"), 0, "a draft"),
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
    let mut request = TurnRequest::new("small", vec![Item::user("Pick the protocol")]);
    request.tools = vec![ToolSpec {
        name: ToolName::new("pick_protocol").expect("tool name"),
        description: "A fixture tool.".to_owned(),
        input_schema: json!({"type": "object"}),
    }];
    request.tool_choice = ToolChoice::Required;
    let model = client(&url);
    let mut sink = VecSink::new(64, 64 * 1024);
    let error = model
        .turn(&request, &mut sink, &Cancel::new())
        .await
        .expect_err("a forced tool that was never called is refused");
    server.await.expect("the fixture server");
    assert_eq!(sink.text(), "a draft", "{error:?}");
    assert!(
        !warning_codes(sink.events()).contains(&SHOWN_TEXT_KEPT),
        "the turn was refused ({}), and the caller was told its shown text was kept: {:?}",
        error.message,
        sink.events()
    );
}
