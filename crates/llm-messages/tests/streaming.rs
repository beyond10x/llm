//! Messages response and event-stream decoding: terminal truth, usage and refusals.
mod support;

use llm_core::{
    Cancel, Dispatch, Error, ErrorCode, Item, MAX_ITEMS, StopReason, StreamEvent, ToolName,
    ToolSpec, TurnOutcome, TurnRequest, Usage, VecSink,
};
use llm_http::{Framing, SseDecoder};
use llm_messages::{IngressStream, StreamDecoder, decode_message, decode_stream};
use serde_json::{Value, json};
use support::binding;

fn request() -> TurnRequest {
    let mut request = TurnRequest::new("internal-model", vec![Item::user("Summarise the log")]);
    request.tools = vec![ToolSpec {
        name: ToolName::new("lookup").unwrap(),
        description: "Look up a record".to_owned(),
        input_schema: json!({"type":"object"}),
    }];
    request
}

/// Frames producer-shaped events exactly as the route sends them: named event, JSON payload.
fn sse(events: &[Value]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for event in events {
        let name = event["type"].as_str().expect("event type");
        bytes.extend_from_slice(format!("event: {name}\ndata: {event}\n\n").as_bytes());
    }
    bytes
}

fn message_start(usage: Value) -> Value {
    let mut message = json!({"id":"msg_014a","type":"message","role":"assistant",
        "model":"example-model-20260201","content":[],
        "stop_reason":null,"stop_sequence":null});
    message["usage"] = usage;
    json!({"type":"message_start","message":message})
}

fn message_delta(stop_reason: Value, usage: Value) -> Value {
    let mut event = json!({"type":"message_delta","delta":{"stop_sequence":null}});
    event["delta"]["stop_reason"] = stop_reason;
    event["usage"] = usage;
    event
}

fn text_turn(usage_start: Value, usage_end: Value) -> Vec<Value> {
    vec![
        message_start(usage_start),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}),
        json!({"type":"content_block_stop","index":0}),
        message_delta(json!("end_turn"), usage_end),
        json!({"type":"message_stop"}),
    ]
}

async fn decode(events: &[Value]) -> (Result<TurnOutcome, Error>, VecSink) {
    decode_bytes(&sse(events)).await
}

async fn decode_bytes(bytes: &[u8]) -> (Result<TurnOutcome, Error>, VecSink) {
    let mut sink = VecSink::new(64, 64 * 1024);
    let outcome = decode_stream(
        bytes,
        &request(),
        binding().provenance(),
        &mut sink,
        &Cancel::new(),
    )
    .await;
    (outcome, sink)
}

#[tokio::test]
async fn a_streamed_turn_preserves_text_thinking_tools_and_terminal_usage() {
    let events = vec![
        message_start(json!({"input_tokens":11,"cache_read_input_tokens":4,
            "cache_creation_input_tokens":6,"output_tokens":1})),
        json!({"type":"content_block_start","index":0,
            "content_block":{"type":"thinking","thinking":"","signature":""}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"thinking_delta","thinking":"weighing"}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"signature_delta","signature":"sig-1"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":1,
            "delta":{"type":"text_delta","text":"Looking"}}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"content_block_start","index":2,
            "content_block":{"type":"tool_use","id":"call-1","name":"lookup","input":{}}}),
        json!({"type":"content_block_delta","index":2,
            "delta":{"type":"input_json_delta","partial_json":"{\"query\":"}}),
        json!({"type":"content_block_delta","index":2,
            "delta":{"type":"input_json_delta","partial_json":"\"errors\"}"}}),
        json!({"type":"content_block_stop","index":2}),
        json!({"type":"ping"}),
        message_delta(
            json!("tool_use"),
            json!({"input_tokens":11,"cache_read_input_tokens":4,
                "cache_creation_input_tokens":6,"output_tokens":9,
                "output_tokens_details":{"thinking_tokens":3}}),
        ),
        json!({"type":"message_stop"}),
    ];
    let (outcome, sink) = decode(&events).await;
    let outcome = outcome.expect("decoded");
    assert_eq!(outcome.stop_reason, StopReason::ToolCalls);
    assert_eq!(
        outcome.items,
        vec![
            Item::Opaque {
                provenance: binding().provenance().clone(),
                payload: json!({"type":"thinking","thinking":"weighing","signature":"sig-1"}),
            },
            Item::assistant("Looking"),
            Item::ToolCall(llm_core::ToolCall {
                call_id: llm_core::CallId::new("call-1").unwrap(),
                name: ToolName::new("lookup").unwrap(),
                arguments: json!({"query":"errors"}),
            }),
        ]
    );
    assert_eq!(
        outcome
            .observation
            .upstream_model
            .as_ref()
            .map(ToString::to_string),
        Some("example-model-20260201".to_owned())
    );
    assert_eq!(
        outcome
            .observation
            .response_id
            .as_ref()
            .map(ToString::to_string),
        Some("msg_014a".to_owned())
    );
    assert!(outcome.observation.final_usage);
    assert_eq!(
        outcome.observation.usage,
        Some(Usage {
            // 11 fresh + 4 read + 6 written; the neutral total is inclusive.
            input_tokens: Some(21),
            output_tokens: Some(9),
            cached_input_tokens: Some(4),
            cache_creation_input_tokens: Some(6),
            reasoning_output_tokens: Some(3),
        })
    );
    assert_eq!(
        sink.events(),
        [
            StreamEvent::ReasoningDelta {
                text: "weighing".to_owned()
            },
            StreamEvent::TextDelta {
                text: "Looking".to_owned()
            },
            StreamEvent::ToolCallStarted {
                call_id: llm_core::CallId::new("call-1").unwrap(),
                name: ToolName::new("lookup").unwrap(),
            },
            StreamEvent::ToolArgumentsDelta {
                call_id: llm_core::CallId::new("call-1").unwrap(),
                delta: "{\"query\":".to_owned()
            },
            StreamEvent::ToolArgumentsDelta {
                call_id: llm_core::CallId::new("call-1").unwrap(),
                delta: "\"errors\"}".to_owned()
            },
        ]
    );
}

/// Text a `text` block opens with is model output like any delta's: the caller is shown it as
/// the block opens, so the turn carries exactly the text the caller was shown and nothing it was
/// not, with no warning. Every pinned fixture opens with `"text": ""`; a route that opens with
/// text is not known, and nothing says one never will. The same events, relayed by the ingress
/// encoder, close on the turn.
#[tokio::test]
async fn text_a_block_opens_with_is_shown_so_the_turn_carries_only_what_was_shown() {
    let cases: [(&str, &str, &[&str]); 3] = [
        ("opening text, then a delta", "Hel", &["lo"]),
        ("opening text alone", "Hello", &[]),
        ("no opening text", "", &["Hel", "lo"]),
    ];
    for (case, opening, deltas) in cases {
        let mut events = vec![
            message_start(json!({"input_tokens":11,"output_tokens":1})),
            json!({"type":"content_block_start","index":0,
                "content_block":{"type":"text","text":opening}}),
        ];
        for delta in deltas {
            events.push(json!({"type":"content_block_delta","index":0,
                "delta":{"type":"text_delta","text":delta}}));
        }
        events.extend([
            json!({"type":"content_block_stop","index":0}),
            message_delta(
                json!("end_turn"),
                json!({"input_tokens":11,"output_tokens":2}),
            ),
            json!({"type":"message_stop"}),
        ]);
        let (outcome, sink) = decode(&events).await;
        let outcome = outcome.unwrap_or_else(|error| panic!("{case}: {error:?}"));
        assert_eq!(outcome.items, vec![Item::assistant("Hello")], "{case}");
        let shown: Vec<StreamEvent> = std::iter::once(opening)
            .chain(deltas.iter().copied())
            .filter(|text| !text.is_empty())
            .map(|text| StreamEvent::TextDelta {
                text: text.to_owned(),
            })
            .collect();
        assert_eq!(
            sink.events(),
            shown.as_slice(),
            "{case}: the caller was not shown the text the turn carries"
        );
        assert_eq!(sink.text(), "Hello", "{case}");
        let mut relay = IngressStream::new("msg_014a", Some("example-model-20260201"))
            .expect("a relay for the route's model");
        for event in sink.events() {
            relay.chunk(event);
        }
        relay.close(&outcome).unwrap_or_else(|error| {
            panic!("{case}: the relayed stream cannot close on the turn: {error:?}")
        });
    }
}

/// story:ingress-block-boundaries. Text of a later block is a new message, so the caller is shown
/// a message boundary before it, and never before the first text it is shown. A redacted thinking
/// block has no delta to show it: it is shown whole, in its place, as the item the turn carries.
/// The events relay through the ingress encoder and read back as themselves.
#[tokio::test]
async fn text_of_a_later_block_follows_a_message_boundary_and_redacted_thinking_shows_whole() {
    let redacted = json!({"type":"redacted_thinking","data":"opaque-bytes"});
    let events = vec![
        message_start(json!({"input_tokens":11,"output_tokens":1})),
        json!({"type":"content_block_start","index":0,
            "content_block":{"type":"thinking","thinking":"","signature":""}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"thinking_delta","thinking":"Let me look"}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"signature_delta","signature":"sig-1"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,"content_block":redacted}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"content_block_start","index":2,
            "content_block":{"type":"text","text":"Hel"}}),
        json!({"type":"content_block_delta","index":2,
            "delta":{"type":"text_delta","text":"lo"}}),
        json!({"type":"content_block_stop","index":2}),
        json!({"type":"content_block_start","index":3,
            "content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":3,
            "delta":{"type":"text_delta","text":"Bye"}}),
        json!({"type":"content_block_stop","index":3}),
        message_delta(
            json!("end_turn"),
            json!({"input_tokens":11,"output_tokens":5}),
        ),
        json!({"type":"message_stop"}),
    ];
    let (outcome, sink) = decode(&events).await;
    let outcome = outcome.expect("the turn decodes");
    let text = |text: &str| StreamEvent::TextDelta {
        text: text.to_owned(),
    };
    assert_eq!(
        sink.events(),
        [
            StreamEvent::ReasoningDelta {
                text: "Let me look".to_owned()
            },
            StreamEvent::OpaqueItem {
                provenance: binding().provenance().clone(),
                payload: redacted.clone(),
            },
            text("Hel"),
            text("lo"),
            StreamEvent::MessageBoundary {},
            text("Bye"),
        ]
    );
    assert_eq!(outcome.items[1], opaque(redacted));
    assert_eq!(
        outcome.items[2..],
        [Item::assistant("Hello"), Item::assistant("Bye")]
    );

    let mut relay = IngressStream::new("msg_014a", Some("example-model-20260201"))
        .expect("a relay for the route's model");
    let mut relayed = Vec::new();
    for event in sink.events() {
        relayed.extend(relay.chunk(event));
    }
    relayed.extend(relay.close(&outcome).expect("the relay closes on the turn"));
    let (read_back, shown) = decode(&relayed).await;
    let read_back = read_back.expect("the relayed turn decodes");
    assert_eq!(
        (read_back.items, read_back.stop_reason),
        (outcome.items, outcome.stop_reason)
    );
    assert_eq!(shown.events(), sink.events());
}

/// The thinking block exactly as Harness's emulator opens it (`crates/harness-cli/tests/fixtures/
/// fake_messages.py` `thinking_events`, and Harness's pinned `anthropic-messages/2026-08-29`
/// through `2026-08-30.1` stream fixtures): no `signature` field on `content_block_start`, the
/// signature arriving whole in a `signature_delta`. The finished block is the same one a start
/// carrying `"signature": ""` produces (`docs/harness-parity.md` row M47).
#[tokio::test]
async fn a_thinking_block_that_opens_without_a_signature_field_takes_it_from_its_delta() {
    let events = vec![
        message_start(json!({"input_tokens":11,"output_tokens":1})),
        json!({"type":"content_block_start","index":0,
            "content_block":{"type":"thinking","thinking":""}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"thinking_delta","thinking":"OPAQUE-REASONING-BLOB"}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"signature_delta","signature":"OPAQUE-SIGNATURE"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,
            "content_block":{"type":"tool_use","id":"call-1","name":"lookup","input":{}}}),
        json!({"type":"content_block_delta","index":1,
            "delta":{"type":"input_json_delta","partial_json":"{}"}}),
        json!({"type":"content_block_stop","index":1}),
        message_delta(json!("tool_use"), json!({"output_tokens":9})),
        json!({"type":"message_stop"}),
    ];
    let (outcome, sink) = decode(&events).await;
    let outcome = outcome.expect("a block opened without a signature field is signed by its delta");
    assert_eq!(
        outcome.items[0],
        Item::Opaque {
            provenance: binding().provenance().clone(),
            payload: json!({"type":"thinking","thinking":"OPAQUE-REASONING-BLOB",
                "signature":"OPAQUE-SIGNATURE"}),
        }
    );
    assert_eq!(
        sink.events()[0],
        StreamEvent::ReasoningDelta {
            text: "OPAQUE-REASONING-BLOB".to_owned()
        }
    );
}

/// The other half of the same rule: a block that opened without a signature field and never
/// received one is still unsigned thinking, refused rather than replayed.
#[tokio::test]
async fn a_thinking_block_that_never_receives_a_signature_is_still_refused() {
    let events = vec![
        message_start(json!({"input_tokens":11,"output_tokens":1})),
        json!({"type":"content_block_start","index":0,
            "content_block":{"type":"thinking","thinking":""}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"thinking_delta","thinking":"weighing"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":1,
            "delta":{"type":"text_delta","text":"Done"}}),
        json!({"type":"content_block_stop","index":1}),
        message_delta(json!("end_turn"), json!({"output_tokens":9})),
        json!({"type":"message_stop"}),
    ];
    let error = decode(&events)
        .await
        .0
        .expect_err("a thinking block with no signature at all");
    assert_eq!(error.code, ErrorCode::Protocol, "{error:?}");
}

/// Only a thinking block begins a signature it was opened without: a `signature_delta` into a
/// text block still does not extend its block.
#[tokio::test]
async fn a_signature_delta_into_a_text_block_is_still_refused() {
    let events = vec![
        message_start(json!({"input_tokens":11,"output_tokens":1})),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"signature_delta","signature":"OPAQUE-SIGNATURE"}}),
        json!({"type":"content_block_stop","index":0}),
        message_delta(json!("end_turn"), json!({"output_tokens":9})),
        json!({"type":"message_stop"}),
    ];
    let error = decode(&events)
        .await
        .0
        .expect_err("a signature delta against a text block");
    assert_eq!(error.code, ErrorCode::Protocol, "{error:?}");
    assert_eq!(error.message, "Messages delta does not extend its block");
}

#[tokio::test]
async fn a_tool_call_is_announced_when_its_block_opens_even_before_any_argument() {
    let events = vec![
        message_start(json!({"input_tokens":11,"output_tokens":1})),
        json!({"type":"content_block_start","index":0,
            "content_block":{"type":"tool_use","id":"call-1","name":"lookup","input":{}}}),
        json!({"type":"content_block_stop","index":0}),
        message_delta(json!("tool_use"), json!({"output_tokens":3})),
        json!({"type":"message_stop"}),
    ];
    let (outcome, sink) = decode(&events).await;
    outcome.expect("decoded");
    assert_eq!(
        sink.events(),
        [StreamEvent::ToolCallStarted {
            call_id: llm_core::CallId::new("call-1").unwrap(),
            name: ToolName::new("lookup").unwrap(),
        }]
    );
}

#[tokio::test]
async fn an_opening_block_with_an_unpublishable_name_is_refused_before_it_is_announced() {
    let events = vec![
        message_start(json!({"input_tokens":11,"output_tokens":1})),
        json!({"type":"content_block_start","index":0,
            "content_block":{"type":"tool_use","id":"call-1","name":"look.up","input":{}}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"input_json_delta","partial_json":"{}"}}),
        json!({"type":"content_block_stop","index":0}),
        message_delta(json!("tool_use"), json!({"output_tokens":3})),
        json!({"type":"message_stop"}),
    ];
    let (outcome, sink) = decode(&events).await;
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::Unsupported);
    assert!(sink.events().is_empty(), "{:?}", sink.events());
}

#[tokio::test]
async fn cumulative_usage_replaces_reported_counters_instead_of_summing_deltas() {
    let (outcome, _) = decode(&text_turn(
        json!({"input_tokens":11,"cache_read_input_tokens":0,
            "cache_creation_input_tokens":0,"output_tokens":1}),
        json!({"input_tokens":11,"cache_read_input_tokens":0,
            "cache_creation_input_tokens":0,"output_tokens":8}),
    ))
    .await;
    let usage = outcome.expect("decoded").observation.usage.expect("usage");
    // Summing the two reports would bill 9 output tokens for 8.
    assert_eq!(usage.output_tokens, Some(8));
    assert_eq!(usage.input_tokens, Some(11));
}

#[tokio::test]
async fn a_counter_the_route_never_reported_stays_unknown() {
    let (outcome, _) = decode(&text_turn(
        json!({"input_tokens":11,"output_tokens":1}),
        json!({"output_tokens":8}),
    ))
    .await;
    let usage = outcome.expect("decoded").observation.usage.expect("usage");
    assert_eq!(usage.cached_input_tokens, None);
    assert_eq!(usage.cache_creation_input_tokens, None);
    assert_eq!(usage.reasoning_output_tokens, None);
    // The disjoint total is unknown while a component is: it never collapses to the fresh count.
    assert_eq!(usage.input_tokens, None);
    assert_eq!(usage.output_tokens, Some(8));
}

#[tokio::test]
async fn a_route_that_reports_no_usage_at_all_reports_none() {
    let events = vec![
        json!({"type":"message_start","message":{"id":"msg_014a","type":"message",
            "role":"assistant","model":"example-model-20260201","content":[]}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hi"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null}}),
        json!({"type":"message_stop"}),
    ];
    let outcome = decode(&events).await.0.expect("decoded");
    assert_eq!(outcome.observation.usage, None);
    assert!(outcome.observation.final_usage);
}

#[tokio::test]
async fn a_usage_counter_that_regresses_is_refused() {
    let (outcome, _) = decode(&text_turn(
        json!({"input_tokens":11,"cache_read_input_tokens":0,
            "cache_creation_input_tokens":0,"output_tokens":8}),
        json!({"output_tokens":3}),
    ))
    .await;
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::Protocol);
}

#[tokio::test]
async fn one_hour_cache_creation_is_refused_rather_than_priced_as_five_minute() {
    let (outcome, _) = decode(&text_turn(
        json!({"input_tokens":11,"cache_read_input_tokens":0,
            "cache_creation_input_tokens":6,"output_tokens":1}),
        json!({"input_tokens":11,"cache_read_input_tokens":0,"cache_creation_input_tokens":6,
            "output_tokens":8,
            "cache_creation":{"ephemeral_1h_input_tokens":6,"ephemeral_5m_input_tokens":0}}),
    ))
    .await;
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::Unsupported);
}

#[tokio::test]
async fn a_five_minute_cache_breakdown_that_contradicts_its_total_is_refused() {
    let (outcome, _) = decode(&text_turn(
        json!({"input_tokens":11,"cache_read_input_tokens":0,
            "cache_creation_input_tokens":6,"output_tokens":1}),
        json!({"input_tokens":11,"cache_read_input_tokens":0,"cache_creation_input_tokens":6,
            "output_tokens":8,
            "cache_creation":{"ephemeral_1h_input_tokens":0,"ephemeral_5m_input_tokens":2}}),
    ))
    .await;
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::Protocol);
}

/// A complete message answering one text turn with `usage`.
fn answered_with(usage: Value) -> Value {
    let mut message = json!({"id":"msg_014a","type":"message","role":"assistant",
        "model":"example-model-20260201","content":[{"type":"text","text":"Hello"}],
        "stop_reason":"end_turn","stop_sequence":null});
    message["usage"] = usage;
    message
}

/// 11 fresh, 4 read and 8 output tokens, `cache_creation` set to `breakdown`, and no
/// `cache_creation_input_tokens`: the one counter the Harness spike's emulator left out.
fn without_the_write_count(breakdown: Option<Value>) -> Value {
    let mut usage = json!({"input_tokens":11,"cache_read_input_tokens":4,"output_tokens":8});
    if let Some(breakdown) = breakdown {
        usage["cache_creation"] = breakdown;
    }
    usage
}

/// Anthropic's API reference (`docs/messages.md` "Usage") documents the total input as the sum of
/// `input_tokens`, `cache_creation_input_tokens` and `cache_read_input_tokens`, and
/// `cache_creation` as the cache write broken down by TTL. With the write count itself absent, a
/// breakdown that reports both of its counts is that count, so the inclusive total is known:
/// 11 fresh + 4 read + (0 + 6) written.
#[tokio::test]
async fn a_complete_cache_breakdown_gives_an_absent_write_count_and_the_inclusive_total() {
    let derived = Usage {
        input_tokens: Some(21),
        output_tokens: Some(8),
        cached_input_tokens: Some(4),
        cache_creation_input_tokens: Some(6),
        reasoning_output_tokens: None,
    };
    let usage = without_the_write_count(Some(
        json!({"ephemeral_1h_input_tokens":0,"ephemeral_5m_input_tokens":6}),
    ));
    let whole = decode_message(
        &answered_with(usage.clone()),
        &request(),
        binding().provenance(),
    )
    .expect("decoded");
    assert_eq!(
        whole.observation.usage,
        Some(derived.clone()),
        "a complete message"
    );

    // As the route streams it: the breakdown on `message_start`, then a `message_delta` whose
    // usage has no breakdown field at all (`MessageDeltaUsage`).
    let mut start = usage.clone();
    start["output_tokens"] = json!(1);
    let (streamed, _) = decode(&text_turn(start, json!({"output_tokens":8}))).await;
    let streamed = streamed.expect("decoded");
    assert!(streamed.observation.final_usage);
    assert_eq!(
        streamed.observation.usage,
        Some(derived.clone()),
        "the breakdown on message_start"
    );

    // The same breakdown arriving only on the terminal `message_delta` is read the same way.
    let (late, _) = decode(&text_turn(
        json!({"input_tokens":11,"cache_read_input_tokens":4,"output_tokens":1}),
        usage,
    ))
    .await;
    assert_eq!(
        late.expect("decoded").observation.usage,
        Some(derived),
        "the breakdown on message_delta"
    );
}

/// Usage is cumulative: a later event that reports a higher `cache_creation_input_tokens` replaces
/// the count the breakdown gave, as any later report replaces an earlier one. A lower one is refused
/// (`adversary_w11_u3_input_total.rs`).
#[tokio::test]
async fn a_write_count_reported_after_a_breakdown_replaces_the_derived_one() {
    let (outcome, _) = decode(&text_turn(
        json!({"input_tokens":11,"cache_read_input_tokens":4,"output_tokens":1,
            "cache_creation":{"ephemeral_1h_input_tokens":0,"ephemeral_5m_input_tokens":6}}),
        json!({"input_tokens":11,"cache_read_input_tokens":4,
            "cache_creation_input_tokens":7,"output_tokens":8}),
    ))
    .await;
    let usage = outcome.expect("decoded").observation.usage.expect("usage");
    assert_eq!(usage.cache_creation_input_tokens, Some(7));
    assert_eq!(usage.input_tokens, Some(22));
}

/// The other way: a breakdown that is absent, null, or missing either of its counts does not say
/// what was written, so the write count and the inclusive total stay unknown, never zero, and the
/// counters that were reported are kept. Each count is documented `default: 0`; a documented
/// default is not an observation.
#[tokio::test]
async fn an_absent_or_partial_cache_breakdown_leaves_the_write_count_and_the_total_unknown() {
    for breakdown in [
        None,
        Some(json!(null)),
        Some(json!({})),
        Some(json!({"ephemeral_5m_input_tokens":6})),
        Some(json!({"ephemeral_1h_input_tokens":0})),
        Some(json!({"ephemeral_1h_input_tokens":0,"ephemeral_5m_input_tokens":null})),
        Some(json!({"ephemeral_1h_input_tokens":null,"ephemeral_5m_input_tokens":6})),
    ] {
        let usage = without_the_write_count(breakdown.clone());
        let whole = decode_message(
            &answered_with(usage.clone()),
            &request(),
            binding().provenance(),
        )
        .unwrap_or_else(|error| panic!("{breakdown:?}: {error:?}"));
        let (streamed, _) = decode(&text_turn(usage, json!({"output_tokens":8}))).await;
        let streamed = streamed.unwrap_or_else(|error| panic!("{breakdown:?}: {error:?}"));
        for (how, outcome) in [("a complete message", whole), ("a stream", streamed)] {
            let usage = outcome
                .observation
                .usage
                .expect("the reported counters are kept");
            assert_eq!(
                usage.cache_creation_input_tokens, None,
                "{how}, {breakdown:?}"
            );
            assert_eq!(usage.input_tokens, None, "{how}, {breakdown:?}");
            assert_eq!(usage.cached_input_tokens, Some(4), "{how}, {breakdown:?}");
            assert_eq!(usage.output_tokens, Some(8), "{how}, {breakdown:?}");
        }
    }
}

/// A derived write count is a cumulative counter like a reported one: a later breakdown that sums
/// to less is refused rather than lowering the total. A breakdown that contradicts a write count
/// an earlier event reported is still refused, as it was before the derivation.
#[tokio::test]
async fn a_derived_write_count_that_goes_backwards_or_contradicts_a_reported_one_is_refused() {
    let backwards = decode(&text_turn(
        json!({"input_tokens":11,"cache_read_input_tokens":4,"output_tokens":1,
            "cache_creation":{"ephemeral_1h_input_tokens":0,"ephemeral_5m_input_tokens":6}}),
        json!({"output_tokens":8,
            "cache_creation":{"ephemeral_1h_input_tokens":0,"ephemeral_5m_input_tokens":2}}),
    ))
    .await
    .0;
    assert_eq!(backwards.expect_err("refused").code, ErrorCode::Protocol);
    let contradicting = decode(&text_turn(
        json!({"input_tokens":11,"cache_read_input_tokens":4,
            "cache_creation_input_tokens":6,"output_tokens":1}),
        json!({"output_tokens":8,
            "cache_creation":{"ephemeral_1h_input_tokens":0,"ephemeral_5m_input_tokens":8}}),
    ))
    .await
    .0;
    assert_eq!(
        contradicting.expect_err("refused").code,
        ErrorCode::Protocol
    );
}

#[tokio::test]
async fn a_charged_server_tool_use_is_refused_and_an_unused_one_is_not() {
    let charged = decode(&text_turn(
        json!({"input_tokens":11,"cache_read_input_tokens":0,
            "cache_creation_input_tokens":0,"output_tokens":1}),
        json!({"input_tokens":11,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,
            "output_tokens":8,"server_tool_use":{"web_search_requests":2}}),
    ))
    .await
    .0;
    assert_eq!(charged.expect_err("refused").code, ErrorCode::Unsupported);
    let unused = decode(&text_turn(
        json!({"input_tokens":11,"cache_read_input_tokens":0,
            "cache_creation_input_tokens":0,"output_tokens":1}),
        json!({"input_tokens":11,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,
            "output_tokens":8,"server_tool_use":{"web_search_requests":0}}),
    ))
    .await
    .0;
    assert!(unused.is_ok(), "an unused server tool is not a charge");
}

#[tokio::test]
async fn a_stream_that_ends_without_its_terminal_event_keeps_the_last_usage_snapshot() {
    let mut events = text_turn(
        json!({"input_tokens":11,"cache_read_input_tokens":4,
            "cache_creation_input_tokens":6,"output_tokens":1}),
        json!({"input_tokens":11,"cache_read_input_tokens":4,
            "cache_creation_input_tokens":6,"output_tokens":8}),
    );
    events.pop().expect("message_stop");
    let (outcome, sink) = decode(&events).await;
    let error = outcome.expect_err("EOF never manufactures success");
    assert_eq!(error.code, ErrorCode::Protocol);
    assert_eq!(error.dispatch, Dispatch::Accepted);
    let observation = error.observation.expect("the snapshot is retained");
    assert!(!observation.final_usage);
    assert_eq!(
        observation.usage.expect("partial usage").output_tokens,
        Some(8)
    );
    assert_eq!(observation.binding, *binding().provenance());
    // The prefix the caller already saw is not withdrawn.
    assert_eq!(
        sink.events(),
        [StreamEvent::TextDelta {
            text: "Hello".to_owned()
        }]
    );
}

#[tokio::test]
async fn a_stream_cut_inside_an_event_is_refused() {
    let mut bytes = sse(&[message_start(json!({"input_tokens":11,"output_tokens":1}))]);
    bytes.extend_from_slice(b"event: content_block_start\ndata: {\"type\":\"content");
    let (outcome, _) = decode_bytes(&bytes).await;
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::Protocol);
}

#[tokio::test]
async fn an_event_name_that_contradicts_its_payload_type_is_refused() {
    // A ping alone is accepted, so the contradicting name is the only reason to refuse.
    let bytes = b"event: content_block_start\ndata: {\"type\":\"ping\"}\n\n".to_vec();
    let (outcome, _) = decode_bytes(&bytes).await;
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::Protocol);
}

#[tokio::test]
async fn out_of_order_and_duplicate_transitions_are_refused() {
    let start = message_start(json!({"input_tokens":11,"output_tokens":1}));
    let cases: Vec<Vec<Value>> = vec![
        // A second message_start.
        vec![start.clone(), start.clone()],
        // Content before the message started.
        vec![json!({"type":"content_block_start","index":0,
            "content_block":{"type":"text","text":""}})],
        // A delta for a block that never started.
        vec![
            start.clone(),
            json!({"type":"content_block_delta","index":0,
                "delta":{"type":"text_delta","text":"Hello"}}),
        ],
        // Two blocks open at the same index.
        vec![
            start.clone(),
            json!({"type":"content_block_start","index":0,
                "content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_start","index":0,
                "content_block":{"type":"text","text":""}}),
        ],
        // A block stop for a block that never started.
        vec![
            start.clone(),
            json!({"type":"content_block_stop","index":3}),
        ],
        // A terminal message leaving a block open.
        vec![
            start.clone(),
            json!({"type":"content_block_start","index":0,
                "content_block":{"type":"text","text":""}}),
            message_delta(json!("end_turn"), json!({"output_tokens":8})),
            json!({"type":"message_stop"}),
        ],
        // A payload after the terminal event.
        vec![
            start.clone(),
            message_delta(json!("end_turn"), json!({"output_tokens":8})),
            json!({"type":"message_stop"}),
            json!({"type":"ping"}),
        ],
    ];
    for events in cases {
        let (outcome, _) = decode(&events).await;
        let error = outcome.expect_err("refused");
        assert_eq!(error.code, ErrorCode::Protocol, "{events:?}");
    }
}

fn opaque(payload: Value) -> Item {
    Item::Opaque {
        provenance: binding().provenance().clone(),
        payload,
    }
}

fn warning(code: &str, message: &str) -> StreamEvent {
    StreamEvent::Warning {
        code: code.to_owned(),
        message: message.to_owned(),
    }
}

/// `text_turn` with `extra` spliced in at `at`.
fn text_turn_with(at: usize, extra: &[Value]) -> Vec<Value> {
    let mut events = text_turn(
        json!({"input_tokens":11,"output_tokens":1}),
        json!({"input_tokens":11,"output_tokens":8}),
    );
    events.splice(at..at, extra.iter().cloned());
    events
}

/// Harness parity M36 (`harness-messages/src/lib.rs:443`-`455`, test `:1146`).
///
/// A route that adds an event type is not a broken stream. The event is kept whole as opaque
/// state bound to the serving binding, the caller is told it was not interpreted, and the turn
/// still completes on its own terminal event. The diagnostic is fixed: no producer text in it.
#[tokio::test]
async fn an_unknown_stream_event_is_preserved_with_a_warning_instead_of_ending_the_turn() {
    let event = json!({"type":"message_reconsidered","detail":"kept"});
    // After the text block stopped, before the terminal delta.
    let (outcome, sink) = decode(&text_turn_with(4, std::slice::from_ref(&event))).await;
    let outcome = outcome.expect("the turn completes");
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
    assert_eq!(outcome.items, vec![Item::assistant("Hello"), opaque(event)]);
    assert_eq!(
        sink.events(),
        [
            StreamEvent::TextDelta {
                text: "Hello".to_owned()
            },
            warning(
                "unknown-stream-event",
                "a stream event outside the pinned subset was preserved, not interpreted"
            ),
        ]
    );
    assert!(outcome.observation.final_usage);
}

/// Harness parity M36 (`harness-messages/src/lib.rs:566`-`578`).
///
/// A delta type this subset does not model, inside a block it does, is kept as the whole event and
/// warned about; the block itself still assembles from the deltas around it. The preserved event
/// sits after every block that had started when it arrived.
#[tokio::test]
async fn an_unknown_content_delta_is_preserved_with_a_warning_and_its_block_completes() {
    let event = json!({"type":"content_block_delta","index":0,
        "delta":{"type":"citations_delta","citation":{"cited_text":"kept"}}});
    // Between the text delta and the block stop.
    let (outcome, sink) = decode(&text_turn_with(3, std::slice::from_ref(&event))).await;
    let outcome = outcome.expect("the turn completes");
    assert_eq!(outcome.items, vec![Item::assistant("Hello"), opaque(event)]);
    assert_eq!(
        sink.events(),
        [
            StreamEvent::TextDelta {
                text: "Hello".to_owned()
            },
            warning(
                "unknown-stream-event",
                "a content block delta outside the pinned subset was preserved, not interpreted"
            ),
        ]
    );
}

/// Harness parity M23 (`harness-messages/src/project.rs:321`-`354`, test `:773`).
///
/// A content block type this subset does not model is kept at its index as opaque state bound to
/// the serving binding, and warned about when it opens. It is never dropped: a dropped block is a
/// hole in the conversation the next turn cannot see.
#[tokio::test]
async fn an_unknown_content_block_is_preserved_with_a_warning_instead_of_ending_the_turn() {
    let block = json!({"type":"server_tool_use","id":"srvtoolu_1","name":"web_search",
        "input":{"query":"errors"}});
    let (outcome, sink) = decode(&text_turn_with(
        4,
        &[
            json!({"type":"content_block_start","index":1,"content_block":block}),
            json!({"type":"content_block_stop","index":1}),
        ],
    ))
    .await;
    let outcome = outcome.expect("the turn completes");
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
    assert_eq!(outcome.items, vec![Item::assistant("Hello"), opaque(block)]);
    assert_eq!(
        sink.events(),
        [
            StreamEvent::TextDelta {
                text: "Hello".to_owned()
            },
            warning(
                "unknown-output-item",
                "a content block outside the pinned subset was preserved, not interpreted"
            ),
        ]
    );
}

/// The other half of the M23 decision. A complete response has no stream to warn on, and a block
/// preserved without telling anyone is one nobody knows is uninterpreted, so `decode_message`
/// keeps refusing it.
#[test]
fn a_complete_message_still_refuses_an_unknown_block_it_has_no_channel_to_warn_about() {
    let message = json!({
        "id":"msg_014a","type":"message","role":"assistant","model":"example-model-20260201",
        "content":[{"type":"text","text":"Hello"},{"type":"novel_block","detail":"kept"}],
        "stop_reason":"end_turn","usage":{"input_tokens":11,"output_tokens":8}});
    let error = decode_message(&message, &request(), binding().provenance()).expect_err("refused");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        error.message,
        "Messages content is outside the declared subset"
    );
}

#[tokio::test]
async fn a_terminal_message_without_a_stop_reason_is_refused() {
    let mut events = text_turn(
        json!({"input_tokens":11,"output_tokens":1}),
        json!({"output_tokens":8}),
    );
    events[4] = message_delta(Value::Null, json!({"output_tokens":8}));
    let (outcome, _) = decode(&events).await;
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::Protocol);
}

#[tokio::test]
async fn a_terminal_reason_that_contradicts_the_content_is_refused() {
    let mut events = text_turn(
        json!({"input_tokens":11,"output_tokens":1}),
        json!({"output_tokens":8}),
    );
    events[1] = json!({"type":"content_block_start","index":0,
        "content_block":{"type":"tool_use","id":"call-1","name":"lookup","input":{}}});
    events[2] = json!({"type":"content_block_delta","index":0,
        "delta":{"type":"input_json_delta","partial_json":"{}"}});
    // `end_turn` beside a tool call is a turn a caller would record as finished.
    let (outcome, _) = decode(&events).await;
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::Protocol);
}

#[tokio::test]
async fn an_unfinished_stop_reason_is_carried_under_its_own_name() {
    let mut events = text_turn(
        json!({"input_tokens":11,"output_tokens":1}),
        json!({"output_tokens":8}),
    );
    events[4] = message_delta(json!("refusal"), json!({"output_tokens":8}));
    let outcome = decode(&events).await.0.expect("decoded");
    assert_eq!(
        outcome.stop_reason,
        StopReason::Incomplete {
            reason: "refusal".to_owned()
        }
    );
}

#[tokio::test]
async fn a_provider_error_event_refuses_without_copying_upstream_text() {
    let events = vec![
        message_start(json!({"input_tokens":11,"output_tokens":1})),
        json!({"type":"error","error":{"type":"overloaded_error",
            "message":"Overloaded: internal trace 0xdeadbeef for tenant acme"}}),
    ];
    let (outcome, _) = decode(&events).await;
    let error = outcome.expect_err("refused");
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert!(
        !error.message.contains("acme") && !error.message.contains("0xdeadbeef"),
        "upstream text leaked: {}",
        error.message
    );
    assert_eq!(
        error
            .observation
            .expect("snapshot retained")
            .usage
            .expect("usage")
            .output_tokens,
        Some(1)
    );
}

#[tokio::test]
async fn streamed_tool_arguments_are_parsed_once_and_bounded() {
    let mut events = text_turn(
        json!({"input_tokens":11,"output_tokens":1}),
        json!({"output_tokens":8}),
    );
    events[1] = json!({"type":"content_block_start","index":0,
        "content_block":{"type":"tool_use","id":"call-1","name":"lookup","input":{}}});
    events[2] = json!({"type":"content_block_delta","index":0,
        "delta":{"type":"input_json_delta","partial_json":"{\"query\":"}});
    // Half an argument object never reaches a caller as a whole one.
    let (outcome, _) = decode(&events).await;
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::Protocol);

    events[2] = json!({"type":"content_block_delta","index":0,
        "delta":{"type":"input_json_delta",
            "partial_json":format!("{{\"query\":\"{}\"}}", "x".repeat(llm_core::MAX_TOOL_ARGUMENT_BYTES))}});
    let (outcome, _) = decode(&events).await;
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::TooLarge);
}

#[tokio::test]
async fn a_tool_call_the_request_never_published_is_refused() {
    let mut events = text_turn(
        json!({"input_tokens":11,"output_tokens":1}),
        json!({"output_tokens":8}),
    );
    events[1] = json!({"type":"content_block_start","index":0,
        "content_block":{"type":"tool_use","id":"call-1","name":"unpublished","input":{}}});
    events[2] = json!({"type":"content_block_delta","index":0,
        "delta":{"type":"input_json_delta","partial_json":"{}"}});
    events[4] = message_delta(json!("tool_use"), json!({"output_tokens":8}));
    let (outcome, _) = decode(&events).await;
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::Protocol);
}

/// story:unpublished-tool-call-reaches-an-opted-in-caller. The live verbs runs (Harness wave
/// D200) took this path: the model called `find`, an entry name the turn did not publish. A caller
/// that set `controls.return_unpublished_calls` gets the call to refuse; the turn is not lost.
#[tokio::test]
async fn a_tool_call_the_request_never_published_reaches_a_caller_that_asked_for_it() {
    let mut events = text_turn(
        json!({"input_tokens":11,"output_tokens":1}),
        json!({"output_tokens":8}),
    );
    events[1] = json!({"type":"content_block_start","index":0,
        "content_block":{"type":"tool_use","id":"toolu_1","name":"find","input":{}}});
    events[2] = json!({"type":"content_block_delta","index":0,
        "delta":{"type":"input_json_delta","partial_json":"{\"pattern\":\"*.md\"}"}});
    events[4] = message_delta(json!("tool_use"), json!({"output_tokens":8}));
    let mut opted_in = request();
    opted_in.controls.return_unpublished_calls = true;
    let mut sink = VecSink::new(64, 64 * 1024);
    let outcome = decode_stream(
        &sse(&events),
        &opted_in,
        binding().provenance(),
        &mut sink,
        &Cancel::new(),
    )
    .await
    .expect("the unpublished call reaches the caller");
    assert_eq!(outcome.stop_reason, StopReason::ToolCalls);
    let calls: Vec<_> = outcome.tool_calls().collect();
    assert_eq!(calls.len(), 1, "{:?}", outcome.items);
    assert_eq!(calls[0].name.as_str(), "find");
    assert_eq!(calls[0].arguments, json!({"pattern": "*.md"}));
}

#[test]
fn one_complete_message_decodes_through_the_same_codec() {
    let message = json!({
        "id":"msg_014a","type":"message","role":"assistant","model":"example-model-20260201",
        "content":[{"type":"text","text":"Hello"}],
        "stop_reason":"end_turn","stop_sequence":null,
        "usage":{"input_tokens":11,"cache_read_input_tokens":4,
            "cache_creation_input_tokens":6,"output_tokens":8}});
    let outcome = decode_message(&message, &request(), binding().provenance()).expect("decoded");
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
    assert_eq!(outcome.items, vec![Item::assistant("Hello")]);
    assert!(outcome.observation.final_usage);
    assert_eq!(
        outcome.observation.usage.expect("usage").input_tokens,
        Some(21)
    );
}

/// The example is a metadata object no documentation of the route lists. Until 0.8.1 it was
/// `container`, which the route documents and sends; that one is now accepted (below).
#[test]
fn a_complete_message_field_outside_the_declared_subset_is_refused() {
    let mut message = json!({
        "id":"msg_014a","type":"message","role":"assistant","model":"example-model-20260201",
        "content":[{"type":"text","text":"Hello"}],"stop_reason":"end_turn",
        "usage":{"input_tokens":11,"output_tokens":8},
        "unlisted_metadata":{"id":"container_1","expires_at":"2026-01-01T00:00:00Z"}});
    assert_eq!(
        decode_message(&message, &request(), binding().provenance())
            .expect_err("refused")
            .code,
        ErrorCode::Unsupported
    );
    // A failure still retains what the route did report.
    message["usage"] = json!({"input_tokens":11,"output_tokens":8});
    let error = decode_message(
        &json!({"id":"msg_014a","type":"message","role":"assistant",
            "model":"example-model-20260201","content":[{"type":"image","source":{}}],
            "stop_reason":"end_turn","usage":{"input_tokens":11,"output_tokens":8}}),
        &request(),
        binding().provenance(),
    )
    .expect_err("refused");
    assert_eq!(
        error
            .observation
            .expect("snapshot retained")
            .usage
            .expect("usage")
            .output_tokens,
        Some(8)
    );
}

/// `src/decode.rs:320`: content blocks are bounded **while the stream is still arriving**, not
/// after it ends.
///
/// Without that bound the only thing that refuses an overrun is the neutral outcome's own late
/// check in `TurnOutcome::validate_for`, which runs once the stream is finished — so a stream that
/// never finishes is never checked at all, and a caller is streamed unbounded content first. This
/// stream therefore carries no terminal event: with the in-flight bound it is refused as too large
/// at the block that crosses it, and the caller sees exactly the bound and no more; without it the
/// same bytes are refused for the unrelated reason that they ran out.
#[tokio::test]
async fn content_blocks_are_bounded_while_the_stream_is_still_arriving() {
    let mut events = vec![message_start(json!({"input_tokens":11,"output_tokens":1}))];
    for index in 0..=u64::try_from(MAX_ITEMS).expect("a representable bound") {
        events.push(json!({"type":"content_block_start","index":index,
            "content_block":{"type":"text","text":""}}));
        events.push(json!({"type":"content_block_delta","index":index,
            "delta":{"type":"text_delta","text":"x"}}));
        events.push(json!({"type":"content_block_stop","index":index}));
    }
    // Room for every block's text and the message boundary before each after the first, so the
    // bound that refuses is the decoder's and not the sink's.
    let mut sink = VecSink::new(2 * MAX_ITEMS + 8, 4 * 1024 * 1024);
    let error = decode_stream(
        &sse(&events),
        &request(),
        binding().provenance(),
        &mut sink,
        &Cancel::new(),
    )
    .await
    .expect_err("a stream whose content exceeds its bound");
    assert_eq!(
        error.code,
        ErrorCode::TooLarge,
        "the overrun was not refused while the stream was arriving; it was carried to the end",
    );
    assert_eq!(error.message, "Messages content exceeds its bound");
    assert_eq!(
        sink.text().len(),
        MAX_ITEMS,
        "content past the bound reached the caller before anything refused it",
    );
    assert_eq!(sink.events().len(), 2 * MAX_ITEMS - 1);
}

/// A field this subset accepts and never consumes still arrives in the shape the producer
/// documents, or it is refused.
///
/// Accepting a name is not the same as ignoring its value: a value in a shape the producer does
/// not document is evidence that the route is not the one this projection was written against,
/// and this codec refuses what it cannot account for rather than passing over it. The class is
/// every name on an accept list that nothing in the codec reads, and it has exactly four members:
/// `service_tier` and `inference_geo` on the usage object, and `stop_sequence` on a message and on
/// a message delta.
#[tokio::test]
async fn an_accepted_field_this_subset_never_consumes_keeps_its_documented_shape() {
    // service_tier, on the usage object of a message start.
    let mut start = message_start(json!({"input_tokens":11,"output_tokens":1}));
    start["message"]["usage"]["service_tier"] = json!(3);
    // inference_geo, on the usage object of a message delta.
    let mut delta = message_delta(json!("end_turn"), json!({"output_tokens":8}));
    delta["usage"]["inference_geo"] = json!(["eu-west"]);
    // stop_sequence, on the message a stream starts with.
    let mut sequence_on_message = message_start(json!({"input_tokens":11,"output_tokens":1}));
    sequence_on_message["message"]["stop_sequence"] = json!(5);
    // stop_sequence, on a message delta.
    let mut sequence_on_delta = message_delta(json!("end_turn"), json!({"output_tokens":8}));
    sequence_on_delta["delta"]["stop_sequence"] = json!({"literal": "END"});
    let good_start = message_start(json!({"input_tokens":11,"output_tokens":1}));
    let good_delta = message_delta(json!("end_turn"), json!({"output_tokens":8}));
    for (name, events) in [
        ("service_tier", vec![start, good_delta.clone()]),
        ("inference_geo", vec![good_start.clone(), delta]),
        (
            "stop_sequence on a message",
            vec![sequence_on_message, good_delta],
        ),
        (
            "stop_sequence on a delta",
            vec![good_start, sequence_on_delta],
        ),
    ] {
        let mut events = events;
        events.push(json!({"type":"message_stop"}));
        let (outcome, _) = decode(&events).await;
        let error = outcome
            .err()
            .unwrap_or_else(|| panic!("{name} was accepted in a shape the producer never sends"));
        assert_eq!(error.code, ErrorCode::Protocol, "{name}");
    }
}

/// The other half of the rule above: the documented shape is accepted, and so is the explicit
/// null the producer writes when the field does not apply. A shape check that refuses what the
/// route really sends is worse than none.
#[tokio::test]
async fn an_accepted_field_in_its_documented_shape_is_carried_without_complaint() {
    let mut start = message_start(json!({"input_tokens":11,"output_tokens":1,
        "service_tier":"standard","inference_geo":"eu-west"}));
    start["message"]["stop_sequence"] = json!("END");
    let mut delta = message_delta(json!("stop_sequence"), json!({"output_tokens":8}));
    delta["delta"]["stop_sequence"] = json!("END");
    // The text the model wrote before the sequence stopped it: without content the turn would be
    // a failure (`every_unmodelled_reason_without_content_fails_as_its_row_at_every_split`).
    let events = vec![
        start,
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"text_delta","text":"Before the sequence"}}),
        json!({"type":"content_block_stop","index":0}),
        delta,
        json!({"type":"message_stop"}),
    ];
    let (outcome, _) = decode(&events).await;
    let outcome = outcome.expect("a message reporting its documented metadata");
    assert_eq!(
        outcome.stop_reason,
        StopReason::Incomplete {
            reason: "stop_sequence".to_owned()
        }
    );
    assert_eq!(outcome.items, [Item::assistant("Before the sequence")]);
}

/// A value the producer could put in a field; a refusal must never carry it.
const UNDECLARED_VALUE: &str = "value-that-no-diagnostic-may-carry";

fn outside_the_subset(path: &str) -> String {
    format!("Messages field is outside the declared subset: {path}")
}

/// Adds `undeclared` to the object at `pointer` inside `event`.
fn with_undeclared(mut event: Value, pointer: &str) -> Value {
    event
        .pointer_mut(pointer)
        .and_then(Value::as_object_mut)
        .unwrap_or_else(|| panic!("{pointer} is an object"))
        .insert("undeclared".to_owned(), json!(UNDECLARED_VALUE));
    event
}

/// A live stream refused with "field is outside the declared subset" and nothing else, so the
/// next run could not say which field to read the producer's documentation for. Every object a
/// stream carries names itself in that refusal, by path from the event, with the field's name and
/// never its value.
#[tokio::test]
async fn a_streamed_field_outside_the_declared_subset_is_refused_by_its_path_and_never_its_value() {
    let usage = json!({"input_tokens":11,"output_tokens":1,
        "cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":0},
        "output_tokens_details":{"thinking_tokens":0},
        "server_tool_use":{"web_search_requests":0}});
    // (event position in `text_turn`, pointer inside that event, path the refusal names)
    let cases = [
        (0, "", "message_start.undeclared"),
        (0, "/message", "message_start.message.undeclared"),
        (
            0,
            "/message/usage",
            "message_start.message.usage.undeclared",
        ),
        (
            0,
            "/message/usage/cache_creation",
            "message_start.message.usage.cache_creation.undeclared",
        ),
        (
            0,
            "/message/usage/output_tokens_details",
            "message_start.message.usage.output_tokens_details.undeclared",
        ),
        (
            0,
            "/message/usage/server_tool_use",
            "message_start.message.usage.server_tool_use.undeclared",
        ),
        (1, "", "content_block_start.undeclared"),
        (
            1,
            "/content_block",
            "content_block_start.content_block.undeclared",
        ),
        (2, "", "content_block_delta.undeclared"),
        (2, "/delta", "content_block_delta.delta.undeclared"),
        (3, "", "content_block_stop.undeclared"),
        (4, "", "message_delta.undeclared"),
        (4, "/delta", "message_delta.delta.undeclared"),
        (4, "/usage", "message_delta.usage.undeclared"),
        (5, "", "message_stop.undeclared"),
    ];
    for (at, pointer, path) in cases {
        let mut events = text_turn(usage.clone(), usage.clone());
        events[at] = with_undeclared(events[at].clone(), pointer);
        let (outcome, _) = decode(&events).await;
        let error = outcome.expect_err(path);
        assert_eq!(error.code, ErrorCode::Unsupported, "{path}: {error}");
        assert_eq!(error.message, outside_the_subset(path));
        assert!(!error.message.contains(UNDECLARED_VALUE), "{error}");
    }

    // A keep-alive is an event too.
    let mut events = text_turn(json!({"input_tokens":11}), json!({"output_tokens":8}));
    events.insert(1, json!({"type":"ping","undeclared":UNDECLARED_VALUE}));
    let (outcome, _) = decode(&events).await;
    assert_eq!(
        outcome.expect_err("ping").message,
        outside_the_subset("ping.undeclared")
    );

    // A field name is producer text too: one that is not name-shaped is not copied.
    let mut events = text_turn(json!({"input_tokens":11}), json!({"output_tokens":8}));
    events[0]["message"]["usage"]["not a name\n"] = json!(1);
    let (outcome, _) = decode(&events).await;
    assert_eq!(
        outcome.expect_err("unnamed").message,
        outside_the_subset("message_start.message.usage.?")
    );
}

#[test]
fn a_complete_message_field_outside_the_declared_subset_is_refused_by_its_path() {
    let message = || {
        json!({"id":"msg_014a","type":"message","role":"assistant",
            "model":"example-model-20260201","content":[{"type":"text","text":"Hello"}],
            "stop_reason":"end_turn","usage":{"input_tokens":11,"output_tokens":8}})
    };
    for (pointer, path) in [
        ("", "undeclared"),
        ("/usage", "usage.undeclared"),
        ("/content/0", "content[].undeclared"),
    ] {
        let error = decode_message(
            &with_undeclared(message(), pointer),
            &request(),
            binding().provenance(),
        )
        .expect_err(path);
        assert_eq!(error.code, ErrorCode::Unsupported, "{path}: {error}");
        assert_eq!(error.message, outside_the_subset(path));
    }
}

/// The fixed text of every shape refusal below: no value the route sent can be in it.
const UNREPORTED_SHAPE: &str = "Messages accepted field has a type this route does not report";

/// A complete message as the route answers one text turn, with `extra` set at its top level.
fn complete(extra: &[(&str, Value)]) -> Value {
    let mut message = json!({"id":"msg_014a","type":"message","role":"assistant",
        "model":"example-model-20260201","content":[{"type":"text","text":"Hello"}],
        "stop_reason":"end_turn","stop_sequence":null,
        "usage":{"input_tokens":11,"output_tokens":8}});
    for (key, value) in extra {
        message[*key] = value.clone();
    }
    message
}

/// `container` in each shape the route documents (`null`, or the container a container tool ran
/// in), then in shapes it does not.
fn containers() -> (Vec<Value>, Vec<Value>) {
    (
        vec![
            Value::Null,
            json!({"id":"container_1","expires_at":"2026-01-01T00:00:00Z"}),
            json!({"id":"container_1","expires_at":"2026-01-01T00:00:00Z","skills":null}),
        ],
        vec![
            json!({"id":1}),
            json!({"id":"c","expires_at":"t","x":1}),
            json!("c"),
            json!({"id":"c","expires_at":"t",
                "skills":[{"type":"anthropic","skill_id":"pdf","version":"latest"}]}),
        ],
    )
}

/// A refusal of `value` that carries the fixed shape text, and so not the value.
fn refused_by_its_shape(error: &Error, value: &Value) {
    assert_eq!(error.code, ErrorCode::Protocol, "{value}: {error}");
    assert_eq!(error.message, UNREPORTED_SHAPE, "{value}");
}

/// 0.8.0 refused a live turn: `Messages field is outside the declared subset:
/// message_start.message.container`. The route documents `container` on every message, `null`
/// unless a container tool ran; it is accepted in its documented shape and never consumed, and
/// any other shape is refused without its value.
#[tokio::test]
async fn a_container_on_a_message_start_is_accepted_in_its_documented_shape_and_no_other() {
    let (documented, undocumented) = containers();
    for container in documented {
        let mut events = text_turn(json!({"input_tokens":11}), json!({"output_tokens":8}));
        events[0]["message"]["container"] = container.clone();
        let (outcome, _) = decode(&events).await;
        let outcome = outcome.unwrap_or_else(|error| panic!("{container}: {error}"));
        assert_eq!(outcome.items, vec![Item::assistant("Hello")], "{container}");
    }
    for container in undocumented {
        let mut events = text_turn(json!({"input_tokens":11}), json!({"output_tokens":8}));
        events[0]["message"]["container"] = container.clone();
        let (outcome, _) = decode(&events).await;
        refused_by_its_shape(&outcome.expect_err("an undocumented container"), &container);
    }
}

#[test]
fn a_container_on_a_complete_message_is_accepted_in_its_documented_shape_and_no_other() {
    let (documented, undocumented) = containers();
    for container in documented {
        let outcome = decode_message(
            &complete(&[("container", container.clone())]),
            &request(),
            binding().provenance(),
        )
        .unwrap_or_else(|error| panic!("{container}: {error}"));
        assert_eq!(outcome.items, vec![Item::assistant("Hello")], "{container}");
    }
    for container in undocumented {
        let error = decode_message(
            &complete(&[("container", container.clone())]),
            &request(),
            binding().provenance(),
        )
        .expect_err("an undocumented container");
        refused_by_its_shape(&error, &container);
    }
}

/// The route documents the same `container` on `message_delta.delta`; one check reads it
/// wherever it is accepted.
#[tokio::test]
async fn a_container_on_a_message_delta_is_read_by_the_same_check() {
    let (documented, undocumented) = containers();
    for container in documented {
        let mut events = text_turn(json!({"input_tokens":11}), json!({"output_tokens":8}));
        events[4]["delta"]["container"] = container.clone();
        let (outcome, _) = decode(&events).await;
        outcome.unwrap_or_else(|error| panic!("{container}: {error}"));
    }
    for container in undocumented {
        let mut events = text_turn(json!({"input_tokens":11}), json!({"output_tokens":8}));
        events[4]["delta"]["container"] = container.clone();
        let (outcome, _) = decode(&events).await;
        refused_by_its_shape(&outcome.expect_err("an undocumented container"), &container);
    }
}

/// `diagnostics` is `null` unless the request supplied `diagnostics`, which this projection never
/// sends; a diagnosis is refused, on a stream and on a complete message.
#[tokio::test]
async fn diagnostics_are_accepted_only_as_null() {
    let diagnosis = json!({"cache_miss_reason":{"type":"model_changed",
        "cache_missed_input_tokens":0}});
    let mut events = text_turn(json!({"input_tokens":11}), json!({"output_tokens":8}));
    events[0]["message"]["diagnostics"] = Value::Null;
    let (outcome, _) = decode(&events).await;
    outcome.expect("null diagnostics on a message start");
    decode_message(
        &complete(&[("diagnostics", Value::Null)]),
        &request(),
        binding().provenance(),
    )
    .expect("null diagnostics on a complete message");

    events[0]["message"]["diagnostics"] = diagnosis.clone();
    let (outcome, _) = decode(&events).await;
    refused_by_its_shape(&outcome.expect_err("a diagnosis"), &diagnosis);
    let error = decode_message(
        &complete(&[("diagnostics", diagnosis.clone())]),
        &request(),
        binding().provenance(),
    )
    .expect_err("a diagnosis");
    refused_by_its_shape(&error, &diagnosis);
}

/// `usage.speed` is `standard` or `null` unless the request asked for `fast`, which this projection
/// never sends and which prices differently.
#[tokio::test]
async fn a_usage_speed_is_accepted_only_as_standard_or_null() {
    for speed in [json!("standard"), Value::Null] {
        let (outcome, _) = decode(&text_turn(
            json!({"input_tokens":11,"speed":speed}),
            json!({"output_tokens":8,"speed":speed}),
        ))
        .await;
        outcome.unwrap_or_else(|error| panic!("{speed}: {error}"));
        decode_message(
            &complete(&[(
                "usage",
                json!({"input_tokens":11,"output_tokens":8,"speed":speed}),
            )]),
            &request(),
            binding().provenance(),
        )
        .unwrap_or_else(|error| panic!("{speed}: {error}"));
    }
    for speed in [json!("fast"), json!(1)] {
        let (outcome, _) = decode(&text_turn(
            json!({"input_tokens":11,"speed":speed}),
            json!({"output_tokens":8}),
        ))
        .await;
        refused_by_its_shape(
            &outcome.expect_err("a speed this route was never asked for"),
            &speed,
        );
        let (outcome, _) = decode(&text_turn(
            json!({"input_tokens":11}),
            json!({"output_tokens":8,"speed":speed}),
        ))
        .await;
        refused_by_its_shape(
            &outcome.expect_err("a speed this route was never asked for"),
            &speed,
        );
    }
}

/// One tool turn whose `tool_use` block opens as `block`.
fn tool_turn(block: &Value) -> Vec<Value> {
    vec![
        message_start(json!({"input_tokens":11,"output_tokens":1})),
        json!({"type":"content_block_start","index":0,"content_block":block}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"input_json_delta","partial_json":"{}"}}),
        json!({"type":"content_block_stop","index":0}),
        message_delta(json!("tool_use"), json!({"output_tokens":8})),
        json!({"type":"message_stop"}),
    ]
}

/// A `tool_use` block carrying `key: value`, streamed and as a complete message.
async fn tool_use_with(key: &str, value: &Value) -> [Result<TurnOutcome, Error>; 2] {
    let mut block = json!({"type":"tool_use","id":"call-1","name":"lookup","input":{}});
    block[key] = value.clone();
    let (streamed, _) = decode(&tool_turn(&block)).await;
    let whole = decode_message(
        &complete(&[
            ("content", json!([block])),
            ("stop_reason", json!("tool_use")),
        ]),
        &request(),
        binding().provenance(),
    );
    [streamed, whole]
}

/// `toolset_name` names the toolset a tool belongs to; this projection publishes none, so it is
/// accepted only as `null`.
#[tokio::test]
async fn a_toolset_name_is_accepted_only_as_null() {
    for outcome in tool_use_with("toolset_name", &Value::Null).await {
        let outcome = outcome.expect("a null toolset name");
        assert!(
            matches!(outcome.items.as_slice(), [Item::ToolCall(call)] if call.name.as_str() == "lookup"),
            "{:?}",
            outcome.items
        );
    }
    let toolset = json!("browser");
    for outcome in tool_use_with("toolset_name", &toolset).await {
        refused_by_its_shape(&outcome.expect_err("a toolset member"), &toolset);
    }
}

/// `caller` defaults to `{"type":"direct"}`: the model called the tool itself, which is what a
/// neutral tool call is. A server tool calling it is still outside the declared subset.
#[tokio::test]
async fn a_direct_caller_is_a_tool_call_and_any_other_caller_stays_unsupported() {
    for outcome in tool_use_with("caller", &json!({"type":"direct"})).await {
        let outcome = outcome.expect("the documented default caller");
        assert!(
            matches!(outcome.items.as_slice(), [Item::ToolCall(call)] if call.name.as_str() == "lookup"),
            "{:?}",
            outcome.items
        );
    }
    for caller in [
        json!({"type":"code_execution_20250825","tool_id":"srvtoolu_1"}),
        json!({"type":"direct","tool_id":"srvtoolu_1"}),
        json!("direct"),
    ] {
        for outcome in tool_use_with("caller", &caller).await {
            let error = outcome.expect_err("a caller other than the model");
            assert_eq!(error.code, ErrorCode::Unsupported, "{caller}: {error}");
        }
    }
}

/// `stop_details` is `null` unless the turn stopped with detail to report, a `refusal`. The null is
/// accepted on a message start, a message delta and a complete message; a refusal's details are
/// refused rather than dropped.
#[tokio::test]
async fn stop_details_are_accepted_only_as_null() {
    let details = json!({"type":"refusal","category":null,"explanation":null});
    let mut events = text_turn(json!({"input_tokens":11}), json!({"output_tokens":8}));
    events[0]["message"]["stop_details"] = Value::Null;
    events[4]["delta"]["stop_details"] = Value::Null;
    let (outcome, _) = decode(&events).await;
    outcome.expect("null stop details on a stream");
    decode_message(
        &complete(&[("stop_details", Value::Null)]),
        &request(),
        binding().provenance(),
    )
    .expect("null stop details on a complete message");

    events[4]["delta"]["stop_details"] = details.clone();
    let (outcome, _) = decode(&events).await;
    refused_by_its_shape(&outcome.expect_err("a refusal's details"), &details);
    let error = decode_message(
        &complete(&[("stop_details", details.clone())]),
        &request(),
        binding().provenance(),
    )
    .expect_err("a refusal's details");
    refused_by_its_shape(&error, &details);
}

// ---------------------------------------------------------------------------------------------
// story:messages-empty-turn-with-an-unmodelled-stop-reason (issue 83). A turn that stops for a
// reason this projection does not model and decodes to no item is a classified failure with its
// usage; with any item it is the incomplete turn it always was.
// ---------------------------------------------------------------------------------------------

const WINDOW_WITHOUT_CONTENT: &str =
    "the Messages turn stopped at the model context window without content";
const REFUSAL_WITHOUT_CONTENT: &str = "the Messages turn was refused without content";
const UNMODELLED_WITHOUT_CONTENT: &str =
    "the Messages turn stopped without content for a reason this projection does not model";

/// A reason in no list, as long as the decoder's bound on a reason name allows: 64 bytes.
const LONGEST_REASON: &str = "a_reason_in_no_list_that_is_exactly_as_long_as_the_bound_permits";

/// Every kind of stop reason this projection does not model, with what a turn that carried no
/// content fails as: the code, whether another attempt may answer, and the fixed diagnostic.
fn unmodelled_reasons() -> Vec<(&'static str, ErrorCode, bool, &'static str)> {
    assert_eq!(LONGEST_REASON.len(), 64);
    vec![
        (
            "model_context_window_exceeded",
            ErrorCode::TooLarge,
            false,
            WINDOW_WITHOUT_CONTENT,
        ),
        (
            "refusal",
            ErrorCode::Refused,
            false,
            REFUSAL_WITHOUT_CONTENT,
        ),
        // The two reasons the route documents and this projection gives no row of their own.
        (
            "pause_turn",
            ErrorCode::Unavailable,
            true,
            UNMODELLED_WITHOUT_CONTENT,
        ),
        (
            "stop_sequence",
            ErrorCode::Unavailable,
            true,
            UNMODELLED_WITHOUT_CONTENT,
        ),
        // In no list: one byte, and as many as a reason may have.
        (
            "7",
            ErrorCode::Unavailable,
            true,
            UNMODELLED_WITHOUT_CONTENT,
        ),
        (
            LONGEST_REASON,
            ErrorCode::Unavailable,
            true,
            UNMODELLED_WITHOUT_CONTENT,
        ),
        // A row is its exact name: another case, a longer or a shorter name is in no list.
        (
            "REFUSAL",
            ErrorCode::Unavailable,
            true,
            UNMODELLED_WITHOUT_CONTENT,
        ),
        (
            "model_context_window_exceeded_2",
            ErrorCode::Unavailable,
            true,
            UNMODELLED_WITHOUT_CONTENT,
        ),
        (
            "model_context_window",
            ErrorCode::Unavailable,
            true,
            UNMODELLED_WITHOUT_CONTENT,
        ),
    ]
}

/// One closed content block of each kind a turn can carry, with the item it decodes to.
fn contents() -> Vec<(&'static str, Vec<Value>, Item)> {
    vec![
        (
            "a text block",
            vec![
                json!({"type":"content_block_start","index":0,
                    "content_block":{"type":"text","text":""}}),
                json!({"type":"content_block_delta","index":0,
                    "delta":{"type":"text_delta","text":"Partial"}}),
                json!({"type":"content_block_stop","index":0}),
            ],
            Item::assistant("Partial"),
        ),
        (
            "an empty text block",
            vec![
                json!({"type":"content_block_start","index":0,
                    "content_block":{"type":"text","text":""}}),
                json!({"type":"content_block_stop","index":0}),
            ],
            Item::assistant(""),
        ),
        (
            "a thinking block",
            vec![
                json!({"type":"content_block_start","index":0,
                    "content_block":{"type":"thinking","thinking":"","signature":""}}),
                json!({"type":"content_block_delta","index":0,
                    "delta":{"type":"thinking_delta","thinking":"weighing"}}),
                json!({"type":"content_block_delta","index":0,
                    "delta":{"type":"signature_delta","signature":"sig-1"}}),
                json!({"type":"content_block_stop","index":0}),
            ],
            Item::Opaque {
                provenance: binding().provenance().clone(),
                payload: json!({"type":"thinking","thinking":"weighing","signature":"sig-1"}),
            },
        ),
        (
            "a tool call",
            vec![
                json!({"type":"content_block_start","index":0,
                    "content_block":{"type":"tool_use","id":"call-1","name":"lookup","input":{}}}),
                json!({"type":"content_block_delta","index":0,
                    "delta":{"type":"input_json_delta","partial_json":"{\"query\":\"errors\"}"}}),
                json!({"type":"content_block_stop","index":0}),
            ],
            Item::ToolCall(llm_core::ToolCall {
                call_id: llm_core::CallId::new("call-1").unwrap(),
                name: ToolName::new("lookup").unwrap(),
                arguments: json!({"query":"errors"}),
            }),
        ),
    ]
}

/// A whole stream that stops for `reason` after `content`: the header with eleven fresh input
/// tokens, the content, the reason with the final output count, and the terminal event.
fn stopped(reason: &str, content: &[Value]) -> Vec<Value> {
    let mut events = vec![message_start(json!({"input_tokens":11,
        "cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":1}))];
    events.extend_from_slice(content);
    events.push(message_delta(json!(reason), json!({"output_tokens":2})));
    events.push(json!({"type":"message_stop"}));
    events
}

/// The usage `stopped` reports: the header's input, the delta's output.
fn stopped_usage() -> Usage {
    Usage {
        input_tokens: Some(11),
        output_tokens: Some(2),
        cached_input_tokens: Some(0),
        cache_creation_input_tokens: Some(0),
        reasoning_output_tokens: None,
    }
}

/// Decodes `bytes` through the production framing and decoder, handed over `size` bytes at a time.
async fn decode_in_chunks(bytes: &[u8], size: usize) -> (Result<TurnOutcome, Error>, VecSink) {
    let mut framing = SseDecoder::new(Framing::PayloadsOnly);
    let mut decoder = StreamDecoder::new(binding().provenance().clone());
    let mut sink = VecSink::new(64, 64 * 1024);
    let cancel = Cancel::new();
    for chunk in bytes.chunks(size) {
        for event in framing.push(chunk) {
            let applied = match event {
                Ok(event) => decoder.apply(&event, &mut sink, &cancel).await,
                Err(error) => Err(error),
            };
            if let Err(error) = applied {
                return (Err(decoder.attach(error)), sink);
            }
        }
    }
    if let Err(error) = framing.finish() {
        return (Err(decoder.attach(error)), sink);
    }
    (decoder.finish(&request()), sink)
}

/// The failure of a turn that stopped for `reason` without content, checked against its row:
/// the code, the retry class, dispatch `accepted`, no cause named, and the fixed text, which
/// carries no byte of the reason.
fn assert_row(error: &Error, reason: &str, code: ErrorCode, retriable: bool, message: &str) {
    assert_eq!(
        (error.code, error.dispatch, error.refusal),
        (code, Dispatch::Accepted, None),
        "`{reason}`: {error:?}"
    );
    assert_eq!(
        (error.retriable, error.may_retry()),
        (retriable, retriable),
        "`{reason}`: {error:?}"
    );
    assert_eq!(error.message, message, "`{reason}`");
    assert!(
        !error.message.contains(reason) && !error.to_string().contains(reason),
        "`{reason}` reached the diagnostic: {error}"
    );
}

/// The story's first acceptance line.
#[tokio::test]
async fn a_stream_that_stops_at_the_context_window_without_content_is_too_large_with_its_usage() {
    let (outcome, sink) = decode(&stopped("model_context_window_exceeded", &[])).await;
    let error = outcome.expect_err("an answer without content is no answer");
    assert_row(
        &error,
        "model_context_window_exceeded",
        ErrorCode::TooLarge,
        false,
        WINDOW_WITHOUT_CONTENT,
    );
    let observation = error
        .observation
        .expect("the failure keeps what was reported");
    assert_eq!(observation.usage, Some(stopped_usage()));
    assert!(observation.final_usage);
    assert_eq!(sink.events(), []);
}

/// The story's second acceptance line.
#[tokio::test]
async fn a_stream_that_stops_for_an_unlisted_reason_without_content_is_a_retriable_provider_failure_with_its_usage()
 {
    let (outcome, sink) = decode(&stopped("a_reason_in_no_list", &[])).await;
    let error = outcome.expect_err("an answer without content is no answer");
    assert_row(
        &error,
        "a_reason_in_no_list",
        ErrorCode::Unavailable,
        true,
        UNMODELLED_WITHOUT_CONTENT,
    );
    let observation = error
        .observation
        .expect("the failure keeps what was reported");
    assert_eq!(observation.usage, Some(stopped_usage()));
    assert!(observation.final_usage);
    assert_eq!(sink.events(), []);
}

/// The `refusal` row: refused, final, and no cause named, because a stop reason is not one of
/// the structured codes a `RefusalCause` is read from.
#[tokio::test]
async fn a_stream_that_stops_as_a_refusal_without_content_is_refused_with_no_cause_named() {
    let (outcome, sink) = decode(&stopped("refusal", &[])).await;
    let error = outcome.expect_err("an answer without content is no answer");
    assert_row(
        &error,
        "refusal",
        ErrorCode::Refused,
        false,
        REFUSAL_WITHOUT_CONTENT,
    );
    assert_eq!(error.refusal, None);
    let observation = error
        .observation
        .expect("the failure keeps what was reported");
    assert_eq!(observation.usage, Some(stopped_usage()));
    assert_eq!(sink.events(), []);
}

/// Every row by its own reason name, the stream handed over whole and one byte at a time: the
/// same failure, with the response it names and its usage, and nothing shown.
#[tokio::test]
async fn every_unmodelled_reason_without_content_fails_as_its_row_at_every_split() {
    for (reason, code, retriable, message) in unmodelled_reasons() {
        let bytes = sse(&stopped(reason, &[]));
        let (whole, shown_whole) = decode_in_chunks(&bytes, bytes.len()).await;
        let (by_byte, shown_by_byte) = decode_in_chunks(&bytes, 1).await;
        let (in_blocks, _) = decode_bytes(&bytes).await;
        assert_eq!(whole, by_byte, "`{reason}`");
        assert_eq!(whole, in_blocks, "`{reason}`");
        assert_eq!(shown_whole.events(), [], "`{reason}`");
        assert_eq!(shown_by_byte.events(), [], "`{reason}`");
        let error = whole.expect_err("an answer without content is no answer");
        assert_row(&error, reason, code, retriable, message);
        let observation = error
            .observation
            .expect("the failure keeps what was reported");
        assert_eq!(observation.usage, Some(stopped_usage()), "`{reason}`");
        assert_eq!(
            observation.response_id.as_ref().map(ToString::to_string),
            Some("msg_014a".to_owned()),
            "`{reason}`"
        );
        assert!(observation.final_usage, "`{reason}`");
    }
}

/// The story's third acceptance line, for every reason and every kind of content: with an item
/// the turn is the incomplete turn it was before, under the reason's own name, with final usage.
#[tokio::test]
async fn every_unmodelled_reason_after_content_is_still_an_incomplete_turn_under_its_own_name() {
    for (reason, ..) in unmodelled_reasons() {
        for (kind, content, item) in contents() {
            let bytes = sse(&stopped(reason, &content));
            let (whole, _) = decode_in_chunks(&bytes, bytes.len()).await;
            let (by_byte, _) = decode_in_chunks(&bytes, 1).await;
            assert_eq!(whole, by_byte, "`{reason}` after {kind}");
            let outcome = whole.unwrap_or_else(|error| panic!("`{reason}` after {kind}: {error}"));
            assert_eq!(
                outcome.stop_reason,
                StopReason::Incomplete {
                    reason: reason.to_owned()
                },
                "after {kind}"
            );
            assert_eq!(outcome.items, [item], "`{reason}` after {kind}");
            assert_eq!(outcome.observation.usage, Some(stopped_usage()));
            assert!(outcome.observation.final_usage, "`{reason}` after {kind}");
        }
    }
}

/// The two streams the story names, after a text block.
#[tokio::test]
async fn the_context_window_and_an_unlisted_reason_after_a_text_block_stay_incomplete() {
    for reason in ["model_context_window_exceeded", "a_reason_in_no_list"] {
        let content = contents().remove(0).1;
        let (outcome, sink) = decode(&stopped(reason, &content)).await;
        let outcome = outcome.expect("a turn with content");
        assert_eq!(
            outcome.stop_reason,
            StopReason::Incomplete {
                reason: reason.to_owned()
            }
        );
        assert_eq!(outcome.items, [Item::assistant("Partial")], "`{reason}`");
        assert_eq!(sink.text(), "Partial", "`{reason}`");
    }
}

/// "No content" is "no item was decoded". Both sides of that line, where it differs from "no
/// content block arrived": an event outside the subset that the decoder keeps is an item, so a
/// turn of nothing else is still an incomplete turn carrying it; a keep-alive is no item, so a
/// turn of nothing else fails as its row.
#[tokio::test]
async fn no_content_means_no_item_was_decoded() {
    for (reason, code, retriable, message) in unmodelled_reasons() {
        let kept = json!({"type":"novel_event","detail":"kept"});
        let (outcome, sink) = decode(&stopped(reason, std::slice::from_ref(&kept))).await;
        let outcome = outcome.unwrap_or_else(|error| panic!("`{reason}`: {error}"));
        assert_eq!(
            outcome.stop_reason,
            StopReason::Incomplete {
                reason: reason.to_owned()
            }
        );
        assert_eq!(
            outcome.items,
            [Item::Opaque {
                provenance: binding().provenance().clone(),
                payload: kept,
            }],
            "`{reason}`"
        );
        assert_eq!(sink.events().len(), 1, "`{reason}`: the kept event is said");

        let (outcome, sink) = decode(&stopped(reason, &[json!({"type":"ping"})])).await;
        let error = outcome.expect_err("a keep-alive is no content");
        assert_row(&error, reason, code, retriable, message);
        assert_eq!(sink.events(), [], "`{reason}`");
    }
}

/// A block that opened and never closed is not this failure: the terminal event leaves it open,
/// and that stays the final `protocol` refusal it was, whatever the reason.
#[tokio::test]
async fn a_block_left_open_before_an_unmodelled_reason_is_still_the_protocol_refusal() {
    for (reason, ..) in unmodelled_reasons() {
        let open = json!({"type":"content_block_start","index":0,
            "content_block":{"type":"text","text":""}});
        let (outcome, _) = decode(&stopped(reason, &[open])).await;
        let error = outcome.expect_err("an open block at the terminal event");
        assert_eq!(error.code, ErrorCode::Protocol, "`{reason}`");
        assert_eq!(
            error.message,
            "Messages terminal event leaves a content block open"
        );
        assert!(!error.retriable && !error.may_retry(), "`{reason}`");
    }
}

/// The failure carries exactly the counters the route reported, wherever it reported them: in
/// the header only, in the terminal delta only, in both, or nowhere. Unknown stays unknown,
/// never zero, on the error too.
#[tokio::test]
async fn a_stop_without_content_keeps_the_usage_wherever_the_route_reported_it() {
    let header = json!({"input_tokens":11,"cache_read_input_tokens":4,
        "cache_creation_input_tokens":6,"output_tokens":1});
    let known = |output| Usage {
        input_tokens: Some(21),
        output_tokens: Some(output),
        cached_input_tokens: Some(4),
        cache_creation_input_tokens: Some(6),
        reasoning_output_tokens: None,
    };
    let output_only = Usage {
        input_tokens: None,
        output_tokens: Some(3),
        cached_input_tokens: None,
        cache_creation_input_tokens: None,
        reasoning_output_tokens: None,
    };
    for (reason, code, retriable, message) in unmodelled_reasons() {
        for (place, start, end, usage) in [
            (
                "the header only",
                header.clone(),
                Value::Null,
                Some(known(1)),
            ),
            (
                "the terminal delta only",
                Value::Null,
                json!({"output_tokens":3}),
                Some(output_only.clone()),
            ),
            (
                "both",
                header.clone(),
                json!({"output_tokens":5}),
                Some(known(5)),
            ),
            ("neither", Value::Null, Value::Null, None),
        ] {
            let events = vec![
                message_start(start),
                message_delta(json!(reason), end),
                json!({"type":"message_stop"}),
            ];
            let (outcome, _) = decode(&events).await;
            let error = outcome.expect_err("an answer without content is no answer");
            assert_row(&error, reason, code, retriable, message);
            let observation = error.observation.expect("the failure keeps its binding");
            assert_eq!(observation.usage, usage, "`{reason}`, usage in {place}");
            assert!(observation.final_usage, "`{reason}`, usage in {place}");
        }
    }
}

/// The header's counters with a cost, the usage extension a route reports beside them.
fn costed_start() -> Value {
    json!({"input_tokens":11,"output_tokens":1,"cost":{"total":0.5}})
}

/// A delta's output count with a cost of its own.
fn costed_end() -> Value {
    json!({"output_tokens":2,"cost":0.25})
}

/// The `usage-extension` warnings among `events`, in the order they were shown.
fn extensions(events: &[StreamEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::Warning { code, message } if code == "usage-extension" => {
                Some(message.clone())
            }
            _ => None,
        })
        .collect()
}

/// A turn that fails by this rule shows no usage extension: a cost the header reported, a cost
/// the terminal delta reported, or both are dropped with the attempt, and each row keeps its
/// code and its retry class.
#[tokio::test]
async fn a_stop_without_content_shows_no_usage_extension_and_keeps_its_row() {
    let plain_start = json!({"input_tokens":11,"output_tokens":1});
    let plain_end = json!({"output_tokens":2});
    for (reason, code, retriable, message) in unmodelled_reasons() {
        for (place, start, end) in [
            ("the header", costed_start(), plain_end.clone()),
            ("the terminal delta", plain_start.clone(), costed_end()),
            ("both", costed_start(), costed_end()),
        ] {
            let events = vec![
                message_start(start),
                message_delta(json!(reason), end),
                json!({"type":"message_stop"}),
            ];
            let (outcome, sink) = decode(&events).await;
            let error = outcome.expect_err("an answer without content is no answer");
            assert_row(&error, reason, code, retriable, message);
            assert_eq!(sink.events(), [], "`{reason}`, cost in {place}");
        }
    }
}

/// No event can have been shown before a turn fails by this rule. A cost reported by a delta
/// that names no reason yet, ahead of the terminal delta, is held like the header's: the turn
/// may still stop without content, and when it does nothing was shown and the row is kept.
#[tokio::test]
async fn a_cost_reported_before_the_terminal_delta_is_not_shown_for_a_stop_without_content() {
    for (reason, code, retriable, message) in unmodelled_reasons() {
        let events = vec![
            message_start(costed_start()),
            json!({"type":"message_delta","delta":{},"usage":{"output_tokens":1,"cost":0.125}}),
            message_delta(json!(reason), costed_end()),
            json!({"type":"message_stop"}),
        ];
        let (outcome, sink) = decode(&events).await;
        let error = outcome.expect_err("an answer without content is no answer");
        assert_row(&error, reason, code, retriable, message);
        assert_eq!(sink.events(), [], "`{reason}`");
    }
}

/// The other side: a held cost is not lost by a turn that answers. Content after the terminal
/// delta makes the turn the incomplete turn, and every cost held until then is shown before
/// that content, in the order the route reported it.
#[tokio::test]
async fn a_cost_held_at_the_terminal_delta_is_shown_when_content_follows_it() {
    for (reason, ..) in unmodelled_reasons() {
        let mut events = vec![
            message_start(costed_start()),
            message_delta(json!(reason), costed_end()),
        ];
        events.extend([
            json!({"type":"content_block_start","index":0,
                "content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":0,
                "delta":{"type":"text_delta","text":"Late"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_stop"}),
        ]);
        let (outcome, sink) = decode(&events).await;
        let outcome = outcome.unwrap_or_else(|error| panic!("`{reason}`: {error}"));
        assert_eq!(
            outcome.stop_reason,
            StopReason::Incomplete {
                reason: reason.to_owned()
            }
        );
        assert_eq!(outcome.items, [Item::assistant("Late")], "`{reason}`");
        let shown = sink.events();
        assert_eq!(
            extensions(shown),
            [
                "message_start.message.usage.cost: {\"total\":0.5}",
                "message_delta.usage.cost: 0.25"
            ],
            "`{reason}`"
        );
        assert!(
            matches!(
                shown,
                [
                    StreamEvent::Warning { .. },
                    StreamEvent::Warning { .. },
                    StreamEvent::TextDelta { .. }
                ]
            ),
            "`{reason}`: {shown:?}"
        );
    }
}

/// A turn that ends for a modelled reason without content is an answer, so its costs are shown
/// with the delta that settles it, as before; and a stream that names no reason at all shows
/// them at its terminal event and is the `protocol` refusal it was.
#[tokio::test]
async fn a_cost_is_still_shown_when_the_turn_does_not_fail_by_this_rule() {
    let both = [
        "message_start.message.usage.cost: {\"total\":0.5}",
        "message_delta.usage.cost: 0.25",
    ];
    for (reason, stop) in [
        ("end_turn", StopReason::EndTurn),
        ("max_tokens", StopReason::MaxOutputTokens),
    ] {
        let events = vec![
            message_start(costed_start()),
            message_delta(json!(reason), costed_end()),
            json!({"type":"message_stop"}),
        ];
        let (outcome, sink) = decode(&events).await;
        let outcome = outcome.unwrap_or_else(|error| panic!("`{reason}`: {error}"));
        assert_eq!((outcome.stop_reason, outcome.items), (stop, vec![]));
        assert_eq!(extensions(sink.events()), both, "`{reason}`");
    }
    // The third modelled reason settles it the same way, whatever the turn then decodes to.
    let events = vec![
        message_start(costed_start()),
        message_delta(json!("tool_use"), costed_end()),
        json!({"type":"message_stop"}),
    ];
    let (_, sink) = decode(&events).await;
    assert_eq!(extensions(sink.events()), both, "`tool_use`");
    let events = vec![
        message_start(costed_start()),
        json!({"type":"message_delta","delta":{},"usage":costed_end()}),
        json!({"type":"message_stop"}),
    ];
    let (outcome, sink) = decode(&events).await;
    let error = outcome.expect_err("a terminal message that names no reason");
    assert_eq!(
        (error.code, error.message.as_str(), error.may_retry()),
        (
            ErrorCode::Protocol,
            "Messages terminal message names no reason",
            false
        )
    );
    assert_eq!(extensions(sink.events()), both);
}

/// A stream that held its costs at a `message_delta` after no content has answered. A failure
/// after that delta is final, and it is not the stop without content, so what was held is shown
/// before the failure is returned: the two costs, in the order the route reported them. The
/// delta names a reason this projection does not model, or none yet.
#[tokio::test]
async fn a_failure_after_a_held_cost_answered_shows_what_was_held() {
    let both = [
        "message_start.message.usage.cost: {\"total\":0.5}",
        "message_delta.usage.cost: 0.25",
    ];
    let endings = [
        ("a body that ends", vec![]),
        (
            "an `overloaded_error`",
            vec![json!({"type":"error","error":{"type":"overloaded_error","message":"busy"}})],
        ),
        (
            "a keep-alive outside the subset",
            vec![json!({"type":"ping","extra":1})],
        ),
        (
            "a terminal event outside the subset",
            vec![json!({"type":"message_stop","extra":1})],
        ),
        (
            "a second header",
            vec![message_start(json!({"input_tokens":11}))],
        ),
        (
            "a payload after the terminal event",
            vec![json!({"type":"message_stop"}), json!({"type":"ping"})],
        ),
    ];
    for reason in [
        json!("pause_turn"),
        json!("refusal"),
        json!("model_context_window_exceeded"),
        Value::Null,
    ] {
        let held = [
            message_start(costed_start()),
            message_delta(reason.clone(), costed_end()),
        ];
        for (ending, after) in &endings {
            let (outcome, sink) = decode(&[&held[..], &after[..]].concat()).await;
            let error = outcome.expect_err("the stream fails");
            assert!(!error.may_retry(), "{ending} after {reason}: {error:?}");
            assert_eq!(
                extensions(sink.events()),
                both,
                "{ending} after {reason}: {error:?}"
            );
        }
        // The body ends inside an event. The framing's class is not changed here: the client
        // makes a cut after an answer final (`tests/retry_class.rs`).
        let mut cut = sse(&held);
        cut.extend_from_slice(b"event: content_block_delta\ndata: {\"type\"");
        let (outcome, sink) = decode_bytes(&cut).await;
        let error = outcome.expect_err("the stream is cut");
        assert_eq!(error.code, ErrorCode::Protocol, "a cut after {reason}");
        assert_eq!(
            extensions(sink.events()),
            both,
            "a cut after {reason}: {error:?}"
        );
    }
}

/// The other side: a stream that fails before it answered shows nothing, whatever the failure.
/// The cost its header reported is dropped with the attempt.
#[tokio::test]
async fn a_failure_before_the_stream_answered_shows_no_held_cost() {
    let header = [message_start(costed_start())];
    let endings = [
        ("a body that ends", vec![]),
        (
            "a keep-alive outside the subset",
            vec![json!({"type":"ping","extra":1})],
        ),
        ("a second header", vec![message_start(costed_start())]),
        (
            "an `invalid_request_error`",
            vec![json!({"type":"error","error":{"type":"invalid_request_error","message":"no"}})],
        ),
    ];
    for (ending, after) in &endings {
        let (outcome, sink) = decode(&[&header[..], &after[..]].concat()).await;
        outcome.expect_err("the stream fails");
        assert_eq!(sink.events(), [], "{ending}");
    }
    let mut cut = sse(&header);
    cut.extend_from_slice(b"event: ping\ndata: {\"type\"");
    let (outcome, sink) = decode_bytes(&cut).await;
    outcome.expect_err("the stream is cut");
    assert_eq!(sink.events(), [], "a cut");
}

/// A `message_delta` that is refused is the first payload that answers all the same: the cost
/// the header reported is shown before the refusal, and the refused delta's own is not.
#[tokio::test]
async fn a_refused_delta_shows_the_cost_held_before_it_and_not_its_own() {
    let events = [
        message_start(costed_start()),
        message_delta(json!(7), costed_end()),
    ];
    let (outcome, sink) = decode(&events).await;
    let error = outcome.expect_err("a reason that is no name");
    assert!(!error.may_retry(), "{error:?}");
    assert_eq!(
        extensions(sink.events()),
        ["message_start.message.usage.cost: {\"total\":0.5}"]
    );
}

/// What is held is shown through the caller's sink like any other event: a sink that refuses
/// one, or a caller that cancelled, ends the turn with that failure and the rest stays unshown.
#[tokio::test]
async fn a_held_cost_the_caller_does_not_take_ends_the_turn_as_that_failure() {
    let events = sse(&[
        message_start(costed_start()),
        message_delta(json!("pause_turn"), costed_end()),
        json!({"type":"error","error":{"type":"overloaded_error","message":"busy"}}),
    ]);
    let mut sink = VecSink::new(1, 64 * 1024);
    let outcome = decode_stream(
        &events,
        &request(),
        binding().provenance(),
        &mut sink,
        &Cancel::new(),
    )
    .await;
    let error = outcome.expect_err("the sink takes one event");
    assert_eq!(
        (error.code, error.message.as_str()),
        (ErrorCode::TooLarge, "stream sink event bound reached")
    );
    assert_eq!(
        extensions(sink.events()),
        ["message_start.message.usage.cost: {\"total\":0.5}"]
    );

    let cancel = Cancel::new();
    cancel.cancel();
    let mut sink = VecSink::new(64, 64 * 1024);
    let outcome = decode_stream(
        &events,
        &request(),
        binding().provenance(),
        &mut sink,
        &cancel,
    )
    .await;
    let error = outcome.expect_err("the caller cancelled");
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_eq!(sink.events(), []);
}

/// Held reports are bounded like kept events. As many as a turn may carry items are held and
/// dropped with a turn that then stops without content; one more is refused as `too-large`,
/// which is not that stop, so the reports held until then are shown before it.
#[tokio::test]
async fn held_usage_extensions_are_bounded_by_the_content_bound() {
    let costed = json!({"type":"message_delta","delta":{},
        "usage":{"output_tokens":1,"cost":0.125}});
    let mut events = vec![message_start(json!({"input_tokens":11,"output_tokens":1}))];
    events.extend(std::iter::repeat_n(costed.clone(), llm_core::MAX_ITEMS));
    let mut at_the_bound = events.clone();
    at_the_bound.extend([
        message_delta(json!("pause_turn"), json!({"output_tokens":2})),
        json!({"type":"message_stop"}),
    ]);
    let (outcome, sink) = decode(&at_the_bound).await;
    let error = outcome.expect_err("an answer without content is no answer");
    assert_row(
        &error,
        "pause_turn",
        ErrorCode::Unavailable,
        true,
        UNMODELLED_WITHOUT_CONTENT,
    );
    assert_eq!(sink.events(), []);

    events.push(costed);
    let mut sink = VecSink::new(llm_core::MAX_ITEMS, 1 << 20);
    let outcome = decode_stream(
        &sse(&events),
        &request(),
        binding().provenance(),
        &mut sink,
        &Cancel::new(),
    )
    .await;
    let error = outcome.expect_err("one report past the bound");
    assert_eq!(
        (error.code, error.message.as_str(), error.may_retry()),
        (
            ErrorCode::TooLarge,
            "Messages content exceeds its bound",
            false
        )
    );
    assert_eq!(
        extensions(sink.events()),
        vec!["message_delta.usage.cost: 0.125"; llm_core::MAX_ITEMS]
    );
    assert_eq!(sink.events().len(), llm_core::MAX_ITEMS);
}

/// A complete (non-streamed) message finishes through the same function: without content each
/// reason fails as its row, with its usage; with a block it is the incomplete turn.
#[test]
fn a_complete_message_that_stops_for_an_unmodelled_reason_follows_the_same_rule() {
    for (reason, code, retriable, message) in unmodelled_reasons() {
        let mut answer = json!({
            "id":"msg_014a","type":"message","role":"assistant","model":"example-model-20260201",
            "content":[],"stop_reason":reason,"stop_sequence":null,
            "usage":{"input_tokens":11,"cache_read_input_tokens":4,
                "cache_creation_input_tokens":6,"output_tokens":0}});
        let error = decode_message(&answer, &request(), binding().provenance())
            .expect_err("an answer without content is no answer");
        assert_row(&error, reason, code, retriable, message);
        let observation = error
            .observation
            .expect("the failure keeps what was reported");
        assert_eq!(
            observation.usage,
            Some(Usage {
                input_tokens: Some(21),
                output_tokens: Some(0),
                cached_input_tokens: Some(4),
                cache_creation_input_tokens: Some(6),
                reasoning_output_tokens: None,
            }),
            "`{reason}`"
        );
        assert!(observation.final_usage, "`{reason}`");

        for (kind, block, item) in [
            (
                "a text block",
                json!({"type":"text","text":"Partial"}),
                Item::assistant("Partial"),
            ),
            (
                "an empty text block",
                json!({"type":"text","text":""}),
                Item::assistant(""),
            ),
        ] {
            answer["content"] = json!([block]);
            let outcome = decode_message(&answer, &request(), binding().provenance())
                .unwrap_or_else(|error| panic!("`{reason}` with {kind}: {error}"));
            assert_eq!(
                outcome.stop_reason,
                StopReason::Incomplete {
                    reason: reason.to_owned()
                }
            );
            assert_eq!(outcome.items, [item], "`{reason}` with {kind}");
            assert!(outcome.observation.final_usage, "`{reason}` with {kind}");
        }
    }
}

/// A complete message that reports no usage fails with none: unknown, never zero.
#[test]
fn a_complete_message_without_content_or_usage_fails_with_unknown_usage() {
    for (reason, code, retriable, message) in unmodelled_reasons() {
        let answer = json!({"id":"msg_014a","type":"message","role":"assistant",
            "model":"example-model-20260201","content":[],"stop_reason":reason});
        let error = decode_message(&answer, &request(), binding().provenance())
            .expect_err("an answer without content is no answer");
        assert_row(&error, reason, code, retriable, message);
        assert_eq!(
            error
                .observation
                .expect("the failure keeps its binding")
                .usage,
            None,
            "`{reason}`"
        );
    }
}

/// The modelled reasons are untouched: a turn that ends, or runs out of output, with no content
/// is the outcome it was.
#[tokio::test]
async fn a_modelled_reason_without_content_is_not_this_failure() {
    for (reason, stop) in [
        ("end_turn", StopReason::EndTurn),
        ("max_tokens", StopReason::MaxOutputTokens),
    ] {
        let (outcome, _) = decode(&stopped(reason, &[])).await;
        let outcome = outcome.unwrap_or_else(|error| panic!("`{reason}`: {error}"));
        assert_eq!(outcome.stop_reason, stop);
        assert_eq!(outcome.items, []);
    }
}

// ---------------------------------------------------------------------------------------------
// Adversary, wave 2026-11-25-messages-empty-turn, unit 1
// (story:messages-empty-turn-with-an-unmodelled-stop-reason). A guard for two mutants of
// `stopped_without_content` that every case above passes under: the `refusal` row matched by
// prefix (`r if r.starts_with("refusal")`) and by a shorter name (`r if "refusal".starts_with(r)`).
// `unmodelled_reasons` holds a longer and a shorter name only for the context-window row.
// ---------------------------------------------------------------------------------------------

/// `docs/messages.md`: "The row is chosen by the whole name". A longer name, a shorter one and
/// another case of `refusal` are reasons in no list: `unavailable` and retriable, never `refused`.
#[tokio::test]
async fn adversary_w30_the_refusal_row_is_chosen_by_its_whole_name() {
    for reason in ["refusal_2", "refusals", "refus", "Refusal"] {
        let (outcome, sink) = decode(&stopped(reason, &[])).await;
        let error = outcome.expect_err("an answer without content is no answer");
        assert_row(
            &error,
            reason,
            ErrorCode::Unavailable,
            true,
            UNMODELLED_WITHOUT_CONTENT,
        );
        assert_eq!(sink.events(), [], "`{reason}`");
        let answer = json!({"id":"msg_014a","type":"message","role":"assistant",
            "model":"example-model-20260201","content":[],"stop_reason":reason});
        let error = decode_message(&answer, &request(), binding().provenance())
            .expect_err("an answer without content is no answer");
        assert_row(
            &error,
            reason,
            ErrorCode::Unavailable,
            true,
            UNMODELLED_WITHOUT_CONTENT,
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Adversary, second pass, wave 2026-11-25-messages-empty-turn, unit 1
// (story:messages-empty-turn-with-an-unmodelled-stop-reason). Guards for when a held cost is
// shown. Every case above reads the sink after the whole stream was decoded, and the order of
// the warnings among the other events is the same whether a cost is shown at its delta or held
// to the next content or to `message_stop`. So a decoder that holds every delta's cost, or holds
// one although the turn has content or a modelled reason, passes them; it differs only for the
// caller that watches the stream, and for the stream that is cut before `message_stop`.
// `docs/messages.md`: a `message_delta`'s cost is "shown with that delta, once the whole delta
// was accepted", and one held is shown "at the delta that names `end_turn`, `tool_use` or
// `max_tokens`".
// ---------------------------------------------------------------------------------------------

/// How many `usage-extension` warnings the caller has been shown after each of `events`, applied
/// one at a time.
async fn adversary_w30b_costs_shown_after_each(events: &[Value]) -> Vec<usize> {
    let mut decoder = StreamDecoder::new(binding().provenance().clone());
    let mut sink = VecSink::new(64, 64 * 1024);
    let cancel = Cancel::new();
    let mut shown = Vec::new();
    for event in events {
        let payload = llm_http::SseEvent::Payload {
            event: event["type"].as_str().map(str::to_owned),
            data: event.clone(),
        };
        decoder
            .apply(&payload, &mut sink, &cancel)
            .await
            .unwrap_or_else(|error| panic!("{event}: {error}"));
        shown.push(extensions(sink.events()).len());
    }
    shown
}

/// A turn that holds content shows a delta's cost with that delta, whatever reason it names:
/// after a closed block, while a block is open, and after a kept event.
#[tokio::test]
async fn adversary_w30b_a_delta_after_content_shows_its_cost_with_that_delta() {
    let open = json!({"type":"content_block_start","index":0,
        "content_block":{"type":"text","text":""}});
    for reason in [
        "end_turn",
        "max_tokens",
        "pause_turn",
        "stop_sequence",
        "refusal",
        "model_context_window_exceeded",
    ] {
        let closed = [
            message_start(costed_start()),
            open.clone(),
            json!({"type":"content_block_delta","index":0,
                "delta":{"type":"text_delta","text":"Partial"}}),
            json!({"type":"content_block_stop","index":0}),
            message_delta(json!(reason), costed_end()),
        ];
        assert_eq!(
            adversary_w30b_costs_shown_after_each(&closed).await,
            [0, 1, 1, 1, 2],
            "`{reason}` after a closed block"
        );
        let still_open = [
            message_start(costed_start()),
            open.clone(),
            message_delta(json!(reason), costed_end()),
        ];
        assert_eq!(
            adversary_w30b_costs_shown_after_each(&still_open).await,
            [0, 1, 2],
            "`{reason}` while a block is open"
        );
        let kept = [
            message_start(costed_start()),
            json!({"type":"an_event_in_no_list"}),
            message_delta(json!(reason), costed_end()),
        ];
        assert_eq!(
            adversary_w30b_costs_shown_after_each(&kept).await,
            [0, 1, 2],
            "`{reason}` after a kept event"
        );
    }
}

/// A turn without content shows what it held at the delta that names a modelled reason, each
/// of the three, and not later: the header's cost, a cost of an earlier delta that named no
/// reason, and that delta's own.
#[tokio::test]
async fn adversary_w30b_a_modelled_reason_without_content_shows_the_held_costs_at_its_delta() {
    for reason in ["end_turn", "tool_use", "max_tokens"] {
        let settled = [
            message_start(costed_start()),
            message_delta(json!(reason), costed_end()),
        ];
        assert_eq!(
            adversary_w30b_costs_shown_after_each(&settled).await,
            [0, 2],
            "`{reason}`"
        );
        let after_an_earlier_report = [
            message_start(costed_start()),
            json!({"type":"message_delta","delta":{},"usage":{"output_tokens":1,"cost":0.125}}),
            message_delta(json!(reason), costed_end()),
        ];
        assert_eq!(
            adversary_w30b_costs_shown_after_each(&after_an_earlier_report).await,
            [0, 0, 3],
            "`{reason}` after a delta that named no reason"
        );
    }
}
