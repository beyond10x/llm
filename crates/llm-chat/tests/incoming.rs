//! Pinned Chat Completions responses projected onto the neutral interface.
mod common;

use common::binding;
use llm_chat::{IngressStream, decode_completion, project_response_bytes};
use llm_core::{Dispatch, ErrorCode, Item, StopReason, StreamEvent, TurnOutcome, Usage};
use serde_json::json;

const OPENAI_TEXT: &[u8] = include_bytes!("../fixtures/openai-text-and-usage.sse");
const OPENAI_TOOLS: &[u8] = include_bytes!("../fixtures/openai-tool-calls.sse");
const VLLM_TEXT: &[u8] = include_bytes!("../fixtures/vllm-text-no-usage.sse");
const VLLM_TOOLS: &[u8] = include_bytes!("../fixtures/vllm-tool-call-and-usage.sse");
const PARTIAL_USAGE: &[u8] = include_bytes!("../fixtures/openai-partial-usage.sse");
const LLAMA_TEXT: &[u8] = include_bytes!("../fixtures/llama-server-text-and-usage.sse");
const LLAMA_TOOLS: &[u8] = include_bytes!("../fixtures/llama-server-tool-call-and-usage.sse");
const LLAMA_TIMINGS: &[u8] = include_bytes!("../fixtures/llama-server-text-timings-only.sse");

fn text(value: &str) -> StreamEvent {
    StreamEvent::TextDelta {
        text: value.to_owned(),
    }
}

#[test]
fn a_pinned_openai_stream_preserves_text_usage_and_its_finish_reason() {
    let binding = binding();
    let (events, outcome) = project_response_bytes(OPENAI_TEXT, binding.provenance());
    let outcome = outcome.expect("projected");

    assert_eq!(events, vec![text("Hel"), text("lo, w"), text("orld")]);
    assert_eq!(outcome.items, vec![Item::assistant("Hello, world")]);
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
    assert_eq!(
        outcome
            .observation
            .upstream_model
            .as_ref()
            .map(ToString::to_string),
        Some("gpt-4o-mini-2024-07-18".to_owned())
    );
    assert_eq!(
        outcome
            .observation
            .response_id
            .as_ref()
            .map(ToString::to_string),
        Some("chatcmpl-Bp1x2QfAkT".to_owned())
    );
    assert!(outcome.observation.final_usage);
    assert_eq!(
        outcome.observation.usage,
        Some(Usage {
            input_tokens: Some(31),
            output_tokens: Some(9),
            cached_input_tokens: Some(16),
            // Chat Completions reports no cache-write counter; absent stays absent.
            cache_creation_input_tokens: None,
            reasoning_output_tokens: Some(4),
        })
    );
}

#[test]
fn a_vllm_stream_without_include_usage_leaves_every_counter_absent() {
    let binding = binding();
    let (events, outcome) = project_response_bytes(VLLM_TEXT, binding.provenance());
    let outcome = outcome.expect("projected");

    assert_eq!(
        events,
        vec![
            StreamEvent::ReasoningDelta {
                text: "The user greets me.".to_owned()
            },
            text("Guten "),
            text("Tag"),
        ]
    );
    assert_eq!(outcome.items, vec![Item::assistant("Guten Tag")]);
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
    // `usage: null` on every chunk is an absent report, never a zero one.
    assert_eq!(outcome.observation.usage, None);
    assert!(outcome.observation.final_usage);
}

/// Chat Completions carries a turn's text only as `delta.content`, so the turn's text is the
/// deltas joined and cannot differ from what the caller was shown: on every pinned stream the
/// outcome's text is exactly the text deltas, and no warning says otherwise. The same events,
/// relayed by the ingress encoder, close on the turn.
#[test]
fn the_turn_carries_exactly_the_text_its_deltas_showed() {
    for (fixture, bytes) in [
        ("openai text", OPENAI_TEXT),
        ("openai tools", OPENAI_TOOLS),
        ("vllm text", VLLM_TEXT),
        ("vllm tools", VLLM_TOOLS),
        ("llama-server text", LLAMA_TEXT),
        ("llama-server tools", LLAMA_TOOLS),
        ("llama-server timings", LLAMA_TIMINGS),
    ] {
        let binding = binding();
        let (events, outcome) = project_response_bytes(bytes, binding.provenance());
        let outcome = outcome.unwrap_or_else(|error| panic!("{fixture}: {error:?}"));
        let shown: String = events
            .iter()
            .filter_map(|event| match event {
                StreamEvent::TextDelta { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        let carried: Vec<&str> = outcome
            .items
            .iter()
            .filter_map(|item| match item {
                Item::AssistantText { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        let expected: Vec<&str> = if shown.is_empty() {
            Vec::new()
        } else {
            vec![shown.as_str()]
        };
        assert_eq!(carried, expected, "{fixture}");
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, StreamEvent::Warning { .. })),
            "{fixture}: {events:?}"
        );
        let mut relay = IngressStream::new("chatcmpl-relay", 1_700_000_000);
        for event in &events {
            relay.chunk(event);
        }
        relay.close(&outcome, true).unwrap_or_else(|error| {
            panic!("{fixture}: the relayed stream cannot close on the turn: {error:?}")
        });
    }
}

#[test]
fn a_pinned_tool_stream_preserves_identifiers_names_and_split_arguments() {
    for fixture in [OPENAI_TOOLS, VLLM_TOOLS, LLAMA_TOOLS] {
        let binding = binding();
        let (events, outcome) = project_response_bytes(fixture, binding.provenance());
        let outcome = outcome.expect("projected");
        assert_eq!(outcome.stop_reason, StopReason::ToolCalls);

        let calls: Vec<_> = outcome.tool_calls().collect();
        assert_eq!(calls[0].name.as_str(), "lookup");
        assert!(matches!(calls[0].arguments, serde_json::Value::Object(_)));
        let joined: String = events
            .iter()
            .filter_map(|event| match event {
                StreamEvent::ToolArgumentsDelta { call_id, delta }
                    if call_id == &calls[0].call_id =>
                {
                    Some(delta.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&joined)
                .expect("joined deltas are the arguments"),
            calls[0].arguments
        );
    }
}

/// The tool events of one stream, in order, as `started:<id>:<name>` and `arguments:<id>:<delta>`.
fn tool_stream(events: &[StreamEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::ToolCallStarted { call_id, name } => {
                Some(format!("started:{call_id}:{name}"))
            }
            StreamEvent::ToolArgumentsDelta { call_id, delta } => {
                Some(format!("arguments:{call_id}:{delta}"))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_streamed_call_is_announced_by_name_before_its_first_argument_fragment() {
    let binding = binding();
    let (events, outcome) = project_response_bytes(OPENAI_TOOLS, binding.provenance());
    outcome.expect("projected");
    assert_eq!(
        tool_stream(&events),
        vec![
            "started:call_7Yq:lookup",
            "arguments:call_7Yq:{\"cit",
            "arguments:call_7Yq:y\": \"Ber",
            "arguments:call_7Yq:lin\"}",
            "started:call_9Zb:clock",
            "arguments:call_9Zb:{}",
        ]
    );

    let (events, outcome) = project_response_bytes(VLLM_TOOLS, binding.provenance());
    outcome.expect("projected");
    assert_eq!(
        tool_stream(&events),
        vec![
            "started:chatcmpl-tool-0a1b2c:lookup",
            "arguments:chatcmpl-tool-0a1b2c:{\"city\":",
            "arguments:chatcmpl-tool-0a1b2c: \"Kyoto\"}",
        ]
    );
}

#[test]
fn a_call_is_announced_even_when_its_opening_fragment_carries_no_arguments_at_all() {
    let binding = binding();
    let bytes = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"type\":\"function\",\"function\":{\"name\":\"clock\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    let (events, outcome) = project_response_bytes(bytes.as_bytes(), binding.provenance());
    outcome.expect("projected");
    assert_eq!(tool_stream(&events), vec!["started:call-1:clock"]);
}

#[test]
fn arguments_that_arrive_before_the_name_are_relayed_right_behind_the_announcement() {
    let binding = binding();
    let bytes = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"function\":{\"arguments\":\"{\\\"a\\\":\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"clock\",\"arguments\":\"1}\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    let (events, outcome) = project_response_bytes(bytes.as_bytes(), binding.provenance());
    outcome.expect("projected");
    // Nothing is relayed under a call the caller cannot name, and nothing is lost: every
    // fragment held back until the name arrived follows the announcement, in one delta.
    assert_eq!(
        tool_stream(&events),
        vec!["started:call-1:clock", "arguments:call-1:{\"a\":1}",]
    );
}

/// Feeds framed payloads one at a time, as a caller doing its own framing does, and requires
/// that none of them is refused: the refusals below belong to `finish`, which runs after the
/// counters the endpoint reports last have been read.
fn drive_to_finish(
    payloads: &[serde_json::Value],
) -> (Vec<StreamEvent>, llm_core::Error, Option<llm_core::Usage>) {
    let binding = binding();
    let mut projection = llm_chat::StreamProjection::new(binding.provenance().clone());
    let mut events = Vec::new();
    for data in payloads {
        events.extend(
            projection
                .accept(&llm_http::SseEvent::Payload {
                    event: None,
                    data: data.clone(),
                })
                .expect("no refusal before the stream finishes"),
        );
    }
    projection
        .accept(&llm_http::SseEvent::Done)
        .expect("terminated");
    let error = projection.finish().expect_err("refused");
    (events, error, projection.observation().usage)
}

#[test]
fn a_call_that_is_never_named_is_never_announced_and_refused_only_at_the_end() {
    let (events, error, usage) = drive_to_finish(&[
        json!({"choices": [{"index": 0, "delta": {"tool_calls": [
            {"index": 0, "id": "call-1", "function": {"arguments": "{}"}}]},
            "finish_reason": "tool_calls"}]}),
        json!({"choices": [], "usage": {"prompt_tokens": 40, "completion_tokens": 12}}),
    ]);
    assert_eq!(error.message, "chat tool call carries no name");
    assert_eq!(usage.and_then(|usage| usage.input_tokens), Some(40));
    assert_eq!(tool_stream(&events), [] as [String; 0]);
}

#[test]
fn a_fragment_that_renames_an_announced_call_is_refused_only_at_the_end() {
    let (events, error, usage) = drive_to_finish(&[
        json!({"choices": [{"index": 0, "delta": {"tool_calls": [
            {"index": 0, "id": "call-1", "function": {"name": "clock", "arguments": ""}}]}}]}),
        json!({"choices": [{"index": 0, "delta": {"tool_calls": [
            {"index": 0, "function": {"name": "lookup", "arguments": "{}"}}]}}]}),
        json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]}),
        json!({"choices": [], "usage": {"prompt_tokens": 40, "completion_tokens": 12}}),
    ]);
    assert_eq!(
        error.message,
        "chat tool call fragment renames a call already announced"
    );
    assert_eq!(usage.and_then(|usage| usage.input_tokens), Some(40));
    assert_eq!(
        tool_stream(&events),
        vec!["started:call-1:clock", "arguments:call-1:{}"]
    );
}

#[test]
fn a_no_argument_tool_call_reaches_the_caller_as_an_empty_object() {
    let binding = binding();
    let (_, outcome) = project_response_bytes(OPENAI_TOOLS, binding.provenance());
    let outcome = outcome.expect("projected");
    let calls: Vec<_> = outcome.tool_calls().collect();
    assert_eq!(calls[1].name.as_str(), "clock");
    assert_eq!(calls[1].arguments, json!({}));
}

#[test]
fn an_unreported_upstream_model_is_never_replaced_by_the_configured_one() {
    let binding = binding();
    let bytes = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    let (_, outcome) = project_response_bytes(bytes.as_bytes(), binding.provenance());
    let outcome = outcome.expect("projected");
    assert_eq!(outcome.observation.upstream_model, None);
    assert_eq!(outcome.observation.response_id, None);
    assert_eq!(outcome.observation.usage, None);
    assert_eq!(outcome.observation.binding, *binding.provenance());
}

#[test]
fn a_stream_that_ends_without_a_finish_reason_is_refused_and_keeps_its_prefix() {
    let binding = binding();
    let bytes = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"}}]}\n\n",
        "data: [DONE]\n\n"
    );
    let (events, outcome) = project_response_bytes(bytes.as_bytes(), binding.provenance());
    assert_eq!(events, vec![text("partial")]);
    let error = outcome.expect_err("refused");
    assert_eq!(error.code, ErrorCode::Protocol);
}

#[test]
fn text_observed_before_a_malformed_frame_is_retained() {
    let binding = binding();
    let bytes = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"visible\"}}]}\n\ndata: not-json\n\n";
    let (events, outcome) = project_response_bytes(bytes.as_bytes(), binding.provenance());
    assert_eq!(events, vec![text("visible")]);
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::Protocol);
}

#[test]
fn an_unknown_finish_reason_is_refused_without_quoting_the_upstream_response() {
    let binding = binding();
    let bytes = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"abandoned-by-operator\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    let (_, outcome) = project_response_bytes(bytes.as_bytes(), binding.provenance());
    let error = outcome.expect_err("refused");
    assert_eq!(error.code, ErrorCode::Protocol);
    assert!(
        !error.message.contains("abandoned-by-operator"),
        "{}",
        error.message
    );
}

#[test]
fn a_second_choice_is_refused_rather_than_silently_dropped() {
    let binding = binding();
    let bytes = concat!(
        "data: {\"choices\":[{\"index\":1,\"delta\":{\"content\":\"other\"}}]}\n\n",
        "data: [DONE]\n\n"
    );
    let (_, outcome) = project_response_bytes(bytes.as_bytes(), binding.provenance());
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::Unsupported);
}

#[test]
fn a_length_finish_reason_is_the_neutral_output_limit() {
    let binding = binding();
    let bytes = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"cut\"},\"finish_reason\":\"length\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    let (_, outcome) = project_response_bytes(bytes.as_bytes(), binding.provenance());
    assert_eq!(
        outcome.expect("projected").stop_reason,
        StopReason::MaxOutputTokens
    );
}

#[test]
fn a_content_filter_finish_reason_is_incomplete_with_a_fixed_reason() {
    let binding = binding();
    let bytes = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"content_filter\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    let (_, outcome) = project_response_bytes(bytes.as_bytes(), binding.provenance());
    assert_eq!(
        outcome.expect("projected").stop_reason,
        StopReason::Incomplete {
            reason: "content-filter".to_owned()
        }
    );
}

#[test]
fn contradictory_reported_counters_are_refused() {
    let binding = binding();
    let bytes = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":1,\"prompt_tokens_details\":{\"cached_tokens\":9}}}\n\n",
        "data: [DONE]\n\n"
    );
    let (_, outcome) = project_response_bytes(bytes.as_bytes(), binding.provenance());
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::Protocol);
}

#[test]
fn a_non_streamed_completion_decodes_text_tools_usage_and_its_finish_reason() {
    let binding = binding();
    let body = json!({
        "id": "chatcmpl-N1",
        "object": "chat.completion",
        "created": 1_772_000_400_u64,
        "model": "Qwen/Qwen3-8B",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "Checking.",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "lookup", "arguments": "{\"city\":\"Oslo\"}"}
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {"prompt_tokens": 12, "completion_tokens": 7, "total_tokens": 19}
    });

    let outcome = decode_completion(&body, binding.provenance()).expect("decoded");
    assert_eq!(outcome.stop_reason, StopReason::ToolCalls);
    assert_eq!(outcome.items[0], Item::assistant("Checking."));
    let call = outcome.tool_calls().next().expect("one call");
    assert_eq!(call.name.as_str(), "lookup");
    assert_eq!(call.arguments, json!({"city":"Oslo"}));
    assert_eq!(
        outcome.observation.usage,
        Some(Usage {
            input_tokens: Some(12),
            output_tokens: Some(7),
            cached_input_tokens: None,
            cache_creation_input_tokens: None,
            reasoning_output_tokens: None,
        })
    );
    assert!(outcome.observation.final_usage);
}

#[test]
fn a_non_streamed_completion_without_a_finish_reason_is_refused() {
    let binding = binding();
    let body = json!({
        "choices": [{"index": 0, "message": {"role": "assistant", "content": "x"}}]
    });
    assert_eq!(
        decode_completion(&body, binding.provenance())
            .expect_err("refused")
            .code,
        ErrorCode::Protocol
    );
}

#[test]
fn a_stream_truncated_before_its_terminal_sentinel_is_refused() {
    let binding = binding();
    let bytes = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"all of it\"},\"finish_reason\":\"stop\"}]}\n\n";
    let (events, outcome) = project_response_bytes(bytes.as_bytes(), binding.provenance());
    assert_eq!(events, vec![text("all of it")]);
    // A finish reason is not a terminated stream: the sentinel is the wire's own proof.
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::Protocol);
}

#[test]
fn an_empty_arguments_string_reaches_the_caller_as_an_empty_object() {
    let binding = binding();
    let bytes = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_x\",\"type\":\"function\",\"function\":{\"name\":\"clock\",\"arguments\":\"\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    let (_, outcome) = project_response_bytes(bytes.as_bytes(), binding.provenance());
    let outcome = outcome.expect("projected");
    let call = outcome.tool_calls().next().expect("one call");
    assert_eq!(call.arguments, json!({}));
}

#[test]
fn an_interrupted_stream_reports_a_snapshot_and_never_a_final_report() {
    use llm_chat::StreamProjection;
    use llm_http::{Framing, SseDecoder};
    let binding = binding();
    let mut projection = StreamProjection::new(binding.provenance().clone());
    let mut decoder = SseDecoder::new(Framing::DoneSentinel);
    for framed in decoder.push(b"data: {\"id\":\"chatcmpl-x\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"so far\"}}]}\n\n") {
        projection.accept(&framed.expect("framed")).expect("accepted");
    }
    let observation = projection.observation();
    assert!(!observation.final_usage);
    assert_eq!(observation.usage, None);
    assert_eq!(observation.binding, *binding.provenance());
}

#[test]
fn a_counter_a_present_usage_report_omits_stays_absent_and_never_becomes_zero() {
    let binding = binding();
    let (_, outcome) = project_response_bytes(PARTIAL_USAGE, binding.provenance());
    let outcome = outcome.expect("projected");
    // The report exists, so `usage` is present; every counter it did not name is still
    // unknown. A zero here would claim the model produced nothing.
    assert_eq!(
        outcome.observation.usage,
        Some(Usage {
            input_tokens: Some(5),
            output_tokens: None,
            // Both detail objects are present and neither names the counter read here.
            cached_input_tokens: None,
            cache_creation_input_tokens: None,
            reasoning_output_tokens: None,
        })
    );
}

#[test]
fn a_reported_zero_counter_is_a_report_and_not_an_absence() {
    let binding = binding();
    let bytes = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":0,\"completion_tokens\":0,\"completion_tokens_details\":{\"reasoning_tokens\":0}}}\n\n",
        "data: [DONE]\n\n"
    );
    let (_, outcome) = project_response_bytes(bytes.as_bytes(), binding.provenance());
    assert_eq!(
        outcome.expect("projected").observation.usage,
        Some(Usage {
            input_tokens: Some(0),
            output_tokens: Some(0),
            cached_input_tokens: None,
            cache_creation_input_tokens: None,
            reasoning_output_tokens: Some(0),
        })
    );
}

/// The rule, not its instances. Nothing in the decode direction is reachable before the
/// endpoint has served bytes, so no refusal it produces may report `not-sent` — dispatch is
/// the retry signal, and claiming nothing left would invite a resend of a turn the provider
/// already served. Every refusal this crate can reach from a response is driven here.
///
/// The guarantee is structural rather than clerical: every public entry point routes its
/// `Err` through one place, so a refusal added inside cannot escape the rule even if this
/// table is never extended. That the *set* of entry points is complete is a separate claim,
/// and `every_exported_decode_entry_point_applies_the_dispatch_correction` reads it out of
/// the source rather than restating it here.
#[test]
fn no_refusal_in_the_decode_direction_reports_that_nothing_was_sent() {
    const STOP: &str =
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n";
    let streamed: &[(&str, String)] = &[
        ("a second choice", format!("data: {{\"choices\":[{{\"index\":1,\"delta\":{{}}}}]}}\n\n{STOP}data: [DONE]\n\n")),
        ("two choices in one chunk", "data: {\"choices\":[{\"delta\":{\"content\":\"a\"}},{\"delta\":{\"content\":\"b\"}}]}\n\ndata: [DONE]\n\n".to_owned()),
        ("an index that is not a whole count", "data: {\"choices\":[{\"index\":0.5,\"delta\":{}}]}\n\ndata: [DONE]\n\n".to_owned()),
        ("an unknown finish reason", "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"who-knows\"}]}\n\ndata: [DONE]\n\n".to_owned()),
        ("a tool call delta with no index", "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"id\":\"c1\"}]}}]}\n\ndata: [DONE]\n\n".to_owned()),
        ("arguments before their identifier", "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{}\"}}]}}]}\n\ndata: [DONE]\n\n".to_owned()),
        ("a fragment renaming an announced call", "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"f\"}}]}}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"g\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n".to_owned()),
        ("an unusable response identifier", "data: {\"id\":\"has space\",\"choices\":[]}\n\ndata: [DONE]\n\n".to_owned()),
        ("an unusable tool call identifier", "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"has space\",\"function\":{\"name\":\"f\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n".to_owned()),
        ("a counter that is not a count", format!("{STOP}data: {{\"choices\":[],\"usage\":{{\"prompt_tokens\":\"many\"}}}}\n\ndata: [DONE]\n\n")),
        ("a frame that is not JSON", "data: not-json\n\n".to_owned()),
        ("a stream that never terminated", STOP.to_owned()),
        ("a stream with no finish reason", "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"x\"}}]}\n\ndata: [DONE]\n\n".to_owned()),
        ("a tool call with no identifier", "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"f\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n".to_owned()),
        ("a tool call with no name", "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n".to_owned()),
        ("an unusable tool name", "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"has space\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n".to_owned()),
        ("arguments that are not JSON", "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"type\":\"function\",\"function\":{\"name\":\"f\",\"arguments\":\"nonsense\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n".to_owned()),
        ("counters that contradict", format!("{STOP}data: {{\"choices\":[],\"usage\":{{\"prompt_tokens\":2,\"prompt_tokens_details\":{{\"cached_tokens\":9}}}}}}\n\ndata: [DONE]\n\n")),
        ("cache counters that overflow", format!("{STOP}data: {{\"choices\":[],\"usage\":{{\"prompt_tokens_details\":{{\"cached_tokens\":18446744073709551615,\"cache_creation_input_tokens\":1}}}}}}\n\ndata: [DONE]\n\n")),
        ("timings that contradict the usage report", format!("{STOP}data: {{\"choices\":[],\"usage\":{{\"completion_tokens\":2}},\"timings\":{{\"predicted_n\":3}}}}\n\ndata: [DONE]\n\n")),
        ("a timings counter that is not a count", format!("{STOP}data: {{\"choices\":[],\"timings\":{{\"predicted_n\":\"many\"}}}}\n\ndata: [DONE]\n\n")),
        ("timings prompt counters that overflow", format!("{STOP}data: {{\"choices\":[],\"timings\":{{\"prompt_n\":18446744073709551615,\"cache_n\":1}}}}\n\ndata: [DONE]\n\n")),
        ("a timings duration that is not a number", format!("{STOP}data: {{\"choices\":[],\"timings\":{{\"prompt_ms\":\"slow\"}}}}\n\ndata: [DONE]\n\n")),
    ];
    let binding = binding();
    let mut seen = std::collections::BTreeSet::new();
    for (case, sse) in streamed {
        let (_, outcome) = project_response_bytes(sse.as_bytes(), binding.provenance());
        let error = outcome.unwrap_err();
        assert_ne!(
            error.dispatch,
            Dispatch::NotSent,
            "{case}: {}",
            error.message
        );
        seen.insert(error.message.clone());
    }

    let bodies: &[(&str, serde_json::Value)] = &[
        ("no choices", json!({})),
        (
            "two choices",
            json!({"choices": [{"index": 0}, {"index": 1}]}),
        ),
        (
            "no finish reason",
            json!({"choices": [{"index": 0, "message": {}}]}),
        ),
        (
            "no message",
            json!({"choices": [{"index": 0, "finish_reason": "stop"}]}),
        ),
        (
            "an unusable model",
            json!({"model": "has space", "choices": []}),
        ),
        (
            "a tool call with no name",
            json!({"choices": [{"index": 0, "finish_reason": "tool_calls",
            "message": {"tool_calls": [{"id": "c1", "function": {"arguments": "{}"}}]}}]}),
        ),
        (
            "timings that contradict the usage report",
            json!({"choices": [{"index": 0, "message": {}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 4}, "timings": {"prompt_n": 4, "cache_n": 1}}),
        ),
    ];
    for (case, body) in bodies {
        let error = decode_completion(body, binding.provenance()).unwrap_err();
        assert_ne!(
            error.dispatch,
            Dispatch::NotSent,
            "{case}: {}",
            error.message
        );
        seen.insert(error.message.clone());
    }
    // Named, not counted: a refusal whose wording changes, or a new one any case above can
    // reach, fails here and has to be accounted for rather than quietly joining the set.
    assert_eq!(seen.into_iter().collect::<Vec<_>>(), DECODE_REFUSALS);
}

/// Every refusal the decode direction can reach, in order, as
/// `no_refusal_in_the_decode_direction_reports_that_nothing_was_sent` drives them.
const DECODE_REFUSALS: [&str; 25] = [
    "SSE data is not valid JSON",
    "chat completion carries no choices",
    "chat completion carries no finish reason",
    "chat completion carries other than one choice",
    "chat completion choice carries no message",
    "chat completion choice index is not a whole count",
    "chat completion reported a counter that is not a whole count",
    "chat completion reported a finish reason outside the supported subset",
    "chat completion reported an unusable identifier",
    "chat completion reported an unusable tool call identifier",
    "chat completion reported an unusable tool name",
    "chat completion reported timings whose prompt counters overflow",
    "chat completion returned more than one choice",
    "chat completion stream carried no finish reason",
    "chat completion stream ended before its terminal sentinel",
    "chat completion timings contradict its usage report",
    "chat completion timings field has a shape llama-server does not write: timings.prompt_ms",
    "chat tool call arguments are not valid JSON",
    "chat tool call arguments arrived before their identifier",
    "chat tool call carries no identifier",
    "chat tool call carries no name",
    "chat tool call delta carries no index",
    "chat tool call fragment renames a call already announced",
    "reported cache tokens overflow",
    "reported usage subsets exceed totals",
];

/// The rule for unknown counters, stated as a rule: take a report that names every counter
/// this wire carries, remove exactly one, and only that one becomes unknown. An earlier
/// version of this suite asserted three instances of this and a mutation defaulting
/// `prompt_tokens` to zero survived all of them.
#[test]
fn removing_any_one_reported_counter_leaves_exactly_that_one_unknown() {
    // (the wire path that carries it, the neutral field it lands in)
    const COUNTERS: &[(&[&str], &str)] = &[
        (&["prompt_tokens"], "input_tokens"),
        (&["completion_tokens"], "output_tokens"),
        (
            &["prompt_tokens_details", "cached_tokens"],
            "cached_input_tokens",
        ),
        (
            &["prompt_tokens_details", "cache_creation_input_tokens"],
            "cache_creation_input_tokens",
        ),
        (
            &["completion_tokens_details", "reasoning_tokens"],
            "reasoning_output_tokens",
        ),
    ];
    let complete = json!({
        "prompt_tokens": 11,
        "completion_tokens": 7,
        "prompt_tokens_details": {"cached_tokens": 3, "cache_creation_input_tokens": 2},
        "completion_tokens_details": {"reasoning_tokens": 5}
    });
    let binding = binding();
    let decode = |usage: &serde_json::Value| {
        let body = json!({
            "choices": [{"index": 0, "message": {"content": "x"}, "finish_reason": "stop"}],
            "usage": usage
        });
        serde_json::to_value(
            decode_completion(&body, binding.provenance())
                .expect("decoded")
                .observation
                .usage
                .expect("a report was made"),
        )
        .expect("encodable")
    };

    // The complete report fills every field the neutral value has, so a counter added to
    // `Usage` later and read from this wire forces this table to grow with it.
    let all = decode(&complete);
    let named: std::collections::BTreeSet<&str> =
        COUNTERS.iter().map(|(_, neutral)| *neutral).collect();
    assert_eq!(
        all.as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        named
    );

    for (path, neutral) in COUNTERS {
        let mut usage = complete.clone();
        let (last, parents) = path.split_last().expect("a nonempty path");
        let mut cursor = &mut usage;
        for step in parents {
            cursor = cursor.get_mut(step).expect("a parent object");
        }
        cursor.as_object_mut().expect("an object").remove(*last);

        let observed = decode(&usage);
        assert!(observed.get(neutral).is_none(), "{neutral} was invented");
        for (_, other) in COUNTERS.iter().filter(|(_, other)| other != neutral) {
            assert!(
                observed.get(other).is_some(),
                "{other} was lost while removing {neutral}"
            );
        }
    }
}

/// The set of entry points is read out of the module, not restated beside it.
///
/// The dispatch rule above is applied once per public entry point. A previous revision of
/// this crate believed there were two, applied it to those two, and left `accept` — the
/// entry point `docs/chat.md` documents for a caller that already has framed events —
/// without it, for six refusals. Nothing failed, because the coverage was a sentence.
///
/// So: derive every exported function of the decode module whose signature can carry a
/// neutral `Error`, and require a refusal driven through each. A fourth entry point added
/// to that module fails this case until someone drives it too.
#[test]
fn every_exported_decode_entry_point_applies_the_dispatch_correction() {
    use std::collections::{BTreeMap, BTreeSet};
    const SOURCE: &str = include_str!("../src/incoming.rs");

    let exported: BTreeSet<&str> = SOURCE
        .split("pub fn ")
        .skip(1)
        .filter_map(|chunk| {
            let signature = chunk.split_once('{')?.0;
            if !signature.contains("Error") {
                return None;
            }
            chunk.split(['(', '<', ' ', '\n']).next()
        })
        .collect();
    assert!(!exported.is_empty(), "no entry points were derived");

    let binding = binding();
    let target = binding.provenance();
    let mut driven: BTreeMap<&str, llm_core::Error> = BTreeMap::new();

    let mut projection = llm_chat::StreamProjection::new(target.clone());
    driven.insert(
        "accept",
        projection
            .accept(&llm_http::SseEvent::Payload {
                event: None,
                data: json!({"choices": [{"index": 1, "delta": {}}]}),
            })
            .expect_err("a second choice"),
    );

    driven.insert(
        "finish",
        llm_chat::StreamProjection::new(target.clone())
            .finish()
            .expect_err("a stream that never terminated"),
    );

    driven.insert(
        "decode_completion",
        decode_completion(&json!({}), target).expect_err("a body with no choices"),
    );

    let (_, refused) = project_response_bytes(b"data: not-json\n\n", target);
    driven.insert(
        "project_response_bytes",
        refused.expect_err("a frame that is not JSON"),
    );

    assert_eq!(
        driven.keys().copied().collect::<BTreeSet<_>>(),
        exported,
        "an exported decode entry point has no refusal driven through it"
    );
    for (name, error) in driven {
        assert_ne!(
            error.dispatch,
            Dispatch::NotSent,
            "{name}: {}",
            error.message
        );
    }
}

// ---------------------------------------------------------------- llama-server `timings`

/// llama-server answers a request without `stream_options.include_usage` with no `usage`
/// object, and puts its `timings` on the chunk that carries the finish reason (llama.cpp
/// `7879491cf`, `tools/server/server-task.cpp:494-510`). Those timings count the turn: `prompt_n`
/// prompt tokens processed and `cache_n` reused are its input, `cache_n` the cached part of it,
/// `predicted_n` its output. A counter the server reported is not left unknown.
#[test]
fn a_llama_server_stream_without_a_usage_report_carries_the_counters_its_timings_report() {
    let binding = binding();
    let (events, outcome) = project_response_bytes(LLAMA_TIMINGS, binding.provenance());
    let outcome = outcome.expect("projected");

    assert_eq!(
        events,
        vec![
            text("In"),
            text(" short"),
            text(":"),
            text(" yes"),
            text(",")
        ]
    );
    assert_eq!(outcome.stop_reason, StopReason::MaxOutputTokens);
    assert!(outcome.observation.final_usage);
    assert_eq!(
        outcome.observation.usage,
        Some(Usage {
            input_tokens: Some(2048),
            output_tokens: Some(5),
            cached_input_tokens: Some(0),
            // Neither report names these; absent stays absent.
            cache_creation_input_tokens: None,
            reasoning_output_tokens: None,
        })
    );
}

/// Where llama-server sends both reports — on the last chunk of a stream, or in one
/// non-streamed body — it writes them from the same counters (`server-context.cpp:2251-2258`,
/// `server-common.cpp:139-152`, `server-task.cpp:357-364`). A pair that disagrees has no
/// truthful reading, so it is refused as any contradictory report is, rather than one of the two
/// being believed. A pair that agrees is the one report.
#[test]
fn timings_that_contradict_the_usage_report_are_refused() {
    type Decode<'a> = &'a dyn Fn(&serde_json::Value) -> Result<TurnOutcome, llm_core::Error>;
    let usage = json!({"completion_tokens": 14, "prompt_tokens": 23,
        "prompt_tokens_details": {"cached_tokens": 8}});
    let agreeing = json!({"cache_n": 8, "prompt_n": 15, "predicted_n": 14, "prompt_ms": 143.862});
    let binding = binding();
    let streamed = |timings: &serde_json::Value| {
        let bytes = format!(
            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            json!({"choices": [{"index": 0, "delta": {"content": "hi"}, "finish_reason": "stop"}]}),
            json!({"choices": [], "usage": usage, "timings": timings})
        );
        project_response_bytes(bytes.as_bytes(), binding.provenance()).1
    };
    let whole = |timings: &serde_json::Value| {
        let body = json!({
            "choices": [{"index": 0, "message": {"content": "hi"}, "finish_reason": "stop"}],
            "usage": usage,
            "timings": timings
        });
        decode_completion(&body, binding.provenance())
    };
    let reported = Some(Usage {
        input_tokens: Some(23),
        output_tokens: Some(14),
        cached_input_tokens: Some(8),
        cache_creation_input_tokens: None,
        reasoning_output_tokens: None,
    });
    let decoders: [(&str, Decode); 2] = [("streamed", &streamed), ("non-streamed", &whole)];
    for (wire, decode) in decoders {
        assert_eq!(
            decode(&agreeing)
                .unwrap_or_else(|error| panic!("{wire}: {error:?}"))
                .observation
                .usage,
            reported,
            "{wire}"
        );
        // Each counter moved by one, alone; the cached count also moves the input it is part of.
        for (field, value) in [("cache_n", 9), ("prompt_n", 16), ("predicted_n", 13)] {
            let mut timings = agreeing.clone();
            timings[field] = json!(value);
            let error = decode(&timings).expect_err(field);
            assert_eq!(error.code, ErrorCode::Protocol, "{wire} {field}");
            assert_eq!(error.dispatch, Dispatch::Accepted, "{wire} {field}");
            assert_eq!(
                error.message, "chat completion timings contradict its usage report",
                "{wire} {field}"
            );
        }
    }
}

/// The unknown-counter rule for `timings`, stated as a rule. With no `usage` object, removing
/// one counter from `timings` leaves unknown exactly the neutral counters it is part of — the
/// input needs both of its parts — and never turns one into a zero.
#[test]
fn removing_any_one_timings_counter_leaves_unknown_exactly_what_it_counted() {
    // (the timings counter, the neutral counters that need it)
    const COUNTERS: &[(&str, &[&str])] = &[
        ("prompt_n", &["input_tokens"]),
        ("cache_n", &["input_tokens", "cached_input_tokens"]),
        ("predicted_n", &["output_tokens"]),
    ];
    let complete = json!({
        "cache_n": 3, "prompt_n": 8, "prompt_ms": 80.5, "prompt_per_token_ms": 10.0625,
        "prompt_per_second": 99.378_881_987_577_64, "predicted_n": 5, "predicted_ms": 200.0,
        "predicted_per_token_ms": 50.0, "predicted_per_second": 20.0
    });
    let binding = binding();
    let decode = |timings: &serde_json::Value| {
        let body = json!({
            "choices": [{"index": 0, "message": {"content": "x"}, "finish_reason": "stop"}],
            "timings": timings
        });
        serde_json::to_value(
            decode_completion(&body, binding.provenance())
                .expect("decoded")
                .observation
                .usage
                .expect("a report was made"),
        )
        .expect("encodable")
    };

    assert_eq!(
        decode(&complete),
        json!({"input_tokens": 11, "output_tokens": 5, "cached_input_tokens": 3})
    );
    let named: std::collections::BTreeSet<&str> = COUNTERS
        .iter()
        .flat_map(|(_, neutral)| neutral.iter().copied())
        .collect();
    for (field, needing) in COUNTERS {
        let mut timings = complete.clone();
        timings.as_object_mut().expect("an object").remove(*field);
        let observed = decode(&timings);
        for neutral in &named {
            assert_eq!(
                observed.get(neutral).is_none(),
                needing.contains(neutral),
                "{neutral} after removing {field}: {observed}"
            );
        }
    }
}

/// With `timings_per_token` llama-server sends `timings` on every chunk, its counters growing
/// (`server-context.cpp:2222-2225`). Before the stream ends they are a snapshot, never a final
/// report; the last one, on the usage chunk, is checked against `usage` and agrees with it.
#[test]
fn per_token_timings_are_a_snapshot_until_the_stream_ends() {
    let binding = binding();
    let mut projection = llm_chat::StreamProjection::new(binding.provenance().clone());
    let payload = |data: serde_json::Value| llm_http::SseEvent::Payload { event: None, data };
    projection
        .accept(&payload(json!({
            "choices": [{"index": 0, "delta": {"content": "a"}, "finish_reason": null}],
            "timings": {"cache_n": 0, "prompt_n": 4, "predicted_n": 1}
        })))
        .expect("a chunk");
    let snapshot = projection.observation();
    assert!(!snapshot.final_usage);
    assert_eq!(
        snapshot.usage.and_then(|usage| usage.output_tokens),
        Some(1)
    );
    for data in [
        json!({"choices": [{"index": 0, "delta": {"content": "b"}, "finish_reason": null}],
            "timings": {"cache_n": 0, "prompt_n": 4, "predicted_n": 2}}),
        json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]}),
        json!({"choices": [], "usage": {"completion_tokens": 2, "prompt_tokens": 4,
            "prompt_tokens_details": {"cached_tokens": 0}},
            "timings": {"cache_n": 0, "prompt_n": 4, "predicted_n": 2}}),
    ] {
        projection.accept(&payload(data)).expect("a chunk");
    }
    projection
        .accept(&llm_http::SseEvent::Done)
        .expect("the sentinel");
    let outcome = projection.finish().expect("finished");
    assert!(outcome.observation.final_usage);
    assert_eq!(
        outcome.observation.usage,
        Some(Usage {
            input_tokens: Some(4),
            output_tokens: Some(2),
            cached_input_tokens: Some(0),
            cache_creation_input_tokens: None,
            reasoning_output_tokens: None,
        })
    );
}

/// The `timings` fields llama.cpp `7879491cf` documents and the neutral `Usage` has no place for
/// (`tools/server/server-common.cpp:139-157`): six durations and rates, which it writes as
/// numbers, and two speculative-decoding counts, which it writes as whole counts.
const TIMINGS_UNREAD: [&str; 8] = [
    "prompt_ms",
    "prompt_per_token_ms",
    "prompt_per_second",
    "predicted_ms",
    "predicted_per_token_ms",
    "predicted_per_second",
    "draft_n",
    "draft_n_accepted",
];

/// A non-streamed body carrying `timings` and no `usage`.
fn decode_timings(timings: &serde_json::Value) -> Result<TurnOutcome, llm_core::Error> {
    let body = json!({
        "choices": [{"index": 0, "message": {"content": "x"}, "finish_reason": "stop"}],
        "timings": timings
    });
    decode_completion(&body, binding().provenance())
}

/// Accepted and never read, as `llm-messages` accepts `UNCONSUMED`: every documented field in
/// the shape the server writes changes nothing but what the three counters report. A key the
/// server does not document at that commit passes too, as every unread key on this wire does
/// (`usage.total_tokens`, `prompt_tokens_details.audio_tokens`): a newer server adding one must
/// not make every turn fail.
#[test]
fn the_documented_timings_fields_without_a_neutral_place_are_accepted_and_not_read() {
    let counters = json!({"cache_n": 3, "prompt_n": 8, "predicted_n": 5});
    let expected = Some(Usage {
        input_tokens: Some(11),
        output_tokens: Some(5),
        cached_input_tokens: Some(3),
        cache_creation_input_tokens: None,
        reasoning_output_tokens: None,
    });
    // One value per name, in `TIMINGS_UNREAD`'s order; the array length ties the two together.
    let values: [serde_json::Value; TIMINGS_UNREAD.len()] = [
        json!(80.5),
        json!(10.0625),
        json!(99.378_881_987_577_64),
        // An integral duration is still a number.
        json!(200),
        json!(50.0),
        json!(0.0),
        json!(12),
        json!(9),
    ];
    let mut documented = counters;
    for (field, value) in std::iter::zip(TIMINGS_UNREAD, values) {
        documented[field] = value;
    }
    assert_eq!(
        decode_timings(&documented)
            .expect("the documented shape")
            .observation
            .usage,
        expected
    );

    let mut undocumented = documented;
    undocumented["a_field_a_later_server_adds"] = json!("any value");
    assert_eq!(
        decode_timings(&undocumented)
            .expect("an undocumented key passes, as on the rest of this wire")
            .observation
            .usage,
        expected
    );
}

/// Accepting a name is not ignoring its value. A documented `timings` field in a shape the
/// server never writes says the endpoint is not the one this projection was written against, so
/// it is refused, by the field's name and without its value. `null` is such a shape: every
/// division behind a rate is guarded (`server-common.h:461-475`), so the server never writes
/// the `null` its JSON library makes of a non-finite number. A `timings` that is not an object
/// is refused the same way.
#[test]
fn a_timings_field_in_a_shape_the_server_does_not_write_is_refused_by_name() {
    const VALUE: &str = "slow-and-unquoted";
    let counters = json!({"cache_n": 3, "prompt_n": 8, "predicted_n": 5});
    let mut cases: Vec<(String, serde_json::Value)> = Vec::new();
    for field in TIMINGS_UNREAD {
        // The two speculative-decoding counts are written as whole counts, the rest as numbers.
        let wrong = if field.starts_with("draft_") {
            [json!(VALUE), serde_json::Value::Null, json!(1.5), json!(-1)]
        } else {
            [
                json!(VALUE),
                serde_json::Value::Null,
                json!([1]),
                json!({"ms": 1}),
            ]
        };
        for value in wrong {
            let mut timings = counters.clone();
            timings[field] = value;
            cases.push((format!("timings.{field}"), timings));
        }
    }
    cases.push(("timings".to_owned(), json!(VALUE)));
    cases.push(("timings".to_owned(), json!([3, 8, 5])));

    for (named, timings) in cases {
        let error = decode_timings(&timings).expect_err(&named);
        assert_eq!(error.code, ErrorCode::Protocol, "{named}");
        assert_eq!(error.dispatch, Dispatch::Accepted, "{named}");
        assert_eq!(
            error.message,
            format!(
                "chat completion timings field has a shape llama-server does not write: {named}"
            ),
            "{timings}"
        );
        assert!(!error.message.contains(VALUE), "{}", error.message);
    }
}

/// The class of `adversary_w11_a_contradiction_refusal_does_not_report_the_contradicted_count`,
/// stated as a rule. A chunk refused while its own report is read — its identifier, its model,
/// its `usage`, its `timings`, or the two reports against each other — leaves the evidence
/// exactly as it was before that chunk. A failure carries that evidence (`client.rs` `retain`),
/// so a part kept from a refused report would be a figure the refusal said not to believe.
#[test]
fn a_chunk_refused_while_its_report_is_read_leaves_the_evidence_as_it_was() {
    let report = json!({
        "choices": [],
        "id": "chatcmpl-report",
        "model": "qwen3-1.7b",
        "usage": {"completion_tokens": 14, "prompt_tokens": 23,
            "prompt_tokens_details": {"cached_tokens": 8}},
        "timings": {"cache_n": 8, "prompt_n": 15, "prompt_ms": 143.862, "predicted_n": 14}
    });
    // The report with one part replaced, each refused when the chunk is read; every other part
    // of the chunk is one the report above shows is accepted on its own.
    let refused = [
        ("an unusable identifier", "id", json!("has space")),
        ("an unusable model", "model", json!("has space")),
        (
            "a usage counter that is not a count",
            "usage",
            json!({"prompt_tokens": "many"}),
        ),
        (
            "a timings counter that is not a count",
            "timings",
            json!({"predicted_n": "many"}),
        ),
        (
            "a timings duration that is not a number",
            "timings",
            json!({"cache_n": 8, "prompt_n": 15, "prompt_ms": "slow", "predicted_n": 14}),
        ),
        (
            "timings prompt counters that overflow",
            "timings",
            json!({"prompt_n": u64::MAX, "cache_n": 1}),
        ),
        (
            "timings that contradict usage",
            "timings",
            json!({"cache_n": 8, "prompt_n": 15, "predicted_n": 13}),
        ),
    ];
    let binding = binding();
    let payload = |data: serde_json::Value| llm_http::SseEvent::Payload { event: None, data };
    let opened = || {
        let mut projection = llm_chat::StreamProjection::new(binding.provenance().clone());
        projection
            .accept(&payload(
                json!({"choices": [{"index": 0, "delta": {"content": "hi"}}]}),
            ))
            .expect("a text chunk");
        projection
    };

    // Not vacuous: the report itself is accepted and changes the evidence.
    let mut accepted = opened();
    let before = accepted.observation();
    accepted
        .accept(&payload(report.clone()))
        .expect("the report");
    assert_ne!(accepted.observation(), before);

    for (case, part, value) in refused {
        let mut projection = opened();
        let before = projection.observation();
        let mut chunk = report.clone();
        chunk[part] = value;
        projection.accept(&payload(chunk)).expect_err(case);
        assert_eq!(projection.observation(), before, "{case}");
    }
}
