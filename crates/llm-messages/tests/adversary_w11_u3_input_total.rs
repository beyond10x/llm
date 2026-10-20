//! Adversary, wave 2026-10-20-w11, unit 3 (story:messages-input-total-derived-from-what-llm-observed),
//! pass 1.
//!
//! The unit derives the Messages cache-write count from a `cache_creation` breakdown that reports
//! both of its counts when `cache_creation_input_tokens` is absent (`docs/messages.md` "Usage").
//! These cases drive that derivation against the rules the same page states for every counter.
mod support;

use llm_core::{Cancel, Error, ErrorCode, Item, TurnOutcome, TurnRequest, Usage, VecSink};
use llm_messages::{decode_message, decode_stream};
use serde_json::{Value, json};
use support::binding;

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

fn text_turn(usage_start: Value, usage_end: Value) -> Vec<Value> {
    let mut message = json!({"id":"msg_014a","type":"message","role":"assistant",
        "model":"example-model-20260201","content":[],
        "stop_reason":null,"stop_sequence":null});
    message["usage"] = usage_start;
    let mut delta = json!({"type":"message_delta",
        "delta":{"stop_reason":"end_turn","stop_sequence":null}});
    delta["usage"] = usage_end;
    vec![
        json!({"type":"message_start","message":message}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}),
        json!({"type":"content_block_stop","index":0}),
        delta,
        json!({"type":"message_stop"}),
    ]
}

async fn decode(events: &[Value]) -> Result<TurnOutcome, Error> {
    let mut sink = VecSink::new(64, 64 * 1024);
    decode_stream(
        &sse(events)[..],
        &request(),
        binding().provenance(),
        &mut sink,
        &Cancel::new(),
    )
    .await
}

fn answered_with(usage: Value) -> Value {
    let mut message = json!({"id":"msg_014a","type":"message","role":"assistant",
        "model":"example-model-20260201","content":[{"type":"text","text":"Hello"}],
        "stop_reason":"end_turn","stop_sequence":null});
    message["usage"] = usage;
    message
}

/// `docs/messages.md` "Usage": counters are cumulative and "a counter that goes backwards
/// contradicts the earlier report and is refused"; the unit's own case says a derived write count
/// is "a cumulative counter like a reported one", refused "rather than lowering the total". So a
/// known count in an earlier snapshot is a lower bound on the final one (the cost crate reads a
/// partial snapshot's counters that way). Here `message_start` carries a complete breakdown of 6
/// written and no write count, and `message_delta` reports a write count of 3. The same two
/// reports in one event are refused; split across two events, the snapshot a cut after
/// `message_start` carries (6 written, 21 input) must not be lowered by the turn that completes.
#[tokio::test]
async fn a_reported_write_count_below_the_derived_one_does_not_lower_the_known_total() {
    let start = json!({"input_tokens":11,"cache_read_input_tokens":4,"output_tokens":1,
        "cache_creation":{"ephemeral_1h_input_tokens":0,"ephemeral_5m_input_tokens":6}});
    let end = json!({"output_tokens":8,"cache_creation_input_tokens":3});

    let mut together = start.clone();
    together["cache_creation_input_tokens"] = json!(3);
    let one_event = decode(&text_turn(together, json!({"output_tokens":8}))).await;
    assert_eq!(
        one_event.expect_err("the same reports in one event").code,
        ErrorCode::Protocol
    );

    let events = text_turn(start, end);
    let earlier = decode(&events[..4])
        .await
        .expect_err("a stream cut before message_delta is refused")
        .observation
        .and_then(|observation| observation.usage);
    let Some(earlier) = earlier else {
        return;
    };
    match decode(&events).await {
        Err(error) => assert_eq!(error.code, ErrorCode::Protocol, "{error:?}"),
        Ok(outcome) => {
            let later = outcome.observation.usage.expect("usage");
            for (name, before, after) in [
                (
                    "cache_creation_input_tokens",
                    earlier.cache_creation_input_tokens,
                    later.cache_creation_input_tokens,
                ),
                ("input_tokens", earlier.input_tokens, later.input_tokens),
            ] {
                if let Some(before) = before {
                    assert!(
                        after.is_some_and(|after| after >= before),
                        "{name} was {before} after message_start and {after:?} at the end"
                    );
                }
            }
        }
    }
}

/// A complete breakdown is an observation of the write count even when it is the only counter
/// the usage carries: the unit made `Snapshot::is_known` count it, so the turn's usage is the
/// derived write count and every other counter stays unknown, not no usage at all.
#[test]
fn a_breakdown_that_is_the_only_counter_is_a_known_write_count() {
    let message = answered_with(
        json!({"cache_creation":{"ephemeral_1h_input_tokens":0,"ephemeral_5m_input_tokens":6}}),
    );
    let outcome = decode_message(&message, &request(), binding().provenance()).expect("decoded");
    assert_eq!(
        outcome.observation.usage,
        Some(Usage {
            cache_creation_input_tokens: Some(6),
            ..Usage::default()
        })
    );
}

/// The inclusive total with a derived write count meets `u64::MAX` exactly, and one more token is
/// refused as an overflow rather than wrapped or saturated.
#[test]
fn a_total_with_a_derived_write_count_reaches_u64_max_and_one_more_is_refused() {
    let with_fresh = |fresh: u64| {
        answered_with(
            json!({"input_tokens":fresh,"cache_read_input_tokens":0,"output_tokens":8,
            "cache_creation":{"ephemeral_1h_input_tokens":0,"ephemeral_5m_input_tokens":6}}),
        )
    };
    let at_max = decode_message(
        &with_fresh(u64::MAX - 6),
        &request(),
        binding().provenance(),
    )
    .expect("decoded");
    let usage = at_max.observation.usage.expect("usage");
    assert_eq!(usage.input_tokens, Some(u64::MAX));
    assert_eq!(usage.cache_creation_input_tokens, Some(6));
    let over = decode_message(
        &with_fresh(u64::MAX - 5),
        &request(),
        binding().provenance(),
    );
    assert_eq!(over.expect_err("overflow").code, ErrorCode::Protocol);
}
