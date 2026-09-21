//! Pinned Chat Completions responses projected onto the neutral interface.
mod common;

use common::binding;
use llm_chat::{decode_completion, project_response_bytes};
use llm_core::{Dispatch, ErrorCode, Item, StopReason, StreamEvent, Usage};
use serde_json::json;

const OPENAI_TEXT: &[u8] = include_bytes!("../fixtures/openai-text-and-usage.sse");
const OPENAI_TOOLS: &[u8] = include_bytes!("../fixtures/openai-tool-calls.sse");
const VLLM_TEXT: &[u8] = include_bytes!("../fixtures/vllm-text-no-usage.sse");
const VLLM_TOOLS: &[u8] = include_bytes!("../fixtures/vllm-tool-call-and-usage.sse");
const PARTIAL_USAGE: &[u8] = include_bytes!("../fixtures/openai-partial-usage.sse");

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

#[test]
fn a_pinned_tool_stream_preserves_identifiers_names_and_split_arguments() {
    for fixture in [OPENAI_TOOLS, VLLM_TOOLS] {
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
        ("an unusable response identifier", "data: {\"id\":\"has space\",\"choices\":[]}\n\ndata: [DONE]\n\n".to_owned()),
        ("an unusable tool call identifier", "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"has space\",\"function\":{\"name\":\"f\",\"arguments\":\"{}\"}}]}}]}\n\ndata: [DONE]\n\n".to_owned()),
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
    assert_eq!(
        seen.into_iter().collect::<Vec<_>>(),
        [
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
            "chat completion returned more than one choice",
            "chat completion stream carried no finish reason",
            "chat completion stream ended before its terminal sentinel",
            "chat tool call arguments are not valid JSON",
            "chat tool call arguments arrived before their identifier",
            "chat tool call carries no identifier",
            "chat tool call carries no name",
            "chat tool call delta carries no index",
            "reported cache tokens overflow",
            "reported usage subsets exceed totals",
        ]
    );
}

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
