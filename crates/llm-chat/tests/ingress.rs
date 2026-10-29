//! Gateway ingress: a Chat Completions client request decoded onto the neutral subset,
//! and a neutral outcome encoded back onto the same wire.
mod common;

use common::binding;
use llm_chat::{
    IngressStream, NAMED_UNSUPPORTED_REQUEST_FIELDS, SUPPORTED_REQUEST_FIELDS,
    decode_ingress_request, encode_ingress_completion, omitted_advisories, project_request,
};
use llm_core::{
    Advisory, CallId, Controls, Dispatch, ErrorCode, Hints, Id, Item, MAX_TOOL_RESULT_BYTES,
    Protocol, StopReason, StreamEvent, ToolCall, ToolChoice, ToolName, ToolSpec, TurnObservation,
    TurnOutcome, TurnRequest, Usage, Verbosity,
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

/// A text part without its text is refused rather than read as empty; an empty string is text.
#[test]
fn a_text_part_without_text_is_refused() {
    let with = |part: Value| {
        json!({"model": "code", "messages": [{"role": "user", "content": [
            {"type": "text", "text": "Hi"}, part
        ]}]})
    };
    for part in [
        json!({"type": "text"}),
        json!({"type": "text", "text": null}),
    ] {
        let error = decode_ingress_request(&with(part.clone())).expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{part}");
        assert!(error.message.contains("carries no text"), "{error}");
    }
    let decoded = decode_ingress_request(&with(json!({"type": "text", "text": ""})))
        .expect("an empty string is text");
    assert_eq!(decoded.request.items, vec![Item::user("Hi")]);
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

fn call_id(value: &str) -> CallId {
    CallId::new(value).expect("fixture call")
}

fn tool_call_outcome(arguments: Value) -> TurnOutcome {
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
        call_id: call_id("call-1"),
        name: ToolName::new("lookup").expect("fixture tool"),
        arguments,
    }));
    outcome
}

fn started(id: &str, name: &str) -> StreamEvent {
    StreamEvent::ToolCallStarted {
        call_id: call_id(id),
        name: ToolName::new(name).expect("fixture tool"),
    }
}

fn arguments(id: &str, delta: &str) -> StreamEvent {
    StreamEvent::ToolArgumentsDelta {
        call_id: call_id(id),
        delta: delta.to_owned(),
    }
}

fn tool_calls_of(chunk: &Value) -> &Value {
    &chunk["choices"][0]["delta"]["tool_calls"]
}

#[test]
fn streamed_chunks_carry_text_and_reasoning_as_they_arrive() {
    let mut stream = IngressStream::new("chatcmpl-gw-4", 1_772_000_800);
    let chunks: Vec<Value> = [
        StreamEvent::TextDelta {
            text: "Hel".to_owned(),
        },
        StreamEvent::ReasoningDelta {
            text: "thinking".to_owned(),
        },
        StreamEvent::Warning {
            code: "unknown-stream-event".to_owned(),
            message: "preserved".to_owned(),
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
}

#[test]
fn a_streamed_call_is_named_where_it_was_announced_and_not_again_at_the_end() {
    let mut stream = IngressStream::new("chatcmpl-gw-4", 1_772_000_800);
    let chunks: Vec<Value> = [
        started("call-1", "lookup"),
        arguments("call-1", "{\"city\":"),
        arguments("call-1", "\"Oslo\"}"),
    ]
    .iter()
    .filter_map(|event| stream.chunk(event))
    .collect();

    assert_eq!(chunks.len(), 3);
    assert_eq!(
        tool_calls_of(&chunks[0]),
        &json!([{
            "index": 0, "id": "call-1", "type": "function",
            "function": {"name": "lookup", "arguments": ""}
        }])
    );
    assert_eq!(
        tool_calls_of(&chunks[1]),
        &json!([{"index": 0, "function": {"arguments": "{\"city\":"}}])
    );
    assert_eq!(
        tool_calls_of(&chunks[2]),
        &json!([{"index": 0, "function": {"arguments": "\"Oslo\"}"}}])
    );

    // The terminal chunks no longer stand in for the announcement: the finish reason and the
    // counters, and no second copy of a call the client already assembled.
    let closing = stream
        .close(&tool_call_outcome(json!({"city":"Oslo"})), true)
        .expect("closed");
    assert_eq!(closing.len(), 2);
    assert!(closing.iter().all(|chunk| tool_calls_of(chunk).is_null()));
    assert_eq!(
        closing[0]["choices"][0]["finish_reason"],
        json!("tool_calls")
    );
    assert_eq!(closing[1]["usage"]["prompt_tokens"], json!(3));
    assert_eq!(closing[1]["choices"], json!([]));
}

#[test]
fn two_announced_calls_keep_their_own_wire_indices() {
    let mut stream = IngressStream::new("chatcmpl-gw-7", 1_772_000_800);
    let chunks: Vec<Value> = [
        started("call-1", "lookup"),
        started("call-2", "clock"),
        arguments("call-2", "{}"),
        arguments("call-1", "{}"),
    ]
    .iter()
    .filter_map(|event| stream.chunk(event))
    .collect();
    let indices: Vec<&Value> = chunks
        .iter()
        .map(|chunk| &tool_calls_of(chunk)[0]["index"])
        .collect();
    assert_eq!(indices, [&json!(0), &json!(1), &json!(1), &json!(0)]);
}

#[test]
fn an_announced_call_whose_arguments_never_streamed_is_completed_at_the_end() {
    let mut stream = IngressStream::new("chatcmpl-gw-8", 1_772_000_800);
    assert!(stream.chunk(&started("call-1", "lookup")).is_some());
    let closing = stream
        .close(&tool_call_outcome(json!({})), false)
        .expect("closed");
    assert_eq!(closing.len(), 2);
    assert_eq!(
        tool_calls_of(&closing[0]),
        &json!([{"index": 0, "function": {"arguments": "{}"}}])
    );
    assert_eq!(
        closing[1]["choices"][0]["finish_reason"],
        json!("tool_calls")
    );
}

#[test]
fn streamed_arguments_that_contradict_the_outcome_are_refused() {
    let mut stream = IngressStream::new("chatcmpl-gw-9", 1_772_000_800);
    stream.chunk(&started("call-1", "lookup"));
    stream.chunk(&arguments("call-1", "{\"city\":\"Bergen\"}"));
    assert_eq!(
        stream
            .close(&tool_call_outcome(json!({"city":"Oslo"})), false)
            .expect_err("refused")
            .code,
        ErrorCode::Protocol
    );
}

#[test]
fn a_call_the_stream_never_announced_is_emitted_complete_in_the_terminal_chunks() {
    // A model that returns its calls only in the outcome, as an embedded one may, still
    // reaches the client: once, complete, before the finish reason.
    let mut stream = IngressStream::new("chatcmpl-gw-5", 1_772_000_900);
    assert_eq!(stream.chunk(&arguments("call-1", "{\"ci")), None);
    let closing = stream
        .close(&tool_call_outcome(json!({"city":"Oslo"})), true)
        .expect("closed");
    assert_eq!(
        tool_calls_of(&closing[0]),
        &json!([{
            "index": 0, "id": "call-1", "type": "function",
            "function": {"name": "lookup", "arguments": "{\"city\":\"Oslo\"}"}
        }])
    );
    assert_eq!(
        closing[1]["choices"][0]["finish_reason"],
        json!("tool_calls")
    );
    assert_eq!(closing[2]["usage"]["prompt_tokens"], json!(3));

    // Without include_usage no usage chunk is produced at all.
    let stream = IngressStream::new("chatcmpl-gw-6", 1_772_000_900);
    assert_eq!(
        stream
            .close(&tool_call_outcome(json!({"city":"Oslo"})), false)
            .expect("closed")
            .len(),
        2
    );
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

#[test]
fn an_outcome_carrying_unattributed_state_cannot_be_encoded_onto_this_wire() {
    let mut outcome = outcome_with(None, None);
    outcome.items.push(Item::UnattributedOpaque {
        protocol: Protocol::ChatCompletions,
        payload: json!({"encrypted": "x"}),
    });
    let error =
        encode_ingress_completion(&outcome, "chatcmpl-gw-6", 1_772_001_000).expect_err("refused");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(error.message, Item::UNATTRIBUTED_REFUSAL);
}

// ---------------------------------------------------------------- closed levels

/// The refusal a key outside a level's published keys gets: its path, from `llm_core::field_path`.
fn assert_refused_at(body: &Value, path: &str) {
    let error = decode_ingress_request(body).expect_err(path);
    assert_eq!(
        error.code,
        ErrorCode::Unsupported,
        "{path}: {}",
        error.message
    );
    assert_eq!(
        error.message,
        format!("chat request field is outside the published subset: {path}")
    );
    assert_eq!(error.dispatch, Dispatch::NotSent, "{path}");
}

// `json!` borrows its values; taking them by value keeps every call site short.
#[allow(clippy::needless_pass_by_value)]
fn with_messages(messages: Value) -> Value {
    json!({"model": "code", "messages": messages})
}

fn lookup_tool() -> Value {
    json!({"type": "function", "function": {
        "name": "lookup", "description": "Look up", "parameters": {"type": "object"}
    }})
}

fn lookup_call() -> Value {
    json!({"id": "call-1", "type": "function",
           "function": {"name": "lookup", "arguments": "{}"}})
}

#[test]
fn an_unknown_top_level_field_is_refused_naming_it() {
    let mut body = with_messages(json!([{"role": "user", "content": "hi"}]));
    body["surprise_setting"] = json!("a-client-supplied-value");
    assert_refused_at(&body, "surprise_setting");
}

#[test]
fn an_unknown_key_on_a_system_message_is_refused_by_its_path() {
    assert_refused_at(
        &with_messages(json!([{"role": "system", "content": "Be brief", "name": "ops"}])),
        "messages[].name",
    );
}

#[test]
fn an_unknown_key_on_a_developer_message_is_refused_by_its_path() {
    assert_refused_at(
        &with_messages(json!([{"role": "developer", "content": "Be brief", "name": "ops"}])),
        "messages[].name",
    );
}

#[test]
fn an_unknown_key_on_a_user_message_is_refused_by_its_path() {
    assert_refused_at(
        &with_messages(json!([{"role": "user", "content": "hi", "name": "ada"}])),
        "messages[].name",
    );
}

#[test]
fn an_unknown_key_on_an_assistant_message_is_refused_by_its_path() {
    assert_refused_at(
        &with_messages(json!([
            {"role": "user", "content": "hi"},
            {"role": "assistant", "content": "Hello", "audio": {"id": "a-1"}}
        ])),
        "messages[].audio",
    );
}

#[test]
fn an_unknown_key_on_a_tool_message_is_refused_by_its_path() {
    assert_refused_at(
        &with_messages(json!([
            {"role": "assistant", "tool_calls": [lookup_call()]},
            {"role": "tool", "tool_call_id": "call-1", "content": "sunny", "name": "lookup"}
        ])),
        "messages[].name",
    );
}

/// A key the role does not read is outside the subset even when another role reads it.
#[test]
fn a_key_another_role_reads_is_refused_on_a_role_that_does_not() {
    assert_refused_at(
        &with_messages(json!([{"role": "user", "content": "hi", "tool_call_id": "call-1"}])),
        "messages[].tool_call_id",
    );
    assert_refused_at(
        &with_messages(json!([{"role": "user", "content": "hi", "tool_calls": []}])),
        "messages[].tool_calls",
    );
}

#[test]
fn an_unknown_key_on_a_content_part_is_refused_by_its_path() {
    assert_refused_at(
        &with_messages(json!([{"role": "user", "content": [
            {"type": "text", "text": "hi", "cache_control": {"type": "ephemeral"}}
        ]}])),
        "messages[].content[].cache_control",
    );
}

#[test]
fn an_unknown_key_on_a_tool_call_is_refused_by_its_path() {
    let mut call = lookup_call();
    call["index"] = json!(0);
    assert_refused_at(
        &with_messages(json!([{"role": "assistant", "tool_calls": [call]}])),
        "messages[].tool_calls[].index",
    );
}

#[test]
fn an_unknown_key_on_a_tool_call_function_is_refused_by_its_path() {
    let mut call = lookup_call();
    call["function"]["strict"] = json!(true);
    assert_refused_at(
        &with_messages(json!([{"role": "assistant", "tool_calls": [call]}])),
        "messages[].tool_calls[].function.strict",
    );
}

#[test]
fn an_unknown_key_on_a_tool_is_refused_by_its_path() {
    let mut body = with_messages(json!([{"role": "user", "content": "hi"}]));
    let mut tool = lookup_tool();
    tool["cache_control"] = json!({"type": "ephemeral"});
    body["tools"] = json!([tool]);
    assert_refused_at(&body, "tools[].cache_control");
}

#[test]
fn an_unknown_key_on_a_tool_function_is_refused_by_its_path() {
    let mut body = with_messages(json!([{"role": "user", "content": "hi"}]));
    let mut tool = lookup_tool();
    tool["function"]["foo"] = json!(1);
    body["tools"] = json!([tool]);
    assert_refused_at(&body, "tools[].function.foo");
}

#[test]
fn an_unknown_key_on_a_tool_choice_is_refused_by_its_path() {
    let mut body = with_messages(json!([{"role": "user", "content": "hi"}]));
    body["tools"] = json!([lookup_tool()]);
    body["tool_choice"] = json!({"type": "function", "function": {"name": "lookup"}, "foo": 1});
    assert_refused_at(&body, "tool_choice.foo");
}

#[test]
fn an_unknown_key_on_a_tool_choice_function_is_refused_by_its_path() {
    let mut body = with_messages(json!([{"role": "user", "content": "hi"}]));
    body["tools"] = json!([lookup_tool()]);
    body["tool_choice"] = json!({"type": "function", "function": {"name": "lookup", "foo": 1}});
    assert_refused_at(&body, "tool_choice.function.foo");
}

#[test]
fn an_unknown_stream_option_is_refused_by_its_path() {
    let mut body = with_messages(json!([{"role": "user", "content": "hi"}]));
    body["stream"] = json!(true);
    body["stream_options"] = json!({"include_usage": true, "foo": true});
    assert_refused_at(&body, "stream_options.foo");
}

/// A key name is caller text: only a bounded name of the wire's own shape is copied.
#[test]
fn a_key_that_is_not_a_field_name_is_refused_without_copying_it() {
    assert_refused_at(
        &with_messages(json!([{"role": "user", "content": "hi", "a secret value": 1}])),
        "messages[].?",
    );
}

/// Every key the design publishes, at every level, decodes; closing a level refuses only what
/// lies outside it.
#[test]
fn every_published_key_at_every_level_is_accepted() {
    let mut tool = lookup_tool();
    tool["function"]["strict"] = json!(false);
    let body = json!({
        "model": "code",
        "messages": [
            {"role": "system", "content": "Be brief"},
            {"role": "developer", "content": [{"type": "text", "text": "Use tools"}]},
            {"role": "user", "content": [{"type": "text", "text": "Weather?"}]},
            {"role": "assistant", "content": null, "tool_calls": [lookup_call()]},
            {"role": "tool", "tool_call_id": "call-1", "content": "sunny"},
            {"role": "assistant", "content": "Sunny.", "tool_calls": null}
        ],
        "tools": [tool],
        "tool_choice": {"type": "function", "function": {"name": "lookup"}},
        "stream": true,
        "stream_options": {"include_usage": true}
    });
    let ingress = decode_ingress_request(&body).expect("decoded");
    assert_eq!(ingress.request.instructions, "Be brief\nUse tools");
    assert_eq!(
        ingress.request.items,
        vec![
            Item::user("Weather?"),
            Item::ToolCall(ToolCall {
                call_id: call_id("call-1"),
                name: ToolName::new("lookup").expect("fixture tool"),
                arguments: json!({}),
            }),
            Item::ToolResult {
                call_id: call_id("call-1"),
                output: json!("sunny"),
                failed: false,
            },
            Item::assistant("Sunny."),
        ]
    );
}

#[test]
fn a_strict_function_schema_is_refused_and_a_non_boolean_strict_is_invalid() {
    let mut body = with_messages(json!([{"role": "user", "content": "hi"}]));
    let mut tool = lookup_tool();
    tool["function"]["strict"] = json!(true);
    body["tools"] = json!([tool.clone()]);
    let error = decode_ingress_request(&body).expect_err("strict refused");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        error.message,
        "chat strict function schemas are outside the published subset"
    );

    tool["function"]["strict"] = json!("yes");
    body["tools"] = json!([tool]);
    assert_eq!(
        decode_ingress_request(&body)
            .expect_err("not a boolean")
            .code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn a_tool_call_whose_type_is_not_function_is_refused() {
    let mut call = lookup_call();
    call["type"] = json!("custom");
    let error = decode_ingress_request(&with_messages(json!([
        {"role": "assistant", "tool_calls": [call]}
    ])))
    .expect_err("refused");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        error.message,
        "chat tool call type is outside the published subset"
    );
}

#[test]
fn a_tool_choice_whose_type_is_not_function_is_refused() {
    let mut body = with_messages(json!([{"role": "user", "content": "hi"}]));
    body["tools"] = json!([lookup_tool()]);
    body["tool_choice"] = json!({"type": "custom", "function": {"name": "lookup"}});
    let error = decode_ingress_request(&body).expect_err("refused");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        error.message,
        "chat tool choice type is outside the published subset"
    );
}

/// A value of the wrong type at a key the subset reads is refused, never read as absent.
#[test]
fn a_published_key_of_the_wrong_type_is_refused_rather_than_dropped() {
    let mut call = lookup_call();
    call["function"]["arguments"] = json!({"city": "Oslo"});
    let mut tool = lookup_tool();
    tool["function"]["description"] = json!(7);
    let cases: &[(&str, Value)] = &[
        (
            "reasoning_effort",
            json!({"model": "code", "messages": [{"role": "user", "content": "hi"}],
                   "reasoning_effort": 3}),
        ),
        (
            "tool_calls",
            with_messages(json!([{"role": "assistant", "content": "x", "tool_calls": 5}])),
        ),
        (
            "content part text",
            with_messages(json!([{"role": "user", "content": [{"type": "text", "text": 5}]}])),
        ),
        (
            "arguments",
            with_messages(json!([{"role": "assistant", "tool_calls": [call]}])),
        ),
        (
            "description",
            json!({"model": "code", "messages": [{"role": "user", "content": "hi"}],
                   "tools": [tool]}),
        ),
        (
            "parallel_tool_calls",
            json!({"model": "code", "messages": [{"role": "user", "content": "hi"}],
                   "tools": [lookup_tool()], "parallel_tool_calls": "yes"}),
        ),
        (
            "prompt_cache_key",
            json!({"model": "code", "messages": [{"role": "user", "content": "hi"}],
                   "prompt_cache_key": 42}),
        ),
        (
            "verbosity",
            json!({"model": "code", "messages": [{"role": "user", "content": "hi"}],
                   "verbosity": 1}),
        ),
    ];
    for (case, body) in cases {
        let Err(error) = decode_ingress_request(body) else {
            panic!("{case} of the wrong type was accepted")
        };
        assert_eq!(
            error.code,
            ErrorCode::InvalidRequest,
            "{case}: {}",
            error.message
        );
    }
}

#[test]
fn a_verbosity_outside_low_medium_and_high_is_refused() {
    let mut body = with_messages(json!([{"role": "user", "content": "hi"}]));
    body["verbosity"] = json!("loud");
    let error = decode_ingress_request(&body).expect_err("refused");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert!(!error.message.contains("loud"), "{}", error.message);
}

// ---------------------------------------------------------------- carried options

#[test]
fn parallel_tool_calls_prompt_cache_key_and_verbosity_are_carried() {
    let mut body = with_messages(json!([{"role": "user", "content": "hi"}]));
    body["tools"] = json!([lookup_tool()]);
    body["parallel_tool_calls"] = json!(false);
    body["prompt_cache_key"] = json!("session-7");
    body["verbosity"] = json!("low");
    let request = decode_ingress_request(&body).expect("decoded").request;
    assert_eq!(
        request.controls,
        Controls {
            thinking: None,
            return_unpublished_calls: false,
            parallel_tool_calls: Some(false),
            hints: Hints {
                prompt_cache_key: Some("session-7".to_owned()),
                verbosity: Some(Verbosity::Low),
                ..Hints::default()
            },
        }
    );
    // Both carried hints are sent by this wire, so nothing is omitted.
    assert_eq!(omitted_advisories(&request), Vec::<Advisory>::new());
}

#[test]
fn parallel_tool_calls_left_the_named_unsupported_list() {
    assert!(!NAMED_UNSUPPORTED_REQUEST_FIELDS.contains(&"parallel_tool_calls"));
    for field in ["parallel_tool_calls", "prompt_cache_key", "verbosity"] {
        assert!(SUPPORTED_REQUEST_FIELDS.contains(&field), "{field}");
    }
}

#[test]
fn explicit_null_options_are_absent() {
    let mut body = with_messages(json!([{"role": "user", "content": "hi"}]));
    body["parallel_tool_calls"] = Value::Null;
    body["prompt_cache_key"] = Value::Null;
    body["verbosity"] = Value::Null;
    let request = decode_ingress_request(&body).expect("decoded").request;
    assert!(request.controls.is_empty(), "{:?}", request.controls);
}

/// The turn the gateway decodes projects onto this wire again, and the projected body decodes
/// back to the same turn. The wire names the binding's upstream model, so only `model` differs.
#[test]
fn each_carried_option_survives_a_round_trip_through_the_projection() {
    let options: &[(&str, Value)] = &[
        ("parallel_tool_calls", json!(true)),
        ("parallel_tool_calls", json!(false)),
        ("prompt_cache_key", json!("session-7")),
        ("verbosity", json!("low")),
        ("verbosity", json!("medium")),
        ("verbosity", json!("high")),
    ];
    for (field, value) in options {
        let mut body = with_messages(json!([{"role": "user", "content": "hi"}]));
        body["model"] = json!("small");
        body["tools"] = json!([lookup_tool()]);
        body[*field] = value.clone();
        let request = decode_ingress_request(&body).expect("decoded").request;
        assert!(!request.controls.is_empty(), "{field} was not carried");

        let projected = project_request(&request, &binding(), false).expect("projected");
        assert_eq!(projected[*field], *value, "{field} was not re-emitted");

        let mut back = decode_ingress_request(&projected)
            .expect("the projected body decodes")
            .request;
        assert_eq!(back.model, "Qwen/Qwen3-8B");
        back.model = request.model.clone();
        assert_eq!(back, request, "{field}");
    }
}

// ---------------------------------------------------------------- tool results

/// A turn answering one `lookup` call with a failed result carrying `output`.
fn failed_result_turn(output: Value) -> TurnRequest {
    let mut request = TurnRequest::new(
        "small",
        vec![
            Item::ToolCall(ToolCall {
                call_id: call_id("call-1"),
                name: ToolName::new("lookup").expect("fixture tool"),
                arguments: json!({}),
            }),
            Item::ToolResult {
                call_id: call_id("call-1"),
                output,
                failed: true,
            },
        ],
    );
    request.tools = vec![ToolSpec {
        name: ToolName::new("lookup").expect("fixture tool"),
        description: "Look up".to_owned(),
        input_schema: json!({"type": "object"}),
    }];
    request
}

/// A request whose `tool` message answers `lookup_call()` with `content` as sent.
fn tool_message_body(content: &Value) -> Value {
    let mut body = with_messages(json!([
        {"role": "user", "content": "Weather?"},
        {"role": "assistant", "content": null, "tool_calls": [lookup_call()]},
        {"role": "tool", "tool_call_id": "call-1", "content": content}
    ]));
    body["tools"] = json!([lookup_tool()]);
    body
}

/// The result a `tool` message answering `lookup_call()` decodes to, `content` as sent.
fn decoded_result(content: &Value) -> Item {
    decode_ingress_request(&tool_message_body(content))
        .expect("decoded")
        .request
        .items
        .pop()
        .expect("the tool result")
}

/// The Chat encoder sends a failed result as `{"error": <output>, "ok": false}`, and ingress
/// reads that object back as the failure it was, with the output the caller gave, of any type.
#[test]
fn a_failed_result_the_chat_encoder_sent_decodes_back_failed_with_its_output() {
    for output in [
        json!("no such city"),
        json!(""),
        json!("{\"city\":\"Oslo\"}"),
        json!({"code": 404, "reason": "missing"}),
        json!({"error": "nested", "ok": false}),
        json!(["a", 1]),
        json!(7),
        json!(false),
        Value::Null,
    ] {
        let request = failed_result_turn(output.clone());
        let projected = project_request(&request, &binding(), false).expect("projected");
        let back = decode_ingress_request(&projected)
            .expect("the projected body decodes")
            .request;
        assert_eq!(back.items, request.items, "{output}");
    }
}

/// The failure object is read by its parsed shape: key order, spacing and how the content was
/// split into text parts do not change it.
#[test]
fn a_tool_message_carrying_the_failure_object_decodes_as_failed() {
    let cases = [
        (
            json!(r#"{"error":"no such city","ok":false}"#),
            json!("no such city"),
        ),
        (
            json!(r#"{ "ok": false, "error": {"code": 404} }"#),
            json!({"code": 404}),
        ),
        (
            json!("\n  {\"error\":\"no such city\",\"ok\":false}\n"),
            json!("no such city"),
        ),
        (
            json!([
                {"type": "text", "text": "{\"error\":\"no such"},
                {"type": "text", "text": " city\",\"ok\":false}"}
            ]),
            json!("no such city"),
        ),
    ];
    for (content, output) in cases {
        assert_eq!(
            decoded_result(&content),
            Item::ToolResult {
                call_id: call_id("call-1"),
                output,
                failed: true,
            },
            "{content}"
        );
    }
}

/// Only an object of exactly `error` and `ok`, `ok` being `false`, is a failure. Any other
/// content, JSON or not, is a successful result carrying that content as a string.
#[test]
fn tool_content_that_is_not_exactly_the_failure_object_stays_a_successful_string() {
    for content in [
        r#"{"ok":false}"#,
        r#"{"error":"no such city"}"#,
        r#"{"error":"no such city","ok":false,"code":404}"#,
        r#"{"error":"no such city","ok":true}"#,
        r#"{"error":"no such city","ok":"false"}"#,
        r#"{"error":"no such city","ok":null}"#,
        r#"{"ok":false,"output":"no such city"}"#,
        r#"[{"error":"no such city","ok":false}]"#,
        // The two values in order, as a sequence rather than the object.
        r#"["no such city",false]"#,
        r#""{\"error\":\"no such city\",\"ok\":false}""#,
        r#"{"error":"no such city","ok":false"#,
        "false",
        "null",
        "42",
        "sunny",
        "",
    ] {
        assert_eq!(
            decoded_result(&json!(content)),
            Item::ToolResult {
                call_id: call_id("call-1"),
                output: json!(content),
                failed: false,
            },
            "{content}"
        );
    }
}

/// The failure object `{"error":"<text>","ok":false}` around `length` bytes of text.
fn failure_object_around(length: usize) -> String {
    format!(r#"{{"error":"{}","ok":false}}"#, "a".repeat(length))
}

/// Content is read as the failure object only up to the tool-result bound. Longer content is
/// not parsed first: it is left to the bound, which refuses it as at the base.
#[test]
fn the_failure_object_is_read_up_to_the_result_bound_and_left_to_the_bound_past_it() {
    let frame = failure_object_around(0).len();

    let at_bound = failure_object_around(MAX_TOOL_RESULT_BYTES - frame);
    assert_eq!(at_bound.len(), MAX_TOOL_RESULT_BYTES);
    assert_eq!(
        decoded_result(&json!(at_bound)),
        Item::ToolResult {
            call_id: call_id("call-1"),
            output: json!("a".repeat(MAX_TOOL_RESULT_BYTES - frame)),
            failed: true,
        }
    );

    let past_bound = failure_object_around(MAX_TOOL_RESULT_BYTES - frame + 1);
    assert_eq!(past_bound.len(), MAX_TOOL_RESULT_BYTES + 1);
    let error = decode_ingress_request(&tool_message_body(&json!(past_bound)))
        .expect_err("content past the bound is refused");
    assert_eq!(error.code, ErrorCode::TooLarge, "{}", error.message);
}

/// What `docs/chat.md` states the bound costs: the failure object adds 21 bytes around the
/// output's encoding, so a failed output encoded within 21 bytes of the bound is sent, and its
/// failure object, past the bound, is refused by ingress as too large. One byte shorter, it
/// round-trips failed.
#[test]
fn a_failed_output_within_the_failure_objects_frame_of_the_bound_is_sent_and_refused_back() {
    // A string of `length` bytes encodes to `length + 2`, its quotes.
    for (length, round_trips) in [
        (MAX_TOOL_RESULT_BYTES - 21 - 2, true),
        (MAX_TOOL_RESULT_BYTES - 20 - 2, false),
    ] {
        let request = failed_result_turn(json!("a".repeat(length)));
        let projected = project_request(&request, &binding(), false).expect("projected");
        let back = decode_ingress_request(&projected);
        if round_trips {
            assert_eq!(back.expect("decodes").request.items, request.items);
        } else {
            let error = back.expect_err("the failure object is past the bound");
            assert_eq!(error.code, ErrorCode::TooLarge, "{}", error.message);
        }
    }
}
