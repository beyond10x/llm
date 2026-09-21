//! Gateway ingress: a Chat Completions client request decoded onto the neutral subset,
//! and a neutral outcome encoded back onto the same wire.
mod common;

use common::binding;
use llm_chat::{
    IngressStream, NAMED_UNSUPPORTED_REQUEST_FIELDS, SUPPORTED_REQUEST_FIELDS,
    decode_ingress_request, encode_ingress_completion,
};
use llm_core::{
    CallId, Dispatch, ErrorCode, Id, Item, StopReason, StreamEvent, ToolCall, ToolChoice, ToolName,
    TurnObservation, TurnOutcome, Usage,
};
use serde_json::{Value, json};

fn outcome_with(usage: Option<Usage>, upstream_model: Option<&str>) -> TurnOutcome {
    let mut observation = TurnObservation::new(binding().provenance().clone());
    observation.usage = usage;
    observation.upstream_model = upstream_model.map(|v| Id::new(v).expect("fixture model"));
    observation.final_usage = true;
    TurnOutcome {
        stop_reason: StopReason::EndTurn,
        items: vec![Item::assistant("Hello, world")],
        observation,
    }
}

#[test]
fn a_chat_request_decodes_onto_the_neutral_subset() {
    let body = json!({
        "model": "code",
        "messages": [
            {"role": "system", "content": "Be brief"},
            {"role": "user", "content": [{"type": "text", "text": "Weather?"}]},
            {"role": "assistant", "content": "Looking.", "tool_calls": [
                {"id": "call-1", "type": "function",
                 "function": {"name": "lookup", "arguments": "{\"city\":\"Oslo\"}"}}
            ]},
            {"role": "tool", "tool_call_id": "call-1", "content": "sunny"}
        ],
        "tools": [{"type": "function", "function": {
            "name": "lookup", "description": "Look up", "parameters": {"type": "object"}
        }}],
        "tool_choice": {"type": "function", "function": {"name": "lookup"}},
        "max_completion_tokens": 512,
        "temperature": 0.25,
        "stream": true,
        "stream_options": {"include_usage": true}
    });

    let ingress = decode_ingress_request(&body).expect("decoded");
    assert!(ingress.stream);
    assert!(ingress.include_usage);
    let request = ingress.request;
    assert_eq!(request.model, "code");
    assert_eq!(request.instructions, "Be brief");
    assert_eq!(request.max_output_tokens, Some(512));
    assert_eq!(request.sampling.temperature, Some(0.25));
    assert_eq!(request.sampling.top_p, None);
    assert_eq!(
        request.tool_choice,
        ToolChoice::Named(ToolName::new("lookup").expect("fixture tool"))
    );
    assert_eq!(
        request.items,
        vec![
            Item::user("Weather?"),
            Item::assistant("Looking."),
            Item::ToolCall(ToolCall {
                call_id: CallId::new("call-1").expect("fixture call"),
                name: ToolName::new("lookup").expect("fixture tool"),
                arguments: json!({"city":"Oslo"}),
            }),
            Item::ToolResult {
                call_id: CallId::new("call-1").expect("fixture call"),
                output: json!("sunny"),
                failed: false,
            },
        ]
    );
    request.validate().expect("the decoded turn is valid");
}

#[test]
fn every_field_outside_the_published_subset_is_refused_by_its_own_name() {
    for field in NAMED_UNSUPPORTED_REQUEST_FIELDS {
        assert!(
            !SUPPORTED_REQUEST_FIELDS.contains(field),
            "{field} is in both lists"
        );
        let mut body = json!({"model": "code", "messages": [{"role": "user", "content": "hi"}]});
        body[*field] = json!(1);
        let error = decode_ingress_request(&body).expect_err("refused");
        assert_eq!(error.code, ErrorCode::Unsupported, "{field}");
        assert!(error.message.contains(field), "{field}: {}", error.message);
    }
}

#[test]
fn an_unrecognized_field_is_refused_without_echoing_the_clients_bytes() {
    let mut body = json!({"model": "code", "messages": [{"role": "user", "content": "hi"}]});
    body["surprise_setting"] = json!("secret-looking-value");
    let error = decode_ingress_request(&body).expect_err("refused");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert!(
        !error.message.contains("secret-looking-value"),
        "{}",
        error.message
    );
}

#[test]
fn a_non_text_content_part_is_refused() {
    let body = json!({
        "model": "code",
        "messages": [{"role": "user", "content": [
            {"type": "image_url", "image_url": {"url": "https://example.invalid/a.png"}}
        ]}]
    });
    assert_eq!(
        decode_ingress_request(&body).expect_err("refused").code,
        ErrorCode::Unsupported
    );
}

#[test]
fn a_tool_choice_of_none_is_refused_because_the_neutral_subset_cannot_express_it() {
    let body = json!({
        "model": "code",
        "messages": [{"role": "user", "content": "hi"}],
        "tool_choice": "none"
    });
    assert_eq!(
        decode_ingress_request(&body).expect_err("refused").code,
        ErrorCode::Unsupported
    );
}

#[test]
fn the_two_output_limits_must_agree() {
    let mut body = json!({
        "model": "code",
        "messages": [{"role": "user", "content": "hi"}],
        "max_tokens": 128,
        "max_completion_tokens": 256
    });
    assert_eq!(
        decode_ingress_request(&body).expect_err("refused").code,
        ErrorCode::InvalidRequest
    );
    body["max_tokens"] = json!(256);
    assert_eq!(
        decode_ingress_request(&body)
            .expect("decoded")
            .request
            .max_output_tokens,
        Some(256)
    );
}

#[test]
fn instructions_must_precede_the_conversation() {
    let body = json!({
        "model": "code",
        "messages": [
            {"role": "user", "content": "hi"},
            {"role": "system", "content": "late"}
        ]
    });
    assert_eq!(
        decode_ingress_request(&body).expect_err("refused").code,
        ErrorCode::Unsupported
    );
}

#[test]
fn an_encoded_response_omits_a_model_the_upstream_never_reported() {
    let body = encode_ingress_completion(&outcome_with(None, None), "chatcmpl-gw-1", 1_772_000_500)
        .expect("encoded");
    assert_eq!(body["id"], json!("chatcmpl-gw-1"));
    assert_eq!(body["object"], json!("chat.completion"));
    assert_eq!(body["created"], json!(1_772_000_500_u64));
    // The configured alias is never substituted for a missing observation, and an
    // absent counter is never reported as zero.
    assert_eq!(body.get("model"), None);
    assert_eq!(body.get("usage"), None);
    assert_eq!(body["choices"][0]["finish_reason"], json!("stop"));
    assert_eq!(
        body["choices"][0]["message"]["content"],
        json!("Hello, world")
    );
}

#[test]
fn an_encoded_response_carries_reported_counters_and_the_reported_model() {
    let usage = Usage {
        input_tokens: Some(31),
        output_tokens: Some(9),
        cached_input_tokens: Some(16),
        cache_creation_input_tokens: None,
        reasoning_output_tokens: Some(4),
    };
    let body = encode_ingress_completion(
        &outcome_with(Some(usage), Some("Qwen/Qwen3-8B")),
        "chatcmpl-gw-2",
        1_772_000_600,
    )
    .expect("encoded");
    assert_eq!(body["model"], json!("Qwen/Qwen3-8B"));
    assert_eq!(
        body["usage"],
        json!({
            "prompt_tokens": 31,
            "completion_tokens": 9,
            "total_tokens": 40,
            "prompt_tokens_details": {"cached_tokens": 16},
            "completion_tokens_details": {"reasoning_tokens": 4}
        })
    );
}

#[test]
fn an_encoded_response_carries_proposed_tool_calls_and_their_finish_reason() {
    let mut outcome = outcome_with(None, None);
    outcome.stop_reason = StopReason::ToolCalls;
    outcome.items.push(Item::ToolCall(ToolCall {
        call_id: CallId::new("call-1").expect("fixture call"),
        name: ToolName::new("lookup").expect("fixture tool"),
        arguments: json!({"city":"Oslo"}),
    }));
    let body =
        encode_ingress_completion(&outcome, "chatcmpl-gw-3", 1_772_000_700).expect("encoded");
    assert_eq!(body["choices"][0]["finish_reason"], json!("tool_calls"));
    assert_eq!(
        body["choices"][0]["message"]["tool_calls"],
        json!([{
            "id": "call-1", "type": "function",
            "function": {"name": "lookup", "arguments": "{\"city\":\"Oslo\"}"}
        }])
    );
}

#[test]
fn streamed_chunks_carry_text_and_the_terminal_chunks_carry_calls_and_usage() {
    let stream = IngressStream::new("chatcmpl-gw-4", 1_772_000_800);
    let chunks: Vec<Value> = [
        StreamEvent::TextDelta {
            text: "Hel".to_owned(),
        },
        StreamEvent::ReasoningDelta {
            text: "thinking".to_owned(),
        },
        StreamEvent::ToolArgumentsDelta {
            call_id: CallId::new("call-1").expect("fixture call"),
            delta: "{\"ci".to_owned(),
        },
    ]
    .iter()
    .filter_map(|event| stream.chunk(event))
    .collect();

    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0]["choices"][0]["delta"]["content"], json!("Hel"));
    assert_eq!(
        chunks[1]["choices"][0]["delta"]["reasoning_content"],
        json!("thinking")
    );

    let mut outcome = outcome_with(
        Some(Usage {
            input_tokens: Some(3),
            output_tokens: Some(4),
            cached_input_tokens: None,
            cache_creation_input_tokens: None,
            reasoning_output_tokens: None,
        }),
        Some("Qwen/Qwen3-8B"),
    );
    outcome.stop_reason = StopReason::ToolCalls;
    outcome.items.push(Item::ToolCall(ToolCall {
        call_id: CallId::new("call-1").expect("fixture call"),
        name: ToolName::new("lookup").expect("fixture tool"),
        arguments: json!({"city":"Oslo"}),
    }));

    let closing = stream.close(&outcome, true).expect("closed");
    assert_eq!(
        closing[0]["choices"][0]["delta"]["tool_calls"],
        json!([{
            "index": 0, "id": "call-1", "type": "function",
            "function": {"name": "lookup", "arguments": "{\"city\":\"Oslo\"}"}
        }])
    );
    assert_eq!(
        closing[1]["choices"][0]["finish_reason"],
        json!("tool_calls")
    );
    assert_eq!(closing[2]["usage"]["prompt_tokens"], json!(3));
    assert_eq!(closing[2]["choices"], json!([]));

    // Without include_usage no usage chunk is produced at all.
    let stream = IngressStream::new("chatcmpl-gw-5", 1_772_000_900);
    assert_eq!(stream.close(&outcome, false).expect("closed").len(), 2);
}

#[test]
fn an_outcome_carrying_opaque_state_cannot_be_encoded_onto_this_wire() {
    let mut outcome = outcome_with(None, None);
    outcome.items.push(Item::Opaque {
        provenance: binding().provenance().clone(),
        payload: json!({"encrypted": "x"}),
    });
    assert_eq!(
        encode_ingress_completion(&outcome, "chatcmpl-gw-6", 1_772_001_000)
            .expect_err("refused")
            .code,
        ErrorCode::Unsupported
    );
}

#[test]
fn a_reported_cache_write_counter_is_named_rather_than_dropped() {
    let usage = Usage {
        input_tokens: Some(10),
        output_tokens: None,
        cached_input_tokens: None,
        cache_creation_input_tokens: Some(4),
        reasoning_output_tokens: None,
    };
    let body = encode_ingress_completion(
        &outcome_with(Some(usage), None),
        "chatcmpl-gw-7",
        1_772_001_100,
    )
    .expect("encoded");
    assert_eq!(
        body["usage"],
        json!({
            "prompt_tokens": 10,
            "prompt_tokens_details": {"cache_creation_input_tokens": 4}
        })
    );
    // Only two known parts make a total; one known part makes none.
    assert_eq!(body["usage"].get("total_tokens"), None);
}

#[test]
fn an_encoded_response_never_turns_an_absent_counter_into_a_reported_zero() {
    let usage = Usage {
        input_tokens: None,
        output_tokens: Some(9),
        cached_input_tokens: None,
        cache_creation_input_tokens: None,
        reasoning_output_tokens: None,
    };
    let body = encode_ingress_completion(
        &outcome_with(Some(usage), None),
        "chatcmpl-gw-8",
        1_772_001_200,
    )
    .expect("encoded");
    // Exactly the one counter that was reported, and nothing standing in for the rest.
    assert_eq!(body["usage"], json!({"completion_tokens": 9}));
}

/// The gateway serves the neutral subset, so a request the outgoing projection would refuse
/// is refused at the door rather than after the client has been told yes. Each case here is
/// a rule `TurnRequest::validate` owns, reached through this wire's own shapes.
#[test]
fn a_request_the_neutral_subset_refuses_is_refused_at_ingress() {
    let cases: &[(&str, Value)] = &[
        (
            "a named tool choice with no published tools",
            json!({"model": "code", "messages": [{"role": "user", "content": "hi"}],
                   "tool_choice": {"type": "function", "function": {"name": "lookup"}}}),
        ),
        (
            "a temperature outside its range",
            json!({"model": "code", "messages": [{"role": "user", "content": "hi"}],
                   "temperature": 5}),
        ),
        (
            "an output limit of zero",
            json!({"model": "code", "messages": [{"role": "user", "content": "hi"}],
                   "max_completion_tokens": 0}),
        ),
        (
            "a tool message answering no call",
            json!({"model": "code", "messages": [
                {"role": "user", "content": "hi"},
                {"role": "tool", "tool_call_id": "call-nobody", "content": "x"}]}),
        ),
        (
            "two calls sharing one identifier",
            json!({"model": "code", "messages": [{"role": "assistant", "tool_calls": [
                {"id": "call-1", "type": "function", "function": {"name": "f", "arguments": "{}"}},
                {"id": "call-1", "type": "function", "function": {"name": "f", "arguments": "{}"}}]}]}),
        ),
        (
            "a model name the neutral vocabulary cannot hold",
            json!({"model": "a model", "messages": [{"role": "user", "content": "hi"}]}),
        ),
    ];
    for (case, body) in cases {
        let Err(error) = decode_ingress_request(body) else {
            panic!("{case} was accepted by the gateway")
        };
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{case}");
        // Nothing has been sent anywhere: an ingress refusal is not dispatch evidence.
        assert_eq!(error.dispatch, Dispatch::NotSent, "{case}");
    }
}

/// The encode half of the counter rule, stated the same way the decode half is.
///
/// Coverage here used to be three exact-equality cases that happened to span the five
/// counters the neutral value carries today. A sixth added to `Usage` would have been
/// dropped on the way out with nothing failing, because nothing tied the cases to the type.
/// This ties them: a fully-known report must place every field of `Usage` somewhere on the
/// wire, and removing any one must remove exactly its own field and nothing else.
#[test]
fn encoding_places_every_counter_the_neutral_value_carries_and_only_those() {
    // (the neutral field, the wire path that must carry it)
    const COUNTERS: &[(&str, &[&str])] = &[
        ("input_tokens", &["prompt_tokens"]),
        ("output_tokens", &["completion_tokens"]),
        (
            "cached_input_tokens",
            &["prompt_tokens_details", "cached_tokens"],
        ),
        (
            "cache_creation_input_tokens",
            &["prompt_tokens_details", "cache_creation_input_tokens"],
        ),
        (
            "reasoning_output_tokens",
            &["completion_tokens_details", "reasoning_tokens"],
        ),
    ];
    let complete = Usage {
        input_tokens: Some(11),
        output_tokens: Some(7),
        cached_input_tokens: Some(3),
        cache_creation_input_tokens: Some(2),
        reasoning_output_tokens: Some(5),
    };
    let encoded = serde_json::to_value(&complete).expect("encodable");
    let known = encoded.as_object().expect("an object");
    // Every field the neutral value can carry is named by the table. A counter added to
    // `Usage` and set here has no wire path until this table grows with it.
    assert_eq!(
        known
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        COUNTERS
            .iter()
            .map(|(neutral, _)| *neutral)
            .collect::<std::collections::BTreeSet<_>>()
    );

    let on_the_wire = |usage: Usage, path: &[&str]| -> Option<Value> {
        let body = encode_ingress_completion(
            &outcome_with(Some(usage), None),
            "chatcmpl-gw-rule",
            1_772_002_000,
        )
        .expect("encoded");
        let mut cursor = body.get("usage")?;
        for step in path {
            cursor = cursor.get(step)?;
        }
        Some(cursor.clone())
    };

    for (neutral, path) in COUNTERS {
        assert!(
            on_the_wire(complete.clone(), path).is_some(),
            "{neutral} reached no field of the wire"
        );
        let mut reduced = encoded.clone();
        reduced.as_object_mut().expect("an object").remove(*neutral);
        let reduced: Usage = serde_json::from_value(reduced).expect("a partial report");
        assert!(
            on_the_wire(reduced.clone(), path).is_none(),
            "{neutral} was reported when it was never known"
        );
        for (other, other_path) in COUNTERS.iter().filter(|(other, _)| other != neutral) {
            assert!(
                on_the_wire(reduced.clone(), other_path).is_some(),
                "{other} was lost while removing {neutral}"
            );
        }
    }
}
