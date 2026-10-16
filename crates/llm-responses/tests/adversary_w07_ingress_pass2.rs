//! Adversary cases for the Responses ingress encoder, pass 2 (wave 2026-10-16-w07).
//!
//! Each case drives the public encoder with what another of this repository's decoders hands a
//! gateway, and reads the answer back with this crate's own decoder on the same binding.

use llm_core::{
    CallId, Error, ErrorCode, Id, Item, Protocol, Provenance, StopReason, StreamEvent, ToolCall,
    ToolName, TurnObservation, TurnOutcome, Usage,
};
use llm_responses::{Binding, IngressStream, decode_stream, ingest_request};
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
            response_id: Some(id("resp_gateway_1")),
            usage: Some(Usage {
                input_tokens: Some(20),
                output_tokens: Some(9),
                ..Usage::default()
            }),
            final_usage: true,
        },
    }
}

fn client_body() -> Value {
    json!({
        "model": "example/Small-Model",
        "input": [{"type": "message", "role": "user",
                   "content": [{"type": "input_text", "text": "What is 2 + 2?"}]}],
        "tools": [
            {"type": "function", "name": "clock", "description": "Now.",
             "parameters": {"type": "object"}, "strict": false}
        ],
        "stream": true,
        "store": false,
        "include": ["reasoning.encrypted_content"]
    })
}

fn assert_valid_answer(encoded: &TurnOutcome) {
    let request = ingest_request(&binding(), &client_body()).expect("the client body ingests");
    encoded
        .validate_for(&request, &provenance())
        .expect("the neutral contract accepts this outcome");
}

fn text(value: &str) -> StreamEvent {
    StreamEvent::TextDelta {
        text: value.to_owned(),
    }
}

fn reasoning(value: &str) -> StreamEvent {
    StreamEvent::ReasoningDelta {
        text: value.to_owned(),
    }
}

/// Streams `events`, closes on `encoded`, decodes the payloads and holds them to the story's
/// streamed round trip: the same events and an equal turn.
fn assert_round_trip(why: &str, events: &[StreamEvent], encoded: &TurnOutcome) {
    assert_valid_answer(encoded);
    let mut stream = IngressStream::new("resp_gateway_1", CREATED_AT);
    let mut payloads = Vec::new();
    for event in events {
        payloads.extend(stream.chunk(event));
    }
    let closing = stream
        .close(encoded)
        .unwrap_or_else(|error| panic!("{why}: close refused a valid outcome: {error:?}"));
    payloads.extend(closing);
    let decoded = decode_stream(&binding(), &payloads);
    assert_eq!(decoded.events, events, "{why}: the events read back");
    assert_eq!(
        decoded.result.expect("the decoder accepts the encoding"),
        *encoded,
        "{why}: the turn read back"
    );
}

/// `fail` carries the counters of the error's observation. A failure raised mid-stream carries
/// the last snapshot with `final_usage: false` ("a partial snapshot, never a final cost",
/// `llm-core/src/turn.rs`). The Messages decoder attaches exactly that on a mid-stream `error`
/// event (`llm-messages/src/decode.rs` `attach` → `observation(target, false)`), holding the
/// counters `message_start` reported. A `response.failed` object's usage is terminal on this
/// wire, so this crate's decoder reads it back with `final_usage: true`: the partial count
/// becomes a final one, and the output the upstream really produced is reported as 1 token.
#[test]
fn fail_does_not_turn_a_partial_usage_snapshot_into_final_counters() {
    let partial = Usage {
        input_tokens: Some(25),
        output_tokens: Some(1),
        ..Usage::default()
    };
    let mut snapshot = TurnObservation::new(provenance());
    snapshot.upstream_model = Some(id("example/Small-Model"));
    snapshot.usage = Some(partial.clone());
    snapshot.final_usage = false;
    let error = Error::new(ErrorCode::Unavailable, "overloaded")
        .with_retriable(true)
        .with_observation(snapshot);

    let mut stream = IngressStream::new("resp_gateway_1", CREATED_AT);
    let mut payloads = stream.chunk(&text("The answer is"));
    payloads.extend(stream.fail(&error));

    let refusal = decode_stream(&binding(), &payloads)
        .result
        .expect_err("a failed response is a refusal");
    let observed = refusal
        .observation
        .as_deref()
        .expect("the failure carries evidence");
    assert!(
        !(observed.final_usage && observed.usage.is_some()),
        "the partial snapshot {partial:?} reads back as final counters: {observed:?}"
    );
}

const ONE_MESSAGE_INTERRUPTED_PENDING: &str = "story:responses-ingress-encoder: one message \
     interrupted by another item needs an item boundary in StreamEvent \
     (story:ingress-block-boundaries); assert the round trip when it exists";

/// Today's answer to one message whose deltas another item interrupts: text after another item
/// opens a new message item, because a Responses upstream's message, reasoning, message must stay
/// two messages, and `StreamEvent` cannot tell the two shapes apart. `close` refuses the outcome
/// that holds one message, the stream stays open, and `fail` ends it; the caller keeps what it
/// was shown.
fn assert_one_message_interrupted_is_refused(events: &[StreamEvent], encoded: &TurnOutcome) {
    assert_valid_answer(encoded);
    let mut stream = IngressStream::new("resp_gateway_1", CREATED_AT);
    let mut payloads = Vec::new();
    for event in events {
        payloads.extend(stream.chunk(event));
    }
    let refusal = stream
        .close(encoded)
        .expect_err(ONE_MESSAGE_INTERRUPTED_PENDING);
    assert_eq!(
        refusal.code,
        ErrorCode::Protocol,
        "{ONE_MESSAGE_INTERRUPTED_PENDING}"
    );
    assert_eq!(
        refusal.message, "model outcome contradicts what was already streamed",
        "{ONE_MESSAGE_INTERRUPTED_PENDING}"
    );
    payloads.extend(stream.fail(&refusal));
    let decoded = decode_stream(&binding(), &payloads);
    assert_eq!(decoded.events, events, "{ONE_MESSAGE_INTERRUPTED_PENDING}");
    assert!(decoded.result.is_err(), "{ONE_MESSAGE_INTERRUPTED_PENDING}");
}

/// The Chat decoder concatenates every `content` delta of a completion into one `AssistantText`
/// (`llm-chat/src/incoming.rs` `outcome`), and within one chunk relays `content` before
/// `reasoning_content` (`delta`). A chunk that carries the end of the reasoning and the start of
/// the answer therefore yields text, reasoning, then more text of the same message. Pinned to
/// today's refusal (module Limits).
#[test]
fn one_message_with_reasoning_between_its_deltas_reads_back_whole() {
    let events = vec![
        reasoning("Two and two"),
        text("The answer"),
        reasoning(" make four."),
        text(" is 4."),
    ];
    let encoded = outcome(
        StopReason::EndTurn,
        vec![Item::assistant("The answer is 4.")],
    );
    assert_one_message_interrupted_is_refused(&events, &encoded);
}

/// The same Chat shape with a call between two content deltas: the outcome holds one message and
/// the call. Pinned to today's refusal (module Limits).
#[test]
fn one_message_with_a_call_between_its_deltas_reads_back_whole() {
    let events = vec![
        text("Let me check"),
        StreamEvent::ToolCallStarted {
            call_id: call_id("call-1"),
            name: tool("clock"),
        },
        StreamEvent::ToolArgumentsDelta {
            call_id: call_id("call-1"),
            delta: "{}".to_owned(),
        },
        text(" the clock."),
    ];
    let encoded = outcome(
        StopReason::ToolCalls,
        vec![
            Item::assistant("Let me check the clock."),
            call("call-1", "clock", json!({})),
        ],
    );
    assert_one_message_interrupted_is_refused(&events, &encoded);
}

/// A call announced with no argument fragments: the Chat decoder reads an empty argument string as
/// `{}` (`llm-chat/src/incoming.rs` `parse_arguments`) after relaying only `ToolCallStarted`. The
/// story's round trip says the decoder gives back the events that were encoded.
#[test]
fn an_announced_call_without_argument_deltas_reads_back_as_the_events_encoded() {
    let events = vec![StreamEvent::ToolCallStarted {
        call_id: call_id("call-1"),
        name: tool("clock"),
    }];
    let encoded = outcome(
        StopReason::ToolCalls,
        vec![call("call-1", "clock", json!({}))],
    );
    assert_round_trip("announced call, no fragments", &events, &encoded);
}

/// `shown_messages` skips a streamed message that showed no text, because the decoder keeps no
/// empty shown text. The check against the outcome does not skip an empty carried message, so an
/// outcome with an empty message ahead of a shown one is refused as a contradiction although it
/// reads back unchanged. The Messages decoder yields `AssistantText("")` for an empty text block
/// and relays no delta for it (`llm-messages/src/codec.rs` `decode_block`,
/// `llm-messages/src/decode.rs` `text_delta`).
#[test]
fn an_empty_message_ahead_of_a_shown_one_is_not_a_contradiction() {
    let events = vec![
        StreamEvent::ToolCallStarted {
            call_id: call_id("call-1"),
            name: tool("clock"),
        },
        StreamEvent::ToolArgumentsDelta {
            call_id: call_id("call-1"),
            delta: "{}".to_owned(),
        },
        text("Checking."),
    ];
    let encoded = outcome(
        StopReason::ToolCalls,
        vec![
            Item::assistant(""),
            call("call-1", "clock", json!({})),
            Item::assistant("Checking."),
        ],
    );
    assert_round_trip("empty message, call, shown message", &events, &encoded);
}
