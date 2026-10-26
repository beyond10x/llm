//! A streamed usage `cost` is carried to the caller as a recorded extension, not refused.
//!
//! story:messages-usage-cost. A route reported `message_delta.usage.cost` on a live streamed turn
//! (`Messages field is outside the declared subset: message_delta.usage.cost`), after the caller
//! had already been shown the answer. The shape of the value is unknown, so it is admitted in any
//! JSON shape on the two usage objects a stream carries and reported verbatim as a
//! `usage-extension` warning. It reads into nothing: the turn has the outcome it has without it.
mod support;

use llm_core::{
    Cancel, Error, ErrorCode, Item, StreamEvent, TurnOutcome, TurnRequest, Usage, VecSink,
};
use llm_http::SseEvent;
use llm_messages::{StreamDecoder, decode_message, decode_stream};
use serde_json::{Value, json};
use support::binding;

const CODE: &str = "usage-extension";

fn request() -> TurnRequest {
    TurnRequest::new("internal-model", vec![Item::user("Summarise the log")])
}

fn sse(events: &[Value]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for event in events {
        let name = event["type"].as_str().expect("event type");
        bytes.extend_from_slice(format!("event: {name}\ndata: {event}\n\n").as_bytes());
    }
    bytes
}

fn message_start(usage: Value) -> Value {
    let mut event = json!({"type":"message_start","message":{"id":"msg_014a","type":"message",
        "role":"assistant","model":"example-model-20260201","content":[],
        "stop_reason":null,"stop_sequence":null}});
    event["message"]["usage"] = usage;
    event
}

fn message_delta(usage: Value) -> Value {
    let mut event = json!({"type":"message_delta",
        "delta":{"stop_reason":"end_turn","stop_sequence":null}});
    event["usage"] = usage;
    event
}

fn text_turn(start: Value, end: Value) -> Vec<Value> {
    vec![
        message_start(start),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}}),
        json!({"type":"content_block_stop","index":0}),
        message_delta(end),
        json!({"type":"message_stop"}),
    ]
}

fn start_usage() -> Value {
    json!({"input_tokens":11,"cache_read_input_tokens":0,
        "cache_creation_input_tokens":0,"output_tokens":1})
}

fn end_usage() -> Value {
    json!({"input_tokens":11,"cache_read_input_tokens":0,
        "cache_creation_input_tokens":0,"output_tokens":8})
}

fn with_cost(mut usage: Value, cost: Value) -> Value {
    usage["cost"] = cost;
    usage
}

async fn decode(events: &[Value]) -> (Result<TurnOutcome, Error>, Vec<StreamEvent>) {
    let mut sink = VecSink::new(64, 64 * 1024);
    let outcome = decode_stream(
        &sse(events),
        &request(),
        binding().provenance(),
        &mut sink,
        &Cancel::new(),
    )
    .await;
    (outcome, sink.into_events())
}

/// The `usage-extension` warnings among `events`, as their messages, in order.
fn extensions(events: &[StreamEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::Warning { code, message } if code == CODE => Some(message.clone()),
            _ => None,
        })
        .collect()
}

/// Every event that is not a `usage-extension` warning.
fn others(events: &[StreamEvent]) -> Vec<StreamEvent> {
    events
        .iter()
        .filter(|event| !matches!(event, StreamEvent::Warning { code, .. } if code == CODE))
        .cloned()
        .collect()
}

/// The value one warning reports for `at`: the message is `<path>: <compact JSON>`.
fn reported(message: &str, at: &str) -> Value {
    let value = message
        .strip_prefix(&format!("{at}: "))
        .unwrap_or_else(|| panic!("the warning names {at}: {message}"));
    serde_json::from_str(value).expect("the warning carries the value as JSON")
}

fn usage(outcome: &TurnOutcome) -> Option<Usage> {
    outcome.observation.usage.clone()
}

/// A turn with a `cost` has exactly the outcome the same turn has without it, apart from the
/// warning, which names the path and carries the value.
async fn carried_like_the_turn_without_it(cost: &Value) {
    let (plain, plain_events) = decode(&text_turn(start_usage(), end_usage())).await;
    let plain = plain.expect("the turn without a cost decodes");
    let (costed, costed_events) = decode(&text_turn(
        start_usage(),
        with_cost(end_usage(), cost.clone()),
    ))
    .await;
    let costed = costed.expect("a cost on message_delta.usage is admitted");
    assert_eq!(costed.items, plain.items);
    assert_eq!(costed.stop_reason, plain.stop_reason);
    assert_eq!(usage(&costed), usage(&plain));
    assert_eq!(costed.observation, plain.observation);
    assert_eq!(others(&costed_events), plain_events);
    let warnings = extensions(&costed_events);
    assert_eq!(
        warnings.len(),
        1,
        "one warning per reported cost: {warnings:?}"
    );
    assert_eq!(&reported(&warnings[0], "message_delta.usage.cost"), cost);
    assert!(
        !costed
            .items
            .iter()
            .any(|item| matches!(item, Item::Opaque { .. })),
        "a cost never becomes an item that would be replayed to a route",
    );
}

#[tokio::test]
async fn a_cost_on_message_delta_usage_decodes_and_is_carried_as_a_warning() {
    carried_like_the_turn_without_it(&json!(0.00042)).await;
    let (_, events) = decode(&text_turn(
        start_usage(),
        with_cost(end_usage(), json!(0.00042)),
    ))
    .await;
    let warnings = extensions(&events);
    assert!(
        warnings[0].contains("message_delta.usage.cost") && warnings[0].contains("0.00042"),
        "{warnings:?}",
    );
}

#[tokio::test]
async fn an_object_cost_is_carried_verbatim_as_a_warning() {
    carried_like_the_turn_without_it(&json!({"total":0.00042,"currency":"USD"})).await;
}

#[tokio::test]
async fn a_null_cost_decodes_and_emits_no_warning() {
    let (plain, plain_events) = decode(&text_turn(start_usage(), end_usage())).await;
    let (outcome, events) = decode(&text_turn(
        with_cost(start_usage(), Value::Null),
        with_cost(end_usage(), Value::Null),
    ))
    .await;
    let outcome = outcome.expect("a null cost is absent");
    assert_eq!(outcome, plain.expect("decoded"));
    assert_eq!(events, plain_events);
}

#[tokio::test]
async fn a_cost_on_both_usage_objects_yields_two_warnings_in_order() {
    let (outcome, events) = decode(&text_turn(
        with_cost(start_usage(), json!(0.0001)),
        with_cost(end_usage(), json!(0.00042)),
    ))
    .await;
    outcome.expect("decoded");
    let warnings = extensions(&events);
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert_eq!(
        reported(&warnings[0], "message_start.message.usage.cost"),
        json!(0.0001)
    );
    assert_eq!(
        reported(&warnings[1], "message_delta.usage.cost"),
        json!(0.00042)
    );
}

fn payload(data: Value) -> SseEvent {
    SseEvent::Payload {
        event: data["type"].as_str().map(str::to_owned),
        data,
    }
}

#[tokio::test]
async fn a_cost_on_message_start_is_held_until_the_stream_answers() {
    let mut decoder = StreamDecoder::new(binding().provenance().clone());
    let mut sink = VecSink::new(64, 64 * 1024);
    let cancel = Cancel::new();
    for lifecycle in [
        message_start(with_cost(start_usage(), json!(0.0001))),
        json!({"type":"ping"}),
    ] {
        decoder
            .apply(&payload(lifecycle), &mut sink, &cancel)
            .await
            .expect("admitted");
        assert!(
            sink.events().is_empty(),
            "nothing is shown before the stream answers: {:?}",
            sink.events(),
        );
    }
    assert!(!decoder.has_answered());
    decoder
        .apply(
            &payload(
                json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            ),
            &mut sink,
            &cancel,
        )
        .await
        .expect("the stream answers");
    let warnings = extensions(sink.events());
    assert_eq!(warnings.len(), 1, "{:?}", sink.events());
    assert_eq!(
        reported(&warnings[0], "message_start.message.usage.cost"),
        json!(0.0001)
    );
}

#[tokio::test]
async fn a_held_cost_is_dropped_with_a_stream_that_fails_before_answering() {
    let (outcome, events) = decode(&[
        message_start(with_cost(start_usage(), json!(0.0001))),
        json!({"type":"ping"}),
        json!({"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}),
    ])
    .await;
    let error = outcome.expect_err("the route failed");
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert!(
        error.retriable,
        "a held warning is not output: the attempt stays retriable"
    );
    assert!(
        events.is_empty(),
        "a stream that never answered showed nothing: {events:?}"
    );
}

#[tokio::test]
async fn an_unknown_usage_key_beside_a_cost_is_still_refused_by_its_path() {
    let mut end = with_cost(end_usage(), json!(0.00042));
    end["price"] = json!(0.00042);
    let (outcome, events) = decode(&text_turn(start_usage(), end)).await;
    let error = outcome.expect_err("price is outside the declared subset");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        error.message,
        "Messages field is outside the declared subset: message_delta.usage.price"
    );
    assert!(extensions(&events).is_empty(), "{events:?}");
}

#[tokio::test]
async fn a_usage_object_refused_for_a_regressing_counter_emits_no_extension_warning() {
    let (outcome, events) = decode(&text_turn(
        start_usage(),
        json!({"output_tokens":0,"cost":0.00042}),
    ))
    .await;
    let error = outcome.expect_err("a counter went backwards");
    assert_eq!(error.code, ErrorCode::Protocol);
    assert_eq!(error.message, "Messages cumulative usage regressed");
    assert!(extensions(&events).is_empty(), "{events:?}");
}

#[tokio::test]
async fn a_cost_over_one_kibibyte_is_refused_naming_its_path() {
    let large = json!({"note": "x".repeat(1024)});
    for (events, at) in [
        (
            text_turn(start_usage(), with_cost(end_usage(), large.clone())),
            "message_delta.usage.cost",
        ),
        (
            text_turn(with_cost(start_usage(), large.clone()), end_usage()),
            "message_start.message.usage.cost",
        ),
    ] {
        let (outcome, shown) = decode(&events).await;
        let error = outcome.expect_err("a cost over its bound is refused");
        assert_eq!(error.code, ErrorCode::TooLarge, "{}", error.message);
        assert_eq!(
            error.message,
            format!("Messages usage extension exceeds its bound: {at}")
        );
        assert!(extensions(&shown).is_empty(), "{shown:?}");
    }
    // Exactly at the bound is carried.
    let fits = json!("x".repeat(1022));
    assert_eq!(serde_json::to_string(&fits).expect("json").len(), 1024);
    let (outcome, shown) = decode(&text_turn(start_usage(), with_cost(end_usage(), fits))).await;
    outcome.expect("a cost of exactly 1 KiB is carried");
    assert_eq!(extensions(&shown).len(), 1);
}

#[test]
fn a_complete_message_still_refuses_a_usage_cost_by_its_path() {
    let message = json!({"id":"msg_014a","type":"message","role":"assistant",
        "model":"example-model-20260201","content":[{"type":"text","text":"ok"}],
        "stop_reason":"end_turn","stop_sequence":null,
        "usage":{"input_tokens":11,"output_tokens":8,"cost":0.00042}});
    let error = decode_message(&message, &request(), binding().provenance())
        .expect_err("a complete message has no sink to carry a cost on");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        error.message,
        "Messages field is outside the declared subset: usage.cost"
    );
}

/// The code a caller matches on is the crate's own constant, not a string it has to copy.
#[test]
fn the_warning_code_is_the_public_constant() {
    assert_eq!(llm_messages::USAGE_EXTENSION, CODE);
}
