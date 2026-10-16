//! Adversary cases for the Responses ingress encoder (wave 2026-10-16-w07).
//!
//! Each case drives the public encoder with an outcome the neutral contract accepts
//! (`TurnOutcome::validate_for` against an ingested request) and holds it to what the story and the
//! module promise: the decoder reads back the events that were encoded, the wire shows each piece of
//! text once, and what the encoder emits the decoder accepts or the encoder refuses.

use std::collections::BTreeMap;

use llm_core::{
    CallId, ErrorCode, Id, Item, Protocol, Provenance, StopReason, StreamEvent, ToolCall, ToolName,
    TurnObservation, TurnOutcome, Usage,
};
use llm_responses::{
    Binding, IngressStream, decode_stream, encode_ingress_response, ingest_request,
};
use serde_json::{Value, json};

const CREATED_AT: u64 = 1_772_000_000;

fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
}

fn provenance() -> Provenance {
    Provenance {
        protocol: Protocol::Responses,
        provider: id("my-lab"),
        account: id("local"),
        endpoint: id("local-vllm"),
        model: id("small"),
        binding_revision: id("rev-1"),
    }
}

fn binding() -> Binding {
    Binding::new(provenance(), id("example/Small-Model"))
}

fn call_id(value: &str) -> CallId {
    CallId::new(value).expect("fixture call id")
}

fn tool(value: &str) -> ToolName {
    ToolName::new(value).expect("fixture tool name")
}

fn call(call: &str, name: &str, arguments: Value) -> Item {
    Item::ToolCall(ToolCall {
        call_id: call_id(call),
        name: tool(name),
        arguments,
    })
}

fn outcome(stop_reason: StopReason, items: Vec<Item>) -> TurnOutcome {
    TurnOutcome {
        stop_reason,
        items,
        observation: TurnObservation {
            binding: provenance(),
            upstream_model: Some(id("example/Small-Model")),
            // The caller's id: an answer carries it on every payload and never relays the
            // upstream's, so the decoder reads it back as the response id (decision, round 1).
            response_id: Some(id("resp_gateway_1")),
            usage: Some(Usage {
                input_tokens: Some(20),
                output_tokens: Some(9),
                cached_input_tokens: None,
                cache_creation_input_tokens: None,
                reasoning_output_tokens: None,
            }),
            final_usage: true,
        },
    }
}

fn client_body() -> Value {
    json!({
        "model": "example/Small-Model",
        "input": [{"type": "message", "role": "user",
                   "content": [{"type": "input_text", "text": "What time is it?"}]}],
        "tools": [
            {"type": "function", "name": "clock", "description": "Now.",
             "parameters": {"type": "object"}, "strict": false}
        ],
        "stream": true,
        "store": false,
        "include": ["reasoning.encrypted_content"]
    })
}

/// The outcome is a valid answer to the request a gateway ingested, so a real caller can hold it.
fn assert_valid_answer(encoded: &TurnOutcome) {
    let request = ingest_request(&binding(), &client_body()).expect("the client body ingests");
    encoded
        .validate_for(&request, &provenance())
        .expect("the neutral contract accepts this outcome");
}

fn encode(events: &[StreamEvent], outcome: &TurnOutcome) -> Vec<Value> {
    let mut stream = IngressStream::new("resp_gateway_1", CREATED_AT);
    let mut payloads = Vec::new();
    for event in events {
        payloads.extend(stream.chunk(event));
    }
    payloads.extend(stream.close(outcome).expect("the outcome encodes"));
    payloads
}

fn text(value: &str) -> StreamEvent {
    StreamEvent::TextDelta {
        text: value.to_owned(),
    }
}

/// Text, a call, then more text: the shape a Messages upstream decodes `text`, `tool_use`, `text`
/// content blocks into (one `AssistantText` per block, `llm-messages/src/codec.rs`).
fn text_call_text() -> (Vec<StreamEvent>, TurnOutcome) {
    let events = vec![
        text("First."),
        StreamEvent::ToolCallStarted {
            call_id: call_id("call-1"),
            name: tool("clock"),
        },
        StreamEvent::ToolArgumentsDelta {
            call_id: call_id("call-1"),
            delta: "{}".to_owned(),
        },
        text("Second."),
    ];
    let encoded = outcome(
        StopReason::ToolCalls,
        vec![
            Item::assistant("First."),
            call("call-1", "clock", json!({})),
            Item::assistant("Second."),
        ],
    );
    (events, encoded)
}

/// Two messages in one turn with no call between them: the shape a Responses upstream with two
/// `message` output items decodes into.
fn two_messages() -> (Vec<StreamEvent>, TurnOutcome) {
    let events = vec![text("Alpha."), text("Beta.")];
    let encoded = outcome(
        StopReason::EndTurn,
        vec![Item::assistant("Alpha."), Item::assistant("Beta.")],
    );
    (events, encoded)
}

/// The open defect, asserted as it stands so it cannot change unnoticed.
const TWO_MESSAGES_PENDING: &str = "story:responses-ingress-encoder: two consecutive messages need \
     a message boundary in StreamEvent (story:ingress-block-boundaries); assert the round trip \
     when it exists";

/// `two_messages` today: the neutral events carry no boundary between the two texts, so both
/// deltas went out under one message item, and `close` refuses an outcome that splits them. The
/// stream stays open and ends with `fail`; the caller keeps the text it was shown.
fn assert_two_messages_are_refused_until_a_boundary_exists() {
    let (events, encoded) = two_messages();
    assert_valid_answer(&encoded);
    let mut stream = IngressStream::new("resp_gateway_1", CREATED_AT);
    let mut payloads = Vec::new();
    for event in &events {
        payloads.extend(stream.chunk(event));
    }
    let refusal = stream.close(&encoded).expect_err(TWO_MESSAGES_PENDING);
    assert_eq!(refusal.code, ErrorCode::Protocol, "{TWO_MESSAGES_PENDING}");
    assert_eq!(
        refusal.message, "model outcome contradicts what was already streamed",
        "{TWO_MESSAGES_PENDING}"
    );
    payloads.extend(stream.fail(&refusal));
    assert_eq!(
        payloads.last().map(|payload| &payload["type"]),
        Some(&json!("response.failed")),
        "{TWO_MESSAGES_PENDING}"
    );
    let decoded = decode_stream(&binding(), &payloads);
    assert_eq!(decoded.events, events, "{TWO_MESSAGES_PENDING}");
    assert!(decoded.result.is_err(), "{TWO_MESSAGES_PENDING}");
}

/// Story Acceptance, "Round trip, streamed": the decoder gives back the same events. A turn with
/// more than one message must not show the caller any text twice.
#[test]
fn a_turn_with_several_messages_reads_back_as_the_events_that_were_encoded() {
    assert_two_messages_are_refused_until_a_boundary_exists();
    let (events, encoded) = text_call_text();
    assert_valid_answer(&encoded);
    let payloads = encode(&events, &encoded);
    let decoded = decode_stream(&binding(), &payloads);
    assert_eq!(
        decoded.events, events,
        "the caller is shown the encoded events, each text once"
    );
    assert_eq!(decoded.result.expect("decodes"), encoded);
}

/// On this wire the deltas of one output item add up to the text its `output_text.done` and its
/// finished item report. A client that renders deltas and one that renders the finished item
/// must show the same message.
#[test]
fn every_streamed_message_finishes_with_the_text_its_deltas_showed() {
    assert_two_messages_are_refused_until_a_boundary_exists();
    let (events, encoded) = text_call_text();
    let payloads = encode(&events, &encoded);
    let mut deltas: BTreeMap<String, String> = BTreeMap::new();
    let mut done: BTreeMap<String, String> = BTreeMap::new();
    for payload in &payloads {
        let item_id = payload["item_id"].as_str().unwrap_or_default().to_owned();
        match payload["type"].as_str() {
            Some("response.output_text.delta") => deltas
                .entry(item_id)
                .or_default()
                .push_str(payload["delta"].as_str().expect("delta text")),
            Some("response.output_text.done") => {
                done.insert(
                    item_id,
                    payload["text"].as_str().expect("done text").to_owned(),
                );
            }
            _ => {}
        }
    }
    assert_eq!(
        deltas, done,
        "per message item: streamed deltas versus the finished text"
    );
}

/// The response a streamed answer opens is the response it ends: one `id` from
/// `response.created` to the terminal object.
#[test]
fn a_streamed_response_keeps_one_id_from_created_to_completed() {
    let encoded = outcome(StopReason::EndTurn, vec![Item::assistant("Hi")]);
    let payloads = encode(&[text("Hi")], &encoded);
    let created = payloads
        .iter()
        .find(|payload| payload["type"] == "response.created")
        .expect("the response opens");
    let completed = payloads.last().expect("a terminal payload");
    assert_eq!(completed["type"], "response.completed");
    assert_eq!(
        created["response"]["id"], completed["response"]["id"],
        "the streamed response changes identity between its first and its last payload"
    );
}

/// Module contract (`ingress.rs:5-9`): the encoder emits only what the decoder interprets, and
/// refuses where that is impossible. A call whose arguments are valid JSON but not an object is
/// valid neutral output (`TurnOutcome::validate_for` accepts it, and the Chat decoder produces it:
/// `llm-chat/src/incoming.rs` `parse_arguments` reads any JSON value). The Responses decoder refuses
/// such arguments, so the encoder must refuse them rather than answer with them.
#[test]
fn arguments_the_decoder_refuses_are_refused_by_the_encoder_not_emitted() {
    for arguments in [json!([1]), json!("noon"), json!(null), json!(5)] {
        let encoded = outcome(
            StopReason::ToolCalls,
            vec![call("call-1", "clock", arguments.clone())],
        );
        assert_valid_answer(&encoded);

        match encode_ingress_response(&encoded, "resp_gateway_1", CREATED_AT) {
            Err(_) => {}
            Ok(body) => {
                let decoded = decode_stream(
                    &binding(),
                    &[json!({"type": "response.completed", "response": body})],
                );
                assert!(
                    decoded.result.is_ok(),
                    "non-streamed {arguments}: the encoder answered, the decoder refused: {:?}",
                    decoded.result.err()
                );
            }
        }

        let mut stream = IngressStream::new("resp_gateway_1", CREATED_AT);
        if let Ok(payloads) = stream.close(&encoded) {
            let decoded = decode_stream(&binding(), &payloads);
            assert!(
                decoded.result.is_ok(),
                "streamed {arguments}: the encoder answered, the decoder refused: {:?}",
                decoded.result.err()
            );
        }
    }
}
