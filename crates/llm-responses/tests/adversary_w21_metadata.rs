//! Adversary pass on story:codex-stream-metadata-is-not-replayed (wave 2026-10-29-w21).
//!
//! `response.metadata` joined `ACCEPTED_STREAM_EVENTS`: it is ignored, and the doc row says "no
//! outcome carries it and no next request replays it". These cases drive the decoder with the
//! event at every place a stream can hold it, with odd payloads, against the recorded live Codex
//! stream and the scenario document the unit wrote, and check the replay claim against a
//! conversation that already carries the item. No provider is contacted.

use llm_core::{ErrorCode, Id, Item, Protocol, Provenance, StopReason, StreamEvent, TurnRequest};
use llm_responses::{Binding, decode_stream, ingest_request, project_request};
use serde_json::{Value, json};

const RECORDED: &str = include_str!("fixtures/live/codex-text-turn.sse");
const SCENARIO: &str = include_str!(
    "../../../contracts/responses/scenarios/a-metadata-event-is-not-conversation-state.yaml"
);

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

fn metadata(payload: &Value) -> Value {
    let mut event = json!({"type": "response.metadata"});
    if let (Some(event), Some(extra)) = (event.as_object_mut(), payload.as_object()) {
        for (key, value) in extra {
            event.insert(key.clone(), value.clone());
        }
    }
    event
}

/// A Codex-shaped turn: a reasoning item, a streamed message, a streamed function call, and a
/// terminal object with an empty `output`, so the streamed items are the turn.
fn base_stream() -> Vec<Value> {
    vec![
        json!({"type": "response.created", "response": {"id": "resp_1", "status": "in_progress"}}),
        json!({"type": "response.in_progress", "response": {"id": "resp_1"}}),
        json!({"type": "response.output_item.added", "output_index": 0,
               "item": {"type": "reasoning", "id": "rs_1"}}),
        json!({"type": "response.output_item.done", "output_index": 0,
               "item": {"type": "reasoning", "id": "rs_1", "summary": []}}),
        json!({"type": "response.output_item.added", "output_index": 1,
               "item": {"type": "message", "id": "msg_1", "role": "assistant", "content": []}}),
        json!({"type": "response.output_text.delta", "output_index": 1, "item_id": "msg_1",
               "delta": "Hel"}),
        json!({"type": "response.output_text.delta", "output_index": 1, "item_id": "msg_1",
               "delta": "lo"}),
        json!({"type": "response.output_text.done", "output_index": 1, "item_id": "msg_1",
               "text": "Hello"}),
        json!({"type": "response.output_item.done", "output_index": 1,
               "item": {"type": "message", "id": "msg_1", "role": "assistant",
                        "content": [{"type": "output_text", "text": "Hello"}]}}),
        json!({"type": "response.output_item.added", "output_index": 2,
               "item": {"type": "function_call", "id": "fc_1", "call_id": "call_1",
                        "name": "pick", "arguments": ""}}),
        json!({"type": "response.function_call_arguments.delta", "output_index": 2,
               "item_id": "fc_1", "delta": "{\"a\":1}"}),
        json!({"type": "response.output_item.done", "output_index": 2,
               "item": {"type": "function_call", "id": "fc_1", "call_id": "call_1",
                        "name": "pick", "arguments": "{\"a\":1}"}}),
        json!({"type": "response.completed", "response": {
            "id": "resp_1", "model": "example/Small-Model", "status": "completed", "output": [],
            "usage": {"input_tokens": 10, "output_tokens": 4}}}),
    ]
}

fn opaque_type(item: &Item) -> Option<&str> {
    match item {
        Item::Opaque { payload, .. } => payload.get("type").and_then(Value::as_str),
        _ => None,
    }
}

fn warnings(events: &[StreamEvent]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::Warning { code, .. } => Some(code.as_str()),
            _ => None,
        })
        .collect()
}

/// The recorded live Codex text turn (2026-10-21) streams `response.metadata` between two text
/// deltas of the answer. Decoded after this change, the turn is the reasoning item and the
/// answer, with no metadata item and no warning, and the answer is exactly the shown text.
#[test]
fn the_recorded_codex_text_turn_decodes_to_reasoning_and_answer_without_a_metadata_item() {
    let payloads: Vec<Value> = RECORDED
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .map(|data| serde_json::from_str(data).expect("a recorded JSON payload"))
        .collect();
    assert!(
        payloads
            .iter()
            .any(|payload| payload["type"] == "response.metadata"),
        "the recording carries the event"
    );
    let decoding = decode_stream(&binding(), &payloads);
    let shown: String = decoding
        .events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        warnings(&decoding.events).is_empty(),
        "{:?}",
        decoding.events
    );
    let outcome = decoding.result.expect("the recorded turn decodes");
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
    assert_eq!(outcome.items.len(), 2, "{:?}", outcome.items);
    assert_eq!(opaque_type(&outcome.items[0]), Some("reasoning"));
    assert_eq!(outcome.items[1], Item::assistant(shown));
}

/// The scenario document the unit wrote, driven against the decoder: its own `stream_json`
/// decodes to no events, no items and `end-turn`.
#[test]
fn the_scenario_documents_stream_decodes_to_what_the_scenario_asserts() {
    let line = SCENARIO
        .lines()
        .find_map(|line| line.trim().strip_prefix("stream_json: "))
        .expect("the scenario names a stream");
    let quoted = line
        .strip_prefix('\'')
        .and_then(|line| line.strip_suffix('\''))
        .expect("a single-quoted YAML scalar");
    let payloads: Vec<Value> =
        serde_json::from_str(&quoted.replace("''", "'")).expect("a JSON stream");
    let decoding = decode_stream(&binding(), &payloads);
    assert!(decoding.events.is_empty(), "{:?}", decoding.events);
    let outcome = decoding.result.expect("the scenario's stream decodes");
    assert!(outcome.items.is_empty(), "{:?}", outcome.items);
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
}

/// The event at every place before the terminal object, once or repeated, with payloads that
/// look like other events (an item, a delta, a terminal-shaped response, an error code), changes
/// nothing: the events the caller is shown and the turn it gets are those of the same stream
/// without it.
#[test]
fn a_metadata_event_anywhere_with_any_payload_changes_neither_events_nor_turn() {
    let base = base_stream();
    let reference = decode_stream(&binding(), &base);
    let reference_outcome = reference.result.as_ref().expect("the base stream decodes");
    assert_eq!(reference_outcome.stop_reason, StopReason::ToolCalls);
    assert_eq!(
        reference_outcome.items.len(),
        3,
        "{:?}",
        reference_outcome.items
    );

    let payloads = [
        json!({}),
        json!({"metadata": null}),
        json!({"metadata": "text"}),
        json!({"metadata": [1, 2, 3]}),
        json!({"metadata": {"openai_chatgpt_moderation_metadata": {"is_blocked": true}}}),
        json!({"sequence_number": 30, "response_id": "resp_other"}),
        json!({"delta": "SHOULD-NOT-BE-SHOWN", "output_index": 1, "item_id": "msg_1"}),
        json!({"item": {"type": "function_call", "id": "fc_9", "call_id": "call_9",
                        "name": "other", "arguments": "{}"}}),
        json!({"response": {"status": "failed", "error": {"code": "server_error"}}}),
        json!({"code": "server_is_overloaded", "error": {"code": "insufficient_quota"}}),
        json!({"response": {"id": "resp_x", "status": "completed",
                            "output": [{"type": "message", "role": "assistant",
                                        "content": [{"type": "output_text", "text": "X"}]}],
                            "usage": {"input_tokens": 999, "output_tokens": 999}}}),
    ];
    let terminal = base.len() - 1;
    for payload in &payloads {
        let event = metadata(payload);
        for at in 0..=terminal {
            for repeat in [1, 3] {
                let mut stream = base.clone();
                for _ in 0..repeat {
                    stream.insert(at, event.clone());
                }
                let decoding = decode_stream(&binding(), &stream);
                assert_eq!(
                    decoding.events, reference.events,
                    "events differ with {event} x{repeat} at {at}"
                );
                let outcome = decoding
                    .result
                    .unwrap_or_else(|error| panic!("{event} x{repeat} at {at}: {error:?}"));
                assert_eq!(
                    outcome.items, reference_outcome.items,
                    "{event} x{repeat} at {at}"
                );
                assert_eq!(outcome.stop_reason, reference_outcome.stop_reason);
                assert_eq!(outcome.observation, reference_outcome.observation);
            }
        }
    }
}

/// After the terminal object nothing is read, so a trailing metadata event is no item either.
#[test]
fn a_metadata_event_after_the_terminal_object_is_not_read() {
    let mut stream = base_stream();
    stream.push(metadata(&json!({"metadata": {"late": true}})));
    stream.push(json!({"type": "response.not_in_the_pin"}));
    let decoding = decode_stream(&binding(), &stream);
    assert!(
        warnings(&decoding.events).is_empty(),
        "{:?}",
        decoding.events
    );
    let outcome = decoding.result.expect("decodes");
    assert_eq!(outcome.items.len(), 3, "{:?}", outcome.items);
    assert!(
        outcome
            .items
            .iter()
            .all(|item| opaque_type(item) != Some("response.metadata"))
    );
}

/// A metadata event whose payload is shaped like a terminal response does not end the turn: a
/// stream that stops after it has reached no terminal state.
#[test]
fn a_terminal_shaped_metadata_event_does_not_end_the_turn() {
    let stream = vec![
        json!({"type": "response.created", "response": {"id": "resp_1"}}),
        metadata(&json!({"response": {"id": "resp_1", "status": "completed", "output": []}})),
    ];
    let error = decode_stream(&binding(), &stream)
        .result
        .expect_err("no terminal object arrived");
    assert_eq!(error.code, ErrorCode::Protocol);
}

/// Pins the retry rule the story kept: the event is not a lifecycle event, so a transient failure
/// after it is final, as after any other non-lifecycle payload. (The doc row calls the event
/// "not output"; retry eligibility still treats it as output.)
#[test]
fn an_overload_after_only_a_metadata_event_is_final_as_before_the_change() {
    for prelude in [
        metadata(&json!({"metadata": {}})),
        json!({"type": "response.not_in_the_pin"}),
    ] {
        let stream = [
            json!({"type": "response.created"}),
            prelude.clone(),
            json!({"type": "error", "code": "server_is_overloaded"}),
        ];
        let error = decode_stream(&binding(), &stream)
            .result
            .expect_err("an overload is a failed attempt");
        assert_eq!(error.code, ErrorCode::Unavailable);
        assert!(!error.may_retry(), "{prelude}: {error:?}");
    }
    let lifecycle_only = [
        json!({"type": "response.created"}),
        json!({"type": "error", "code": "server_is_overloaded"}),
    ];
    let error = decode_stream(&binding(), &lifecycle_only)
        .result
        .expect_err("an overload is a failed attempt");
    assert!(error.may_retry(), "{error:?}");
}

/// The added doc row says "no next request replays it". A conversation a caller kept from a turn
/// decoded before this change (llm 0.16.0 and earlier, e.g. a Harness session saved to disk and
/// continued with `--resume`) carries the event as an opaque item of this binding. The next
/// request built from it still sends `response.metadata` as an input entry, the one the endpoint
/// refused with HTTP 400 "Invalid value: 'response.metadata'". story:saved-codex-metadata-item-on-resume
/// (D208) decided it: the egress leaves the item out and sends the rest of the conversation.
#[test]
fn a_metadata_item_kept_from_an_earlier_turn_is_not_replayed_as_an_input_entry() {
    let kept = Item::Opaque {
        provenance: provenance(),
        payload: json!({"type": "response.metadata", "sequence_number": 30,
                        "metadata": {"fixture": "synthetic"}}),
    };
    let request = TurnRequest::new(
        "small",
        vec![
            Item::user("first question"),
            Item::assistant("first answer"),
            kept,
            Item::user("second question"),
        ],
    );
    let body = project_request(&binding(), &request)
        .expect("story:saved-codex-metadata-item-on-resume: a saved opaque item still projects");
    let types: Vec<&str> = body["input"]
        .as_array()
        .expect("an input list")
        .iter()
        .filter_map(|entry| entry["type"].as_str())
        .collect();
    assert!(
        !types.contains(&"response.metadata"),
        "story:saved-codex-metadata-item-on-resume: a saved metadata item is not sent: {types:?}"
    );
    assert_eq!(
        types,
        ["message", "message", "message"],
        "story:saved-codex-metadata-item-on-resume: every other item is sent"
    );
}

/// The gateway direction: a client that replays such an entry to an ingress decoder is refused
/// before anything is sent upstream, not carried.
#[test]
fn ingress_refuses_a_replayed_metadata_entry() {
    let request = TurnRequest::new("small", vec![Item::user("hi")]);
    let mut body = project_request(&binding(), &request).expect("projects");
    ingest_request(&binding(), &body).expect("the body without the entry ingests");
    body["input"]
        .as_array_mut()
        .expect("an input list")
        .push(json!({"type": "response.metadata", "metadata": {"fixture": "synthetic"}}));
    ingest_request(&binding(), &body).expect_err("an entry outside the subset is refused");
}
