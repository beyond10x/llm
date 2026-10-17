//! The Responses ingress encoder, read back by this crate's own decoder.
//!
//! A server answering a Responses caller turns neutral `StreamEvent`s and a `TurnOutcome` into the
//! event sequence and the response object this wire carries. Every case here decodes what the
//! encoder produced with `decode_stream` on the binding the outcome names, so "the encoder emits
//! what the decoder accepts" is checked by the decoder, not restated.

use llm_core::{
    CallId, Dispatch, Error, ErrorCode, Id, Item, MAX_ITEMS, Protocol, Provenance, StopReason,
    StreamEvent, ToolCall, ToolName, TurnObservation, TurnOutcome, Usage,
};
use llm_responses::{
    ACCEPTED_STREAM_EVENTS, Binding, IngressStream, decode_stream, encode_ingress_response,
    ingest_request,
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

/// Encrypted reasoning as the decoder binds it: the output item, verbatim, under the binding.
fn reasoning() -> Item {
    Item::Opaque {
        provenance: provenance(),
        payload: json!({
            "type": "reasoning",
            "id": "rs_upstream_1",
            "summary": [{"type": "summary_text", "text": "Need the time and the file."}],
            "encrypted_content": "gAAAAABfixture"
        }),
    }
}

fn full_usage() -> Usage {
    Usage {
        input_tokens: Some(1788),
        output_tokens: Some(115),
        cached_input_tokens: Some(1680),
        cache_creation_input_tokens: None,
        reasoning_output_tokens: Some(42),
    }
}

fn observation() -> TurnObservation {
    TurnObservation {
        binding: provenance(),
        upstream_model: Some(id("example/Small-Model")),
        // The caller's id, which every payload of an answer carries in place of the upstream's
        // own `response_id`; the decoder reads it back as the response id, so a round trip
        // compares equal only when the encoded outcome carries it too.
        response_id: Some(id("resp_gateway_1")),
        usage: Some(full_usage()),
        final_usage: true,
    }
}

fn outcome(stop_reason: StopReason, items: Vec<Item>) -> TurnOutcome {
    TurnOutcome {
        stop_reason,
        items,
        observation: observation(),
    }
}

/// A client body a gateway receives, publishing the two tools the outcomes below call.
fn client_body() -> Value {
    json!({
        "model": "example/Small-Model",
        "input": [{"type": "message", "role": "user",
                   "content": [{"type": "input_text", "text": "What time is it, and what is in README.md?"}]}],
        "tools": [
            {"type": "function", "name": "clock", "description": "Now.",
             "parameters": {"type": "object"}, "strict": false},
            {"type": "function", "name": "file_read", "description": "Read a file.",
             "parameters": {"type": "object"}, "strict": false}
        ],
        "stream": true,
        "store": false,
        "include": ["reasoning.encrypted_content"]
    })
}

fn text_and_reasoning_events() -> Vec<StreamEvent> {
    vec![
        StreamEvent::ReasoningDelta {
            text: "Need the time".to_owned(),
        },
        StreamEvent::ReasoningDelta {
            text: " and the file.".to_owned(),
        },
        StreamEvent::TextDelta {
            text: "Let me ".to_owned(),
        },
        StreamEvent::TextDelta {
            text: "check.".to_owned(),
        },
    ]
}

fn two_call_events() -> Vec<StreamEvent> {
    vec![
        StreamEvent::ToolCallStarted {
            call_id: call_id("call-1"),
            name: tool("clock"),
        },
        StreamEvent::ToolArgumentsDelta {
            call_id: call_id("call-1"),
            delta: "{}".to_owned(),
        },
        StreamEvent::ToolCallStarted {
            call_id: call_id("call-2"),
            name: tool("file_read"),
        },
        StreamEvent::ToolArgumentsDelta {
            call_id: call_id("call-2"),
            delta: "{\"path\":".to_owned(),
        },
        StreamEvent::ToolArgumentsDelta {
            call_id: call_id("call-2"),
            delta: "\"README.md\"}".to_owned(),
        },
    ]
}

fn two_call_items() -> Vec<Item> {
    vec![
        reasoning(),
        Item::assistant("Let me check."),
        call("call-1", "clock", json!({})),
        call("call-2", "file_read", json!({"path": "README.md"})),
    ]
}

/// Encodes `events` then closes on `outcome`, returning every payload in wire order.
fn encode(events: &[StreamEvent], outcome: &TurnOutcome) -> Result<Vec<Value>, Error> {
    let mut stream = IngressStream::new("resp_gateway_1", CREATED_AT);
    let mut payloads = Vec::new();
    for event in events {
        payloads.extend(stream.chunk(event));
    }
    payloads.extend(stream.close(outcome)?);
    Ok(payloads)
}

fn kinds(payloads: &[Value]) -> Vec<&str> {
    payloads
        .iter()
        .map(|payload| {
            payload["type"]
                .as_str()
                .expect("every payload names its type")
        })
        .collect()
}

const TERMINAL: &[&str] = &[
    "response.completed",
    "response.incomplete",
    "response.failed",
];

/// Every payload is one the decoder interprets, the response opens before anything else, and
/// exactly one terminal payload ends it.
fn assert_well_formed(payloads: &[Value]) {
    let kinds = kinds(payloads);
    for kind in &kinds {
        assert!(
            ACCEPTED_STREAM_EVENTS.contains(kind),
            "`{kind}` is outside the decoder's pinned subset"
        );
    }
    assert_eq!(
        &kinds[..2],
        ["response.created", "response.in_progress"],
        "{kinds:?}"
    );
    let terminals = kinds.iter().filter(|kind| TERMINAL.contains(kind)).count();
    assert_eq!(terminals, 1, "{kinds:?}");
    assert!(
        TERMINAL.contains(kinds.last().expect("not empty")),
        "{kinds:?}"
    );
}

#[test]
fn a_streamed_turn_round_trips_through_the_decoder_for_every_stop_reason() {
    let request = ingest_request(&binding(), &client_body()).expect("the client body ingests");
    let mut with_calls = text_and_reasoning_events();
    with_calls.extend(two_call_events());
    // `EndTurn` beside a proposed call is a contradiction the neutral contract itself refuses
    // (`TurnOutcome::validate_for`), so the end-turn case carries text and reasoning only.
    let cases = [
        (
            StopReason::EndTurn,
            text_and_reasoning_events(),
            vec![reasoning(), Item::assistant("Let me check.")],
        ),
        (StopReason::ToolCalls, with_calls.clone(), two_call_items()),
        (
            StopReason::MaxOutputTokens,
            with_calls.clone(),
            two_call_items(),
        ),
        (
            StopReason::Incomplete {
                reason: "content_filter".to_owned(),
            },
            with_calls,
            two_call_items(),
        ),
    ];
    for (stop_reason, events, items) in cases {
        let encoded = outcome(stop_reason.clone(), items);
        encoded
            .validate_for(&request, &provenance())
            .expect("the fixture outcome is a valid answer to the ingested request");
        let payloads = encode(&events, &encoded).expect("the outcome encodes");
        assert_well_formed(&payloads);
        let decoded = decode_stream(&binding(), &payloads);
        assert_eq!(
            decoded.events, events,
            "{stop_reason:?}: the caller sees the same events"
        );
        assert_eq!(
            decoded.result.expect("the decoder accepts the encoding"),
            encoded,
            "{stop_reason:?}: the decoded turn is the encoded one"
        );
    }
}

#[test]
fn a_non_streamed_response_round_trips_when_wrapped_as_the_terminal_object() {
    let cases = [
        (
            StopReason::EndTurn,
            vec![reasoning(), Item::assistant("It is noon.")],
        ),
        (StopReason::ToolCalls, two_call_items()),
        (StopReason::MaxOutputTokens, two_call_items()),
        (
            StopReason::Incomplete {
                reason: "content_filter".to_owned(),
            },
            vec![Item::assistant("partial")],
        ),
    ];
    for (stop_reason, items) in cases {
        let encoded = outcome(stop_reason.clone(), items);
        let body = encode_ingress_response(&encoded, "resp_gateway_1", CREATED_AT)
            .expect("the outcome encodes");
        assert_eq!(body["object"], "response");
        assert_eq!(body["created_at"], CREATED_AT);
        let decoded = decode_stream(
            &binding(),
            &[json!({"type": "response.completed", "response": body})],
        );
        assert_eq!(decoded.events, Vec::new(), "{stop_reason:?}");
        assert_eq!(
            decoded.result.expect("the decoder accepts the object"),
            encoded,
            "{stop_reason:?}"
        );
    }
}

#[test]
fn an_unknown_input_count_is_omitted_and_decodes_back_unknown_never_zero() {
    let mut encoded = outcome(StopReason::EndTurn, vec![Item::assistant("Hi")]);
    encoded.observation.usage = Some(Usage {
        input_tokens: None,
        output_tokens: Some(5),
        ..Usage::default()
    });
    let body = encode_ingress_response(&encoded, "resp_gateway_1", CREATED_AT).expect("encodes");
    assert_eq!(body["usage"], json!({"output_tokens": 5}));
    let payloads = encode(
        &[StreamEvent::TextDelta {
            text: "Hi".to_owned(),
        }],
        &encoded,
    )
    .expect("encodes");
    let terminal = payloads.last().expect("a terminal payload");
    assert_eq!(terminal["response"]["usage"], json!({"output_tokens": 5}));
    let decoded = decode_stream(&binding(), &payloads)
        .result
        .expect("decodes");
    let usage = decoded.observation.usage.clone().expect("a usage report");
    assert_eq!(usage.input_tokens, None);
    assert_eq!(usage.output_tokens, Some(5));
    assert_eq!(decoded, encoded);
}

#[test]
fn a_total_is_reported_only_from_two_known_parts() {
    let body = encode_ingress_response(
        &outcome(StopReason::EndTurn, vec![Item::assistant("Hi")]),
        "resp_gateway_1",
        CREATED_AT,
    )
    .expect("encodes");
    assert_eq!(
        body["usage"],
        json!({
            "input_tokens": 1788, "output_tokens": 115, "total_tokens": 1903,
            "input_tokens_details": {"cached_tokens": 1680},
            "output_tokens_details": {"reasoning_tokens": 42}
        })
    );
}

#[test]
fn a_reported_cache_write_is_named_rather_than_dropped() {
    let mut encoded = outcome(StopReason::EndTurn, vec![Item::assistant("Hi")]);
    encoded.observation.usage = Some(Usage {
        input_tokens: Some(100),
        cache_creation_input_tokens: Some(64),
        ..Usage::default()
    });
    let body = encode_ingress_response(&encoded, "resp_gateway_1", CREATED_AT).expect("encodes");
    assert_eq!(
        body["usage"],
        json!({"input_tokens": 100, "input_tokens_details": {"cache_creation_input_tokens": 64}})
    );
}

#[test]
fn an_unreported_model_is_left_out_and_the_response_id_is_always_the_callers() {
    let mut encoded = outcome(StopReason::EndTurn, vec![Item::assistant("Hi")]);
    encoded.observation.upstream_model = None;
    // The upstream's own id names the upstream conversation and is not relayed.
    encoded.observation.response_id = Some(id("resp_upstream_1"));
    encoded.observation.usage = None;
    let body = encode_ingress_response(&encoded, "resp_gateway_1", CREATED_AT).expect("encodes");
    assert!(body.get("model").is_none(), "{body}");
    assert!(body.get("usage").is_none(), "{body}");
    assert_eq!(body["id"], "resp_gateway_1");
    let decoded = decode_stream(
        &binding(),
        &[json!({"type": "response.completed", "response": body})],
    )
    .result
    .expect("decodes");
    assert_eq!(decoded.observation.upstream_model, None);
    assert_eq!(decoded.observation.usage, None);
    assert_eq!(decoded.observation.response_id, Some(id("resp_gateway_1")));
}

const EVERY_ERROR_CODE: [ErrorCode; 11] = [
    ErrorCode::InvalidRequest,
    ErrorCode::Transport,
    ErrorCode::Protocol,
    ErrorCode::Unauthorized,
    ErrorCode::RateLimited,
    ErrorCode::Refused,
    ErrorCode::TooLarge,
    ErrorCode::Unsupported,
    ErrorCode::Cancelled,
    ErrorCode::Deadline,
    ErrorCode::Unavailable,
];

/// The table in the module documentation ("Failures"): the wire code a neutral code is written
/// as, and the code this crate's decoder reads it back as. Exhaustive, so a new `ErrorCode` does
/// not compile until it has a row.
fn documented_failure(code: ErrorCode, retriable: bool) -> (&'static str, ErrorCode) {
    match code {
        ErrorCode::Unavailable if retriable => ("server_is_overloaded", ErrorCode::Unavailable),
        ErrorCode::Unavailable => ("server_error", ErrorCode::Unavailable),
        ErrorCode::RateLimited => ("rate_limit_exceeded", ErrorCode::RateLimited),
        ErrorCode::Refused => ("refused", ErrorCode::Refused),
        ErrorCode::InvalidRequest => ("invalid_request", ErrorCode::Refused),
        ErrorCode::Transport => ("transport", ErrorCode::Refused),
        ErrorCode::Protocol => ("protocol", ErrorCode::Refused),
        ErrorCode::Unauthorized => ("unauthorized", ErrorCode::Refused),
        ErrorCode::TooLarge => ("too_large", ErrorCode::Refused),
        ErrorCode::Unsupported => ("unsupported", ErrorCode::Refused),
        ErrorCode::Cancelled => ("cancelled", ErrorCode::Refused),
        ErrorCode::Deadline => ("deadline", ErrorCode::Refused),
    }
}

#[test]
fn fail_ends_the_stream_with_a_failure_the_decoder_reads_back_by_the_documented_mapping() {
    for code in EVERY_ERROR_CODE {
        for retriable in [false, true] {
            let error = Error::new(code, "upstream-prose-never-relayed").with_retriable(retriable);
            let (wire_code, decoded_code) = documented_failure(code, retriable);
            // Nothing streamed: `fail` opens the response itself.
            let mut stream = IngressStream::new("resp_gateway_1", CREATED_AT);
            let payloads = stream.fail(&error);
            assert_well_formed(&payloads);
            let failed = payloads.last().expect("a terminal payload");
            assert_eq!(failed["type"], "response.failed");
            assert_eq!(failed["response"]["id"], "resp_gateway_1");
            assert_eq!(
                failed["response"]["error"]["code"], wire_code,
                "{code:?}, retriable {retriable}"
            );
            // The neutral diagnostic is never relayed onto the wire.
            assert!(!failed.to_string().contains(&error.message), "{failed}");
            let refusal = decode_stream(&binding(), &payloads)
                .result
                .expect_err("a failed response is a refusal");
            assert_eq!(
                refusal.code, decoded_code,
                "{code:?}, retriable {retriable}: written as {wire_code}"
            );
            assert_eq!(refusal.dispatch, Dispatch::Accepted);
        }
    }
}

#[test]
fn fail_after_output_keeps_what_was_shown_and_ends_with_one_failure() {
    let mut stream = IngressStream::new("resp_gateway_1", CREATED_AT);
    let shown = [StreamEvent::TextDelta {
        text: "Hel".to_owned(),
    }];
    let mut payloads = Vec::new();
    for event in &shown {
        payloads.extend(stream.chunk(event));
    }
    payloads.extend(stream.fail(&Error::new(ErrorCode::RateLimited, "slow down")));
    assert_well_formed(&payloads);
    // The stream is over: nothing more is emitted, and it cannot be closed a second time.
    assert_eq!(
        stream.chunk(&StreamEvent::TextDelta {
            text: "lo".to_owned()
        }),
        Vec::<Value>::new()
    );
    assert!(
        stream
            .close(&outcome(
                StopReason::EndTurn,
                vec![Item::assistant("Hello")]
            ))
            .is_err()
    );
    let decoded = decode_stream(&binding(), &payloads);
    assert_eq!(decoded.events, shown);
    assert_eq!(
        decoded.result.expect_err("a refusal").code,
        ErrorCode::RateLimited
    );
}

#[test]
fn an_outcome_contradicting_the_streamed_calls_is_refused() {
    let events = two_call_events();
    let missing = outcome(
        StopReason::ToolCalls,
        vec![call("call-1", "clock", json!({}))],
    );
    let renamed = outcome(
        StopReason::ToolCalls,
        vec![
            call("call-1", "clock", json!({})),
            call("call-2", "file_write", json!({"path": "README.md"})),
        ],
    );
    let different_arguments = outcome(
        StopReason::ToolCalls,
        vec![
            call("call-1", "clock", json!({})),
            call("call-2", "file_read", json!({"path": "Cargo.toml"})),
        ],
    );
    for (why, contradicting) in [
        ("an announced call is missing", missing),
        ("an announced call is renamed", renamed),
        ("relayed arguments differ", different_arguments),
    ] {
        let refusal = encode(&events, &contradicting).expect_err(why);
        assert_eq!(refusal.code, ErrorCode::Protocol, "{why}");
    }
}

#[test]
fn an_outcome_without_the_text_the_caller_was_shown_is_refused() {
    let refusal = encode(
        &[StreamEvent::TextDelta {
            text: "Hel".to_owned(),
        }],
        &outcome(
            StopReason::ToolCalls,
            vec![call("call-1", "clock", json!({}))],
        ),
    )
    .expect_err("shown text the outcome does not carry");
    assert_eq!(refusal.code, ErrorCode::Protocol);
}

/// The decoder keeps only shown text that is not empty, so an empty delta obliges the outcome to
/// carry no message.
#[test]
fn an_empty_text_delta_needs_no_message_in_the_outcome() {
    let events = vec![
        StreamEvent::TextDelta {
            text: String::new(),
        },
        StreamEvent::ToolCallStarted {
            call_id: call_id("call-1"),
            name: tool("clock"),
        },
        StreamEvent::ToolArgumentsDelta {
            call_id: call_id("call-1"),
            delta: "{}".to_owned(),
        },
    ];
    let encoded = outcome(
        StopReason::ToolCalls,
        vec![call("call-1", "clock", json!({}))],
    );
    let payloads = encode(&events, &encoded).expect("encodes");
    assert_well_formed(&payloads);
    let decoded = decode_stream(&binding(), &payloads);
    assert_eq!(decoded.events, events);
    assert_eq!(decoded.result.expect("decodes"), encoded);
}

#[test]
fn a_refused_close_leaves_the_stream_open_for_fail() {
    let mut stream = IngressStream::new("resp_gateway_1", CREATED_AT);
    let mut payloads = Vec::new();
    for event in two_call_events() {
        payloads.extend(stream.chunk(&event));
    }
    let refusal = stream
        .close(&outcome(
            StopReason::EndTurn,
            vec![Item::assistant("no calls")],
        ))
        .expect_err("contradiction");
    payloads.extend(stream.fail(&refusal));
    assert_well_formed(&payloads);
}

#[test]
fn caller_owned_items_and_unattributed_or_foreign_state_are_refused() {
    let mut foreign = provenance();
    foreign.binding_revision = id("rev-2");
    let cases = [
        (Item::user("Hi"), ErrorCode::Protocol),
        (
            Item::ToolResult {
                call_id: call_id("call-1"),
                output: json!("noon"),
                failed: false,
            },
            ErrorCode::Protocol,
        ),
        (
            Item::UnattributedOpaque {
                protocol: Protocol::Responses,
                payload: json!({"type": "reasoning", "summary": []}),
            },
            ErrorCode::Unsupported,
        ),
        (
            Item::Opaque {
                provenance: foreign,
                payload: json!({"type": "reasoning", "summary": []}),
            },
            ErrorCode::Unsupported,
        ),
        // Opaque state this wire's output carries only as reasoning.
        (
            Item::Opaque {
                provenance: provenance(),
                payload: json!({"type": "response.web_search_call.searching"}),
            },
            ErrorCode::Unsupported,
        ),
    ];
    for (item, code) in cases {
        let refused = outcome(
            StopReason::EndTurn,
            vec![Item::assistant("Hi"), item.clone()],
        );
        let streamed = encode(&[], &refused).expect_err("refused streamed");
        assert_eq!(streamed.code, code, "{item:?}");
        let whole = encode_ingress_response(&refused, "resp_gateway_1", CREATED_AT)
            .expect_err("refused non-streamed");
        assert_eq!(whole.code, code, "{item:?}");
        if matches!(item, Item::UnattributedOpaque { .. }) {
            assert_eq!(whole.message, Item::UNATTRIBUTED_REFUSAL);
        }
    }
}

#[test]
fn a_stop_reason_the_wire_cannot_express_is_refused_not_approximated() {
    let cases = [
        // The decoder derives `ToolCalls` from any call in the turn.
        outcome(
            StopReason::EndTurn,
            vec![call("call-1", "clock", json!({}))],
        ),
        // And `EndTurn` from a turn without one.
        outcome(StopReason::ToolCalls, vec![Item::assistant("Hi")]),
        // Read back as `MaxOutputTokens`, not as this reason.
        outcome(
            StopReason::Incomplete {
                reason: "max_output_tokens".to_owned(),
            },
            vec![Item::assistant("Hi")],
        ),
        // Read back as `unrecognized`: the decoder relays only code-shaped reasons.
        outcome(
            StopReason::Incomplete {
                reason: "content-filter".to_owned(),
            },
            vec![Item::assistant("Hi")],
        ),
    ];
    for refused in cases {
        let error = encode_ingress_response(&refused, "resp_gateway_1", CREATED_AT)
            .expect_err("inexpressible");
        assert_eq!(
            error.code,
            ErrorCode::Unsupported,
            "{:?}",
            refused.stop_reason
        );
        assert_eq!(
            encode(&[], &refused).expect_err("inexpressible").code,
            ErrorCode::Unsupported
        );
    }
}

#[test]
fn a_call_never_announced_is_emitted_whole_at_close() {
    // Fragments for a call nobody announced are not relayed: there is no item to name.
    let unannounced = StreamEvent::ToolArgumentsDelta {
        call_id: call_id("call-1"),
        delta: "{}".to_owned(),
    };
    let encoded = outcome(
        StopReason::ToolCalls,
        vec![call("call-1", "clock", json!({}))],
    );
    let mut stream = IngressStream::new("resp_gateway_1", CREATED_AT);
    assert_eq!(stream.chunk(&unannounced), Vec::<Value>::new());
    let payloads = stream.close(&encoded).expect("encodes");
    assert_well_formed(&payloads);
    let decoded = decode_stream(&binding(), &payloads);
    assert_eq!(
        decoded.events,
        vec![
            StreamEvent::ToolCallStarted {
                call_id: call_id("call-1"),
                name: tool("clock"),
            },
            StreamEvent::ToolArgumentsDelta {
                call_id: call_id("call-1"),
                delta: "{}".to_owned(),
            },
        ]
    );
    assert_eq!(decoded.result.expect("decodes"), encoded);
}

#[test]
fn reasoning_shown_without_carried_state_adds_no_item() {
    let events = vec![StreamEvent::ReasoningDelta {
        text: "thinking".to_owned(),
    }];
    let encoded = outcome(StopReason::EndTurn, vec![Item::assistant("noon")]);
    let payloads = encode(&events, &encoded).expect("encodes");
    assert_well_formed(&payloads);
    let decoded = decode_stream(&binding(), &payloads);
    assert_eq!(
        decoded.events,
        vec![
            StreamEvent::ReasoningDelta {
                text: "thinking".to_owned()
            },
            StreamEvent::TextDelta {
                text: "noon".to_owned()
            },
        ]
    );
    assert_eq!(decoded.result.expect("decodes"), encoded);
}

#[test]
fn an_empty_stream_closes_on_the_opening_events_and_the_turn() {
    let encoded = outcome(StopReason::EndTurn, Vec::new());
    let payloads = encode(&[], &encoded).expect("encodes");
    assert_eq!(
        kinds(&payloads),
        [
            "response.created",
            "response.in_progress",
            "response.completed"
        ]
    );
    assert_eq!(
        decode_stream(&binding(), &payloads)
            .result
            .expect("decodes"),
        encoded
    );
}

/// Everything the decoder refuses when reading a turn's items back, the encoder refuses before
/// emitting it: arguments that are not an object, arguments over their bound, and more items than
/// a turn holds. Each is checked on both paths.
#[test]
fn what_the_decoder_would_refuse_to_read_back_is_refused_not_emitted() {
    let oversize = json!({"path": "x".repeat(70_000)});
    let mut cases: Vec<(String, TurnOutcome, ErrorCode)> = [json!([1]), json!("noon"), json!(null)]
        .into_iter()
        .map(|arguments| {
            (
                format!("arguments {arguments}"),
                outcome(
                    StopReason::ToolCalls,
                    vec![call("call-1", "clock", arguments)],
                ),
                ErrorCode::Unsupported,
            )
        })
        .collect();
    cases.push((
        "arguments over their bound".to_owned(),
        outcome(
            StopReason::ToolCalls,
            vec![call("call-1", "file_read", oversize)],
        ),
        ErrorCode::TooLarge,
    ));
    cases.push((
        "more items than a turn holds".to_owned(),
        outcome(
            StopReason::EndTurn,
            vec![Item::assistant("x"); MAX_ITEMS + 1],
        ),
        ErrorCode::TooLarge,
    ));
    for (why, refused, code) in cases {
        let whole =
            encode_ingress_response(&refused, "resp_gateway_1", CREATED_AT).expect_err(&why);
        assert_eq!(whole.code, code, "{why}");
        let streamed = encode(&[], &refused).expect_err(&why);
        assert_eq!(streamed.code, code, "{why}");
    }
}

/// Text after a call is a new message item: each item's deltas are one message's text, and the
/// close finishes each with that text, as the outcome's message in the same position.
#[test]
fn text_after_a_call_opens_a_new_message() {
    let events = vec![
        StreamEvent::TextDelta {
            text: "First.".to_owned(),
        },
        StreamEvent::ToolCallStarted {
            call_id: call_id("call-1"),
            name: tool("clock"),
        },
        StreamEvent::ToolArgumentsDelta {
            call_id: call_id("call-1"),
            delta: "{}".to_owned(),
        },
        StreamEvent::TextDelta {
            text: "Second.".to_owned(),
        },
    ];
    let encoded = outcome(
        StopReason::ToolCalls,
        vec![
            Item::assistant("First."),
            call("call-1", "clock", json!({})),
            Item::assistant("Second."),
        ],
    );
    let payloads = encode(&events, &encoded).expect("encodes");
    assert_well_formed(&payloads);
    let finished: Vec<(&str, &str)> = payloads
        .iter()
        .filter(|payload| payload["type"] == "response.output_text.done")
        .map(|payload| {
            (
                payload["item_id"].as_str().expect("item id"),
                payload["text"].as_str().expect("text"),
            )
        })
        .collect();
    assert_eq!(
        finished,
        [
            ("msg_resp_gateway_1_0", "First."),
            ("msg_resp_gateway_1_2", "Second.")
        ]
    );
    let decoded = decode_stream(&binding(), &payloads);
    assert_eq!(decoded.events, events);
    assert_eq!(decoded.result.expect("decodes"), encoded);
}

/// A streamed message finishes with exactly the text its deltas showed; an outcome carrying other
/// text in that message is refused rather than shown twice or rewritten.
#[test]
fn an_outcome_rewriting_a_shown_message_is_refused() {
    let refusal = encode(
        &[StreamEvent::TextDelta {
            text: "Hel".to_owned(),
        }],
        &outcome(StopReason::EndTurn, vec![Item::assistant("Hello")]),
    )
    .expect_err("the caller was shown other text");
    assert_eq!(refusal.code, ErrorCode::Protocol);
}

/// A `response.failed` object's counters read back as final. `fail` relays the error's counters
/// only when its observation says they are final; a partial snapshot is left out, never promoted.
/// The upstream model is an observation either way and is relayed in both.
#[test]
fn fail_relays_counters_only_when_the_observation_reports_them_as_final() {
    for final_usage in [true, false] {
        let mut observed = TurnObservation::new(provenance());
        observed.upstream_model = Some(id("example/Small-Model"));
        observed.usage = Some(full_usage());
        observed.final_usage = final_usage;
        let error = Error::new(ErrorCode::Unavailable, "overloaded").with_observation(observed);
        let mut stream = IngressStream::new("resp_gateway_1", CREATED_AT);
        let mut payloads = stream.chunk(&StreamEvent::TextDelta {
            text: "Hel".to_owned(),
        });
        payloads.extend(stream.fail(&error));
        assert_well_formed(&payloads);
        let failed = &payloads.last().expect("a terminal payload")["response"];
        assert_eq!(
            failed.get("usage").is_some(),
            final_usage,
            "final_usage {final_usage}: {failed}"
        );
        let refusal = decode_stream(&binding(), &payloads)
            .result
            .expect_err("a failed response is a refusal");
        let read_back = refusal.observation.as_deref().expect("evidence");
        assert_eq!(
            read_back.upstream_model,
            Some(id("example/Small-Model")),
            "final_usage {final_usage}"
        );
        let expected = final_usage.then(full_usage);
        assert_eq!(read_back.usage, expected, "final_usage {final_usage}");
    }
}

/// A call announced with argument fragments that are blank reads back as exactly those events:
/// the close finishes the call with its arguments and relays no fragment of its own, because the
/// decoder reads a call's arguments from the finished item, not from the fragments.
#[test]
fn an_announced_call_with_blank_fragments_reads_back_as_the_events_encoded() {
    for fragments in [&[][..], &[""][..], &[" ", "\n"][..]] {
        let mut events = vec![StreamEvent::ToolCallStarted {
            call_id: call_id("call-1"),
            name: tool("file_read"),
        }];
        events.extend(
            fragments
                .iter()
                .map(|delta| StreamEvent::ToolArgumentsDelta {
                    call_id: call_id("call-1"),
                    delta: (*delta).to_owned(),
                }),
        );
        let encoded = outcome(
            StopReason::ToolCalls,
            vec![call("call-1", "file_read", json!({"path": "README.md"}))],
        );
        let payloads = encode(&events, &encoded).expect("encodes");
        assert_well_formed(&payloads);
        let decoded = decode_stream(&binding(), &payloads);
        assert_eq!(decoded.events, events, "fragments {fragments:?}");
        assert_eq!(
            decoded.result.expect("decodes"),
            encoded,
            "fragments {fragments:?}"
        );
    }
}

/// An empty assistant message is never shown, so it matches no streamed message: wherever the
/// outcome carries one, it is emitted whole at close and the shown messages keep their order.
#[test]
fn an_empty_message_anywhere_in_the_outcome_reads_back_unchanged() {
    let shown = |value: &str| StreamEvent::TextDelta {
        text: value.to_owned(),
    };
    let started = StreamEvent::ToolCallStarted {
        call_id: call_id("call-1"),
        name: tool("clock"),
    };
    let arguments = StreamEvent::ToolArgumentsDelta {
        call_id: call_id("call-1"),
        delta: "{}".to_owned(),
    };
    let clock = || call("call-1", "clock", json!({}));
    let cases = [
        (
            "empty ahead of a shown message",
            vec![shown("Done.")],
            StopReason::EndTurn,
            vec![Item::assistant(""), Item::assistant("Done.")],
        ),
        (
            "empty after a shown message",
            vec![shown("Done.")],
            StopReason::EndTurn,
            vec![Item::assistant("Done."), Item::assistant("")],
        ),
        (
            "an empty delta, a call, then a shown message",
            vec![shown(""), started.clone(), arguments.clone(), shown("B.")],
            StopReason::ToolCalls,
            vec![Item::assistant(""), clock(), Item::assistant("B.")],
        ),
        (
            "two shown messages around a call, empty between",
            vec![shown("A."), started, arguments, shown("B.")],
            StopReason::ToolCalls,
            vec![
                Item::assistant("A."),
                Item::assistant(""),
                clock(),
                Item::assistant("B."),
            ],
        ),
    ];
    for (why, events, stop_reason, items) in cases {
        let encoded = outcome(stop_reason, items);
        let payloads = encode(&events, &encoded).unwrap_or_else(|error| panic!("{why}: {error:?}"));
        assert_well_formed(&payloads);
        let decoded = decode_stream(&binding(), &payloads);
        assert_eq!(decoded.events, events, "{why}");
        assert_eq!(decoded.result.expect("decodes"), encoded, "{why}");
    }
}

/// An empty carried message does not stand in for a shown one: the shown text must still be
/// carried, or the outcome is a different turn from the one the caller watched.
#[test]
fn an_empty_message_does_not_carry_shown_text() {
    let refusal = encode(
        &[StreamEvent::TextDelta {
            text: "Hel".to_owned(),
        }],
        &outcome(StopReason::EndTurn, vec![Item::assistant("")]),
    )
    .expect_err("the shown text is not carried");
    assert_eq!(refusal.code, ErrorCode::Protocol);
}
