//! Pinned request and streaming fixtures for the supported Responses subset.
//!
//! Every fixture here is a literal, so a change to the projected body or to the decoded stream
//! is visible as a diff rather than as a recomputed expectation.

use llm_core::{
    CallId, Dispatch, Error, ErrorCode, Id, Item, Protocol, Provenance, Sampling, StopReason,
    StreamEvent, ToolCall, ToolChoice, ToolName, ToolSpec, TurnRequest, Usage,
};
use llm_responses::{Binding, PATH, decode_stream, ingest_request, project_request};
use serde_json::{Value, json};

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

fn text_request() -> TurnRequest {
    TurnRequest {
        model: "small".to_owned(),
        instructions: "Keep instructions".to_owned(),
        items: vec![Item::user("Keep message")],
        tools: Vec::new(),
        max_output_tokens: Some(128),
        sampling: Sampling::default(),
        tool_choice: ToolChoice::Auto,
    }
}

fn tool() -> ToolSpec {
    ToolSpec {
        name: ToolName::new("file_read").expect("fixture tool name"),
        description: "Read one file".to_owned(),
        input_schema: json!({"type": "object"}),
    }
}

#[test]
fn a_text_turn_projects_the_pinned_body() {
    let body = project_request(&binding(), &text_request()).expect("the pinned subset projects");
    assert_eq!(
        body,
        json!({
            "model": "example/Small-Model",
            "input": [
                {"type": "message", "role": "developer",
                 "content": [{"type": "input_text", "text": "Keep instructions"}]},
                {"type": "message", "role": "user",
                 "content": [{"type": "input_text", "text": "Keep message"}]}
            ],
            "tools": [],
            "stream": true,
            "store": false,
            "include": ["reasoning.encrypted_content"],
            "max_output_tokens": 128
        })
    );
    assert_eq!(PATH, "/responses");
}

#[test]
fn the_wire_model_is_the_upstream_name_and_the_alias_never_substitutes_for_it() {
    let body = project_request(&binding(), &text_request()).expect("projects");
    assert_eq!(body["model"], json!("example/Small-Model"));
    assert_ne!(body["model"], json!("small"));
}

#[test]
fn an_absent_sampling_setting_stays_absent_on_the_wire() {
    let body = project_request(&binding(), &text_request()).expect("projects");
    for absent in [
        "temperature",
        "top_p",
        "reasoning",
        "tool_choice",
        "instructions",
    ] {
        assert!(body.get(absent).is_none(), "`{absent}` must stay absent");
    }
}

#[test]
fn a_present_sampling_setting_is_projected_in_this_wires_spelling() {
    let mut request = text_request();
    request.sampling = Sampling {
        temperature: Some(0.5),
        top_p: Some(0.9),
        reasoning_effort: Some("medium".to_owned()),
    };
    request.tool_choice = ToolChoice::Required;
    request.tools = vec![tool()];
    let body = project_request(&binding(), &request).expect("projects");
    assert_eq!(body["temperature"], json!(0.5));
    assert_eq!(body["top_p"], json!(0.9));
    assert_eq!(body["reasoning"], json!({"effort": "medium"}));
    assert_eq!(body["tool_choice"], json!("required"));
    assert_eq!(
        body["tools"],
        json!([{"type": "function", "name": "file_read",
                "description": "Read one file", "parameters": {"type": "object"},
                "strict": false}])
    );
}

#[test]
fn a_named_tool_choice_names_the_published_tool() {
    let mut request = text_request();
    request.tools = vec![tool()];
    request.tool_choice = ToolChoice::Named(ToolName::new("file_read").expect("name"));
    let body = project_request(&binding(), &request).expect("projects");
    assert_eq!(
        body["tool_choice"],
        json!({"type": "function", "name": "file_read"})
    );
}

fn tool_round_trip_request() -> TurnRequest {
    let call_id = CallId::new("call-1").expect("fixture call id");
    TurnRequest {
        model: "small".to_owned(),
        instructions: String::new(),
        items: vec![
            Item::user("Read it"),
            Item::assistant("Reading."),
            Item::ToolCall(ToolCall {
                call_id: call_id.clone(),
                name: ToolName::new("file_read").expect("name"),
                arguments: json!({"path": "README.md"}),
            }),
            Item::ToolResult {
                call_id,
                output: json!("# Title"),
                failed: false,
            },
        ],
        tools: vec![tool()],
        max_output_tokens: None,
        sampling: Sampling::default(),
        tool_choice: ToolChoice::Auto,
    }
}

#[test]
fn the_text_and_tool_subset_round_trips_through_the_wire_body() {
    let request = tool_round_trip_request();
    let body = project_request(&binding(), &request).expect("projects");
    assert_eq!(
        body["input"],
        json!([
            {"type": "message", "role": "user",
             "content": [{"type": "input_text", "text": "Read it"}]},
            {"type": "message", "role": "assistant",
             "content": [{"type": "output_text", "text": "Reading."}]},
            {"type": "function_call", "call_id": "call-1", "name": "file_read",
             "arguments": "{\"path\":\"README.md\"}"},
            {"type": "function_call_output", "call_id": "call-1",
             "output": "{\"ok\":true,\"output\":\"# Title\"}"}
        ])
    );
    let ingested = ingest_request(&binding(), &body).expect("the same contract reads it back");
    assert_eq!(ingested, request);
}

#[test]
fn a_failed_tool_result_keeps_its_failure_through_the_round_trip() {
    let call_id = CallId::new("call-1").expect("fixture call id");
    let mut request = tool_round_trip_request();
    request.items[3] = Item::ToolResult {
        call_id,
        output: json!({"ok": true, "output": "not a success flag"}),
        failed: true,
    };
    let body = project_request(&binding(), &request).expect("projects");
    assert_eq!(
        body["input"][3]["output"],
        json!("{\"ok\":false,\"output\":{\"ok\":true,\"output\":\"not a success flag\"}}")
    );
    let ingested = ingest_request(&binding(), &body).expect("reads back");
    assert_eq!(ingested, request);
}

#[test]
fn an_empty_instruction_projects_no_developer_entry_and_round_trips() {
    let request = tool_round_trip_request();
    assert!(request.instructions.is_empty());
    let body = project_request(&binding(), &request).expect("projects");
    assert_ne!(body["input"][0]["role"], json!("developer"));
    assert_eq!(
        ingest_request(&binding(), &body)
            .expect("reads back")
            .instructions,
        ""
    );
}

#[test]
fn a_tool_name_this_wire_cannot_publish_is_refused_before_it_is_sent() {
    let mut request = text_request();
    request.tools = vec![ToolSpec {
        name: ToolName::new("workspace.read").expect("a printable identifier"),
        description: "d".to_owned(),
        input_schema: json!({"type": "object"}),
    }];
    let error = project_request(&binding(), &request).expect_err("refused");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(error.dispatch, Dispatch::NotSent);
    assert!(
        !error.message.contains("workspace.read"),
        "diagnostics stay fixed: {}",
        error.message
    );
}

fn opaque(mut provenance: Provenance, edit: impl FnOnce(&mut Provenance)) -> Item {
    edit(&mut provenance);
    Item::Opaque {
        provenance,
        payload: json!({"type": "reasoning", "id": "rs_1", "summary": []}),
    }
}

/// Opaque state goes out verbatim and does not come back in. The asymmetry is the contract.
///
/// Egress can attribute it: the caller holds an item this binding minted and the six coordinates
/// are checked against the binding before a byte is sent. Ingress cannot — the wire body carries
/// no provenance, so reading one back would mean this crate asserting an origin it never
/// observed, and a payload minted under `rev-1` and replayed after a repoint would be reinstated
/// as native state for `rev-2`. The round trip is therefore total for the text and tool subset
/// and deliberately partial here, which is the choice `docs/design.md` sanctions: preserved or
/// refused, never quietly translated.
#[test]
fn opaque_state_is_carried_out_verbatim_and_refused_on_the_way_back_in() {
    let mut request = text_request();
    request.items.push(opaque(provenance(), |_| {}));
    let body = project_request(&binding(), &request).expect("projects");
    assert_eq!(
        body["input"][2],
        json!({"type": "reasoning", "id": "rs_1", "summary": []})
    );
    let error = ingest_request(&binding(), &body)
        .map(|returned| returned.items)
        .expect_err("ingress cannot attribute what it did not watch this binding produce");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(error.dispatch, Dispatch::NotSent);

    // And the text and tool subset is unaffected: drop the opaque item and it round trips.
    request.items.pop();
    let body = project_request(&binding(), &request).expect("projects");
    assert_eq!(
        ingest_request(&binding(), &body).expect("reads back"),
        request
    );
}

#[test]
fn opaque_state_from_any_other_binding_coordinate_is_refused() {
    type Edit = (&'static str, fn(&mut Provenance));
    let edits: [Edit; 6] = [
        ("protocol", |p| p.protocol = Protocol::Messages),
        ("provider", |p| p.provider = id("other-lab")),
        ("account", |p| p.account = id("other-account")),
        ("endpoint", |p| p.endpoint = id("other-endpoint")),
        ("model", |p| p.model = id("other-model")),
        ("binding_revision", |p| p.binding_revision = id("rev-2")),
    ];
    for (coordinate, edit) in edits {
        let mut request = text_request();
        request.items.push(opaque(provenance(), edit));
        let Err(error) = project_request(&binding(), &request) else {
            panic!("a foreign `{coordinate}` must refuse rather than send");
        };
        assert_eq!(error.code, ErrorCode::Unsupported, "{coordinate}");
        assert_eq!(error.dispatch, Dispatch::NotSent, "{coordinate}");
    }
}

#[test]
fn a_body_field_outside_the_pinned_subset_is_refused_on_ingress() {
    let mut body = project_request(&binding(), &text_request()).expect("projects");
    body["previous_response_id"] = json!("resp_1");
    let error = ingest_request(&binding(), &body).expect_err("refused");
    assert_eq!(error.code, ErrorCode::Unsupported);
}

#[test]
fn an_ingress_body_for_another_model_is_refused() {
    let mut body = project_request(&binding(), &text_request()).expect("projects");
    body["model"] = json!("example/Large-Model");
    let error = ingest_request(&binding(), &body).expect_err("refused");
    assert_eq!(error.code, ErrorCode::InvalidRequest);
}

fn completed(usage: &Value) -> Value {
    json!({"type": "response.completed", "response": {
        "id": "resp_1", "model": "example/Small-Model", "status": "completed",
        "output": [{"type": "message", "role": "assistant",
                    "content": [{"type": "output_text", "text": "Hello"}]}],
        "usage": usage
    }})
}

#[test]
fn a_text_stream_decodes_to_terminal_truth_and_exact_counts() {
    let payloads = vec![
        json!({"type": "response.created", "response": {"id": "resp_1"}}),
        json!({"type": "response.output_text.delta", "delta": "Hel"}),
        json!({"type": "response.output_text.delta", "delta": "lo"}),
        completed(&json!({
            "input_tokens": 1788, "output_tokens": 115,
            "input_tokens_details": {"cached_tokens": 1680},
            "output_tokens_details": {"reasoning_tokens": 42}
        })),
    ];
    let decoding = decode_stream(&binding(), &payloads);
    assert_eq!(
        decoding.events,
        vec![
            StreamEvent::TextDelta {
                text: "Hel".to_owned()
            },
            StreamEvent::TextDelta {
                text: "lo".to_owned()
            },
        ]
    );
    let outcome = decoding.result.expect("a terminal response");
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
    assert_eq!(outcome.items, vec![Item::assistant("Hello")]);
    let observation = outcome.observation;
    assert_eq!(observation.binding, provenance());
    assert_eq!(
        observation.upstream_model.map(|v| v.to_string()),
        Some("example/Small-Model".to_owned())
    );
    assert_eq!(
        observation.response_id.map(|v| v.to_string()),
        Some("resp_1".to_owned())
    );
    assert!(observation.final_usage);
    let usage = observation.usage.expect("reported counters");
    assert_eq!(usage.input_tokens, Some(1788));
    assert_eq!(usage.output_tokens, Some(115));
    assert_eq!(usage.cached_input_tokens, Some(1680));
    assert_eq!(usage.reasoning_output_tokens, Some(42));
    assert_eq!(usage.cache_creation_input_tokens, None);
}

#[test]
fn an_unreported_counter_stays_unknown_and_is_never_zero() {
    let payloads = vec![completed(&json!({"input_tokens": 10}))];
    let outcome = decode_stream(&binding(), &payloads)
        .result
        .expect("terminal");
    let usage = outcome.observation.usage.expect("reported counters");
    assert_eq!(usage.input_tokens, Some(10));
    assert_eq!(usage.output_tokens, None);
    assert_eq!(usage.cached_input_tokens, None);
    assert_eq!(usage.reasoning_output_tokens, None);
}

#[test]
fn an_unreported_model_stays_unknown_rather_than_the_configured_one() {
    let payloads = vec![json!({"type": "response.completed", "response": {
        "status": "completed", "output": []
    }})];
    let outcome = decode_stream(&binding(), &payloads)
        .result
        .expect("terminal");
    assert_eq!(outcome.observation.upstream_model, None);
    assert_eq!(outcome.observation.response_id, None);
    assert_eq!(outcome.observation.usage, None);
}

#[test]
fn a_tool_call_stream_names_the_call_its_arguments_belong_to() {
    let payloads = vec![
        json!({"type": "response.output_item.added", "item": {
            "type": "function_call", "id": "fc_1", "call_id": "call-1", "name": "file_read"}}),
        json!({"type": "response.function_call_arguments.delta",
               "item_id": "fc_1", "delta": "{\"path\":"}),
        json!({"type": "response.function_call_arguments.delta",
               "item_id": "fc_1", "delta": "\"README.md\"}"}),
        json!({"type": "response.completed", "response": {
            "status": "completed",
            "output": [{"type": "function_call", "call_id": "call-1", "name": "file_read",
                        "arguments": "{\"path\":\"README.md\"}"}]}}),
    ];
    let decoding = decode_stream(&binding(), &payloads);
    assert_eq!(
        decoding.events,
        vec![
            StreamEvent::ToolCallStarted {
                call_id: CallId::new("call-1").expect("id"),
                name: ToolName::new("file_read").expect("name"),
            },
            StreamEvent::ToolArgumentsDelta {
                call_id: CallId::new("call-1").expect("id"),
                delta: "{\"path\":".to_owned()
            },
            StreamEvent::ToolArgumentsDelta {
                call_id: CallId::new("call-1").expect("id"),
                delta: "\"README.md\"}".to_owned()
            },
        ]
    );
    let outcome = decoding.result.expect("terminal");
    assert_eq!(outcome.stop_reason, StopReason::ToolCalls);
    assert_eq!(
        outcome
            .tool_calls()
            .map(|call| call.name.to_string())
            .collect::<Vec<_>>(),
        vec!["file_read".to_owned()]
    );
}

#[test]
fn each_interleaved_call_is_announced_before_any_of_its_arguments() {
    let payloads = vec![
        json!({"type": "response.output_item.added", "item": {
            "type": "function_call", "id": "fc_1", "call_id": "call-1", "name": "file_read"}}),
        json!({"type": "response.function_call_arguments.delta", "item_id": "fc_1", "delta": "{}"}),
        json!({"type": "response.output_item.added", "item": {
            "type": "function_call", "id": "fc_2", "call_id": "call-2", "name": "clock"}}),
        json!({"type": "response.function_call_arguments.delta", "item_id": "fc_2", "delta": "{}"}),
        json!({"type": "response.completed", "response": {"status": "completed", "output": [
            {"type": "function_call", "call_id": "call-1", "name": "file_read", "arguments": "{}"},
            {"type": "function_call", "call_id": "call-2", "name": "clock", "arguments": "{}"}]}}),
    ];
    let decoding = decode_stream(&binding(), &payloads);
    let started = |id: &str, name: &str| StreamEvent::ToolCallStarted {
        call_id: CallId::new(id).expect("id"),
        name: ToolName::new(name).expect("name"),
    };
    let arguments = |id: &str| StreamEvent::ToolArgumentsDelta {
        call_id: CallId::new(id).expect("id"),
        delta: "{}".to_owned(),
    };
    assert_eq!(
        decoding.events,
        vec![
            started("call-1", "file_read"),
            arguments("call-1"),
            started("call-2", "clock"),
            arguments("call-2"),
        ]
    );
    decoding.result.expect("terminal");
}

#[test]
fn an_item_reopened_under_another_call_id_keeps_the_call_it_announced() {
    let payloads = vec![
        json!({"type": "response.output_item.added", "item": {
            "type": "function_call", "id": "fc_1", "call_id": "call-1", "name": "file_read"}}),
        json!({"type": "response.output_item.added", "item": {
            "type": "function_call", "id": "fc_1", "call_id": "call-2", "name": "file_read"}}),
        json!({"type": "response.function_call_arguments.delta", "item_id": "fc_1", "delta": "{}"}),
        json!({"type": "response.completed", "response": {"status": "completed", "output": [
            {"type": "function_call", "call_id": "call-1", "name": "file_read", "arguments": "{}"}]}}),
    ];
    let decoding = decode_stream(&binding(), &payloads);
    // The item was announced once; its arguments stay under the call the caller was shown.
    assert_eq!(
        decoding.events,
        vec![
            StreamEvent::ToolCallStarted {
                call_id: CallId::new("call-1").expect("id"),
                name: ToolName::new("file_read").expect("name"),
            },
            StreamEvent::ToolArgumentsDelta {
                call_id: CallId::new("call-1").expect("id"),
                delta: "{}".to_owned(),
            },
        ]
    );
    decoding.result.expect("terminal");
}

#[test]
fn an_opening_item_without_a_usable_name_announces_nothing_and_streams_nothing() {
    let payloads = vec![
        json!({"type": "response.output_item.added", "item": {
            "type": "function_call", "id": "fc_1", "call_id": "call-1", "name": "has space"}}),
        json!({"type": "response.function_call_arguments.delta", "item_id": "fc_1", "delta": "{}"}),
        json!({"type": "response.completed", "response": {"status": "completed", "output": [
            {"type": "function_call", "call_id": "call-1", "name": "has space", "arguments": "{}"}]}}),
    ];
    let decoding = decode_stream(&binding(), &payloads);
    // Arguments of a call nobody was told about would be deltas for a call the caller cannot
    // name; the terminal object then refuses the name itself.
    assert!(decoding.events.is_empty(), "{:?}", decoding.events);
    assert_eq!(
        decoding.result.expect_err("refused").code,
        ErrorCode::Protocol
    );
}

#[test]
fn both_reasoning_delta_spellings_are_inside_the_pinned_subset() {
    // OpenAI streams `reasoning_summary_text`; vLLM v0.27.1 streams `reasoning_text`, observed in
    // beyond10x/harness `verification-report:openai-responses-on-vllm`.
    for name in [
        "response.reasoning_summary_text.delta",
        "response.reasoning_text.delta",
    ] {
        let payloads = vec![
            json!({"type": name, "delta": "Weighing "}),
            json!({"type": "response.completed", "response": {"status": "completed", "output": []}}),
        ];
        let decoding = decode_stream(&binding(), &payloads);
        assert_eq!(
            decoding.events,
            vec![StreamEvent::ReasoningDelta {
                text: "Weighing ".to_owned()
            }],
            "`{name}` must produce a reasoning delta"
        );
        assert!(decoding.result.is_ok());
    }
}

#[test]
fn the_vllm_reasoning_markers_raise_no_unknown_event_warning() {
    let payloads = vec![
        json!({"type": "response.reasoning_part.added", "summary_index": 0}),
        json!({"type": "response.reasoning_text.delta", "delta": "Weighing the options."}),
        json!({"type": "response.reasoning_text.done", "text": "Weighing the options."}),
        json!({"type": "response.reasoning_part.done", "summary_index": 0}),
        json!({"type": "response.completed", "response": {"status": "completed", "output": []}}),
    ];
    let decoding = decode_stream(&binding(), &payloads);
    assert_eq!(
        decoding.events,
        vec![StreamEvent::ReasoningDelta {
            text: "Weighing the options.".to_owned()
        }]
    );
    assert!(decoding.result.is_ok());
}

#[test]
fn an_event_outside_the_pinned_subset_is_preserved_and_reported() {
    let payloads = vec![
        json!({"type": "response.something_new", "data": 1}),
        json!({"type": "response.completed", "response": {"status": "completed"}}),
    ];
    let decoding = decode_stream(&binding(), &payloads);
    assert_eq!(
        decoding.events,
        vec![StreamEvent::Warning {
            code: "unknown-stream-event".to_owned(),
            message: "a stream event outside the pinned subset was preserved, not interpreted"
                .to_owned()
        }]
    );
    let outcome = decoding.result.expect("terminal");
    assert_eq!(
        outcome.items,
        vec![Item::Opaque {
            provenance: provenance(),
            payload: json!({"type": "response.something_new", "data": 1})
        }]
    );
}

#[test]
fn a_stream_that_never_reaches_a_terminal_response_refuses_and_keeps_its_output() {
    let payloads = vec![json!({"type": "response.output_text.delta", "delta": "Hel"})];
    let decoding = decode_stream(&binding(), &payloads);
    assert_eq!(
        decoding.events,
        vec![StreamEvent::TextDelta {
            text: "Hel".to_owned()
        }]
    );
    let error = decoding.result.expect_err("no terminal response");
    assert_eq!(error.code, ErrorCode::Protocol);
    assert_eq!(error.dispatch, Dispatch::Unknown);
    let observation = error.observation.as_ref().expect("the last bound evidence");
    assert_eq!(observation.binding, provenance());
    assert!(!observation.final_usage);
    error.validate_for(&provenance()).expect("bound evidence");
}

#[test]
fn a_failed_response_keeps_its_reported_usage_and_carries_no_upstream_text() {
    let payloads = vec![json!({"type": "response.failed", "response": {
        "id": "resp_1", "model": "example/Small-Model", "status": "failed",
        "error": {"code": "server_error", "message": "SECRET-UPSTREAM-TEXT"},
        "usage": {"input_tokens": 10, "output_tokens": 3}
    }})];
    let error = decode_stream(&binding(), &payloads)
        .result
        .expect_err("a failed response");
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert_eq!(error.dispatch, Dispatch::Accepted);
    assert!(
        !error.message.contains("SECRET-UPSTREAM-TEXT"),
        "diagnostics stay fixed: {}",
        error.message
    );
    let observation = error
        .observation
        .expect("terminal evidence survives the failure");
    assert!(observation.final_usage);
    let usage = observation.usage.expect("reported counters");
    assert_eq!(usage.input_tokens, Some(10));
    assert_eq!(usage.output_tokens, Some(3));
}

#[test]
fn the_three_provider_failure_classes_are_separated() {
    for (code, expected) in [
        ("server_error", ErrorCode::Unavailable),
        ("rate_limit_exceeded", ErrorCode::RateLimited),
        ("invalid_prompt", ErrorCode::Refused),
    ] {
        let payloads = vec![json!({"type": "error", "code": code, "message": "x"})];
        let error: Error = decode_stream(&binding(), &payloads)
            .result
            .expect_err("a provider failure");
        assert_eq!(error.code, expected, "`{code}`");
    }
}

#[test]
fn an_incomplete_response_keeps_the_reason_it_stopped_for() {
    let payloads = vec![json!({"type": "response.incomplete", "response": {
        "status": "incomplete", "incomplete_details": {"reason": "max_output_tokens"},
        "output": []}})];
    let outcome = decode_stream(&binding(), &payloads)
        .result
        .expect("terminal");
    assert_eq!(outcome.stop_reason, StopReason::MaxOutputTokens);

    let payloads = vec![json!({"type": "response.incomplete", "response": {
        "status": "incomplete", "incomplete_details": {"reason": "content_filter"},
        "output": []}})];
    let outcome = decode_stream(&binding(), &payloads)
        .result
        .expect("terminal");
    assert_eq!(
        outcome.stop_reason,
        StopReason::Incomplete {
            reason: "content_filter".to_owned()
        }
    );
}

#[test]
fn a_reason_outside_the_safe_class_is_not_relayed_verbatim() {
    let payloads = vec![json!({"type": "response.incomplete", "response": {
        "status": "incomplete",
        "incomplete_details": {"reason": "upstream said: SECRET-UPSTREAM-TEXT"},
        "output": []}})];
    let outcome = decode_stream(&binding(), &payloads)
        .result
        .expect("terminal");
    assert_eq!(
        outcome.stop_reason,
        StopReason::Incomplete {
            reason: "unrecognized".to_owned()
        }
    );
}

#[test]
fn contradictory_reported_counters_refuse_rather_than_saturate() {
    let payloads = vec![completed(&json!({
        "input_tokens": 10, "output_tokens": 3,
        "input_tokens_details": {"cached_tokens": 11}
    }))];
    let error = decode_stream(&binding(), &payloads)
        .result
        .expect_err("contradictory counters");
    assert_eq!(error.code, ErrorCode::Protocol);
}

#[test]
fn every_event_the_pin_declares_accepted_is_actually_interpreted() {
    // The class, not the instance: a name added to `ACCEPTED_STREAM_EVENTS` without a match arm
    // is a pin that claims to know an event it warns about, and this is what catches it.
    for name in llm_responses::ACCEPTED_STREAM_EVENTS {
        if *name == "error" || *name == "response.failed" {
            continue; // Both are refusals, asserted by their own cases above.
        }
        let payloads = vec![
            json!({"type": name}),
            json!({"type": "response.completed", "response": {"status": "completed", "output": []}}),
        ];
        let decoding = decode_stream(&binding(), &payloads);
        assert!(
            !decoding.events.iter().any(|event| matches!(
                event,
                StreamEvent::Warning { code, .. } if code == "unknown-stream-event"
            )),
            "`{name}` is declared accepted but was warned about"
        );
        assert!(decoding.result.is_ok(), "`{name}` broke the decode");
    }
}

#[test]
fn every_body_field_the_pin_declares_accepted_survives_ingress() {
    // Same class check on the other side: a field named in `ACCEPTED_BODY_FIELDS` that ingress
    // has no reader for would be accepted and then silently dropped.
    let mut request = tool_round_trip_request();
    request.instructions = "Keep instructions".to_owned();
    request.max_output_tokens = Some(128);
    request.sampling = Sampling {
        temperature: Some(0.5),
        top_p: Some(0.9),
        reasoning_effort: Some("medium".to_owned()),
    };
    request.tool_choice = ToolChoice::Named(ToolName::new("file_read").expect("name"));
    let body = project_request(&binding(), &request).expect("projects");
    let object = body.as_object().expect("an object");
    for field in llm_responses::ACCEPTED_BODY_FIELDS {
        assert!(
            object.contains_key(*field),
            "`{field}` is declared accepted but this fixture never exercises it"
        );
    }
    assert_eq!(
        ingest_request(&binding(), &body).expect("reads back"),
        request
    );
}

// ---------------------------------------------------------------------------------------------
// Class checks. Each one enumerates a whole rule rather than one instance of it, because six
// deliberate defects survived the first suite: every fixture above reports complete usage and
// stays inside the pinned subset, so no guard that fires outside it was ever reached.

/// Every neutral counter, driven absent in turn with the other four present.
///
/// One counter's default is invisible to a fixture that reports all of them, which is exactly how
/// `input_tokens` could be made to read zero while 43 scenarios and 28 cases stayed green.
#[test]
fn every_reported_counter_is_independently_optional() {
    let full = json!({
        "input_tokens": 1788, "output_tokens": 115,
        "input_tokens_details": {"cached_tokens": 1680},
        "output_tokens_details": {"reasoning_tokens": 42}
    });
    let read = |usage: &Usage| {
        [
            usage.input_tokens,
            usage.output_tokens,
            usage.cached_input_tokens,
            usage.reasoning_output_tokens,
        ]
    };
    let wire = [
        "input_tokens",
        "output_tokens",
        "input_tokens_details",
        "output_tokens_details",
    ];
    for (index, absent) in wire.iter().enumerate() {
        let mut usage = full.clone();
        usage.as_object_mut().expect("an object").remove(*absent);
        let outcome = decode_stream(&binding(), &[completed(&usage)])
            .result
            .expect("a terminal response");
        let reported = read(&outcome.observation.usage.expect("reported counters"));
        for (other, value) in reported.iter().enumerate() {
            if other == index {
                assert_eq!(
                    *value, None,
                    "`{absent}` absent must stay unknown, not default"
                );
            } else {
                assert!(
                    value.is_some(),
                    "`{absent}` absent must not erase counter {other}"
                );
            }
        }
    }
    // The fifth has no source on this wire at all, so it is absent even when everything is.
    let outcome = decode_stream(&binding(), &[completed(&full)])
        .result
        .expect("a terminal response");
    assert_eq!(
        outcome
            .observation
            .usage
            .expect("counters")
            .cache_creation_input_tokens,
        None
    );
}

/// One wire request body, with named fields overridden. A `null` removes the field.
fn wire_body(fields: &[(&str, Value)]) -> Value {
    let mut body = json!({
        "model": "example/Small-Model",
        "input": [{"type": "message", "role": "user",
                   "content": [{"type": "input_text", "text": "Hi"}]}],
        "tools": [], "stream": true, "store": false,
        "include": ["reasoning.encrypted_content"]
    });
    let object = body.as_object_mut().expect("an object");
    for (field, value) in fields {
        if value.is_null() {
            object.remove(*field);
        } else {
            object.insert((*field).to_owned(), value.clone());
        }
    }
    body
}

fn published(field: &str, value: Value) -> Value {
    let mut tool = json!({"type": "function", "name": "file_read", "description": "d",
                          "parameters": {"type": "object"}, "strict": false});
    tool[field] = value;
    json!([tool])
}

/// Every body-level refusal the ingress half promises, each reached by its own fixture.
///
/// A refusal with no case that reaches it is a guard nothing defends: five of the six mutations
/// that survived the first suite were guards of exactly this kind.
#[test]
fn every_ingress_body_refusal_the_contract_promises_is_reachable() {
    let cases: [(&str, Value, ErrorCode); 9] = [
        (
            "a top-level field outside the subset",
            wire_body(&[("previous_response_id", json!("resp_1"))]),
            ErrorCode::Unsupported,
        ),
        (
            "a body addressed to another model",
            wire_body(&[("model", json!("example/Large-Model"))]),
            ErrorCode::InvalidRequest,
        ),
        (
            "a body naming no model",
            wire_body(&[("model", Value::Null)]),
            ErrorCode::InvalidRequest,
        ),
        (
            "a non-streaming body",
            wire_body(&[("stream", json!(false))]),
            ErrorCode::Unsupported,
        ),
        (
            "provider-side storage",
            wire_body(&[("store", json!(true))]),
            ErrorCode::Unsupported,
        ),
        (
            "an include outside the subset",
            wire_body(&[("include", json!(["file_search_call.results"]))]),
            ErrorCode::Unsupported,
        ),
        (
            "a reasoning field outside the subset",
            wire_body(&[("reasoning", json!({"effort": "medium", "summary": "auto"}))]),
            ErrorCode::Unsupported,
        ),
        (
            "a tool choice outside the subset",
            wire_body(&[("tool_choice", json!("none"))]),
            ErrorCode::Unsupported,
        ),
        (
            "an output-token limit that is not a count",
            wire_body(&[("max_output_tokens", json!("many"))]),
            ErrorCode::InvalidRequest,
        ),
    ];
    assert_refused(&cases);
}

/// Every refusal the ingress half promises about `input` entries and published tools.
#[test]
fn every_ingress_content_refusal_the_contract_promises_is_reachable() {
    let cases: [(&str, Value, ErrorCode); 9] = [
        (
            "a standing instruction away from the head",
            wire_body(&[(
                "input",
                json!([
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Hi"}]},
            {"type": "message", "role": "developer",
             "content": [{"type": "input_text", "text": "Be brief"}]}]),
            )]),
            ErrorCode::Unsupported,
        ),
        (
            "a message role outside the subset",
            wire_body(&[(
                "input",
                json!([
            {"type": "message", "role": "system",
             "content": [{"type": "input_text", "text": "Hi"}]}]),
            )]),
            ErrorCode::Unsupported,
        ),
        (
            "content this version does not carry",
            wire_body(&[(
                "input",
                json!([
            {"type": "message", "role": "user",
             "content": [{"type": "input_image", "image_url": "https://a.invalid/a.png"}]}]),
            )]),
            ErrorCode::Unsupported,
        ),
        (
            "an unmodelled entry this side cannot attribute",
            wire_body(&[(
                "input",
                json!([
            {"type": "reasoning", "id": "rs_1", "summary": []}]),
            )]),
            ErrorCode::Unsupported,
        ),
        (
            "a tool result outside the pinned envelope",
            wire_body(&[(
                "input",
                json!([
            {"type": "function_call", "call_id": "call-1", "name": "file_read",
             "arguments": "{\"path\":\"README.md\"}"},
            {"type": "function_call_output", "call_id": "call-1", "output": "# Title"}]),
            )]),
            ErrorCode::Unsupported,
        ),
        (
            "function call arguments that are not a JSON object",
            wire_body(&[(
                "input",
                json!([
            {"type": "function_call", "call_id": "call-1", "name": "file_read",
             "arguments": "\"README.md\""}]),
            )]),
            ErrorCode::Protocol,
        ),
        (
            "a tool that is not a function",
            wire_body(&[("tools", json!([{"type": "web_search"}]))]),
            ErrorCode::Unsupported,
        ),
        (
            "a published tool field outside the subset",
            wire_body(&[("tools", published("container", json!({"type": "auto"})))]),
            ErrorCode::Unsupported,
        ),
        (
            "a strict tool schema",
            wire_body(&[("tools", published("strict", json!(true)))]),
            ErrorCode::Unsupported,
        ),
    ];
    assert_refused(&cases);
}

/// A tool name egress cannot publish is refused by ingress too, so the two directions agree.
#[test]
fn ingress_refuses_a_tool_name_egress_cannot_publish() {
    let body = wire_body(&[("tools", published("name", json!("workspace.read")))]);
    let error = ingest_request(&binding(), &body)
        .map(|request| request.tools)
        .expect_err("a gateway must not accept what it can never forward");
    assert_eq!(error.code, ErrorCode::Unsupported);
}

fn assert_refused(cases: &[(&str, Value, ErrorCode)]) {
    ingest_request(&binding(), &wire_body(&[]))
        .expect("the unedited body must be inside the subset, or a guard fires before the edit");
    for (why, body, expected) in cases {
        match ingest_request(&binding(), body) {
            Ok(request) => panic!("{why} was accepted, returning {:?}", request.items),
            Err(error) => {
                assert_eq!(error.code, *expected, "{why}");
                assert_eq!(error.dispatch, Dispatch::NotSent, "{why}");
            }
        }
    }
}

/// Every refusal the egress half promises, each reached by its own fixture.
#[test]
fn every_egress_refusal_the_contract_promises_is_reachable() {
    let mut foreign_protocol = provenance();
    foreign_protocol.protocol = Protocol::Messages;
    let cases: [(&str, Binding, TurnRequest, ErrorCode); 4] = [
        (
            "a binding of another protocol",
            Binding::new(foreign_protocol, id("example/Small-Model")),
            text_request(),
            ErrorCode::Unsupported,
        ),
        (
            "a request addressed to another model",
            binding(),
            TurnRequest {
                model: "large".to_owned(),
                ..text_request()
            },
            ErrorCode::InvalidRequest,
        ),
        (
            "a tool name this wire cannot publish",
            binding(),
            TurnRequest {
                tools: vec![ToolSpec {
                    name: ToolName::new("workspace.read").expect("printable"),
                    description: "d".to_owned(),
                    input_schema: json!({"type": "object"}),
                }],
                ..text_request()
            },
            ErrorCode::Unsupported,
        ),
        (
            "opaque state from another binding",
            binding(),
            {
                let mut request = text_request();
                request
                    .items
                    .push(opaque(provenance(), |p| p.binding_revision = id("rev-2")));
                request
            },
            ErrorCode::Unsupported,
        ),
    ];
    for (why, binding, request, expected) in cases {
        let error = project_request(&binding, &request).expect_err(why);
        assert_eq!(error.code, expected, "{why}");
        assert_eq!(error.dispatch, Dispatch::NotSent, "{why}");
    }
}

/// Every refusal the stream half promises, each reached by its own fixture, and each one still
/// reporting the evidence the response it refused had already carried.
#[test]
fn every_stream_refusal_the_contract_promises_is_reachable() {
    let oversize = format!("{{\"path\":\"{}\"}}", "x".repeat(70_000));
    let terminal = |item: Value| {
        vec![json!({"type": "response.completed", "response": {
            "id": "resp_1", "model": "example/Small-Model", "status": "completed",
            "output": [item], "usage": {"input_tokens": 10, "output_tokens": 3}}})]
    };
    let call = |arguments: &str| {
        terminal(
            json!({"type": "function_call", "call_id": "call-1", "name": "file_read",
                        "arguments": arguments}),
        )
    };
    // 0: the reported evidence is itself what refused, so there is none to attach.
    // 1: nothing terminal arrived, so only the binding is known.
    // 2: a terminal object arrived and its counters must survive its refusal.
    let cases: [(&str, Vec<Value>, ErrorCode, u8); 8] = [
        (
            "no terminal response",
            vec![json!({"type": "response.output_text.delta",
                                             "delta": "Hel"})],
            ErrorCode::Protocol,
            1,
        ),
        (
            "contradictory counters",
            vec![completed(&json!({
            "input_tokens": 10, "input_tokens_details": {"cached_tokens": 11}}))],
            ErrorCode::Protocol,
            0,
        ),
        (
            "a function call with no call id",
            terminal(json!({"type": "function_call", "name": "file_read", "arguments": "{}"})),
            ErrorCode::Protocol,
            2,
        ),
        (
            "a function call with no name",
            terminal(json!({"type": "function_call", "call_id": "call-1", "arguments": "{}"})),
            ErrorCode::Protocol,
            2,
        ),
        (
            "a function call with no arguments",
            terminal(json!({"type": "function_call", "call_id": "call-1", "name": "file_read"})),
            ErrorCode::Protocol,
            2,
        ),
        (
            "function call arguments that are not JSON",
            call("{\"path\": \"READ"),
            ErrorCode::Protocol,
            2,
        ),
        (
            "function call arguments over their bound",
            call(&oversize),
            ErrorCode::TooLarge,
            2,
        ),
        (
            "message content outside the pinned subset",
            terminal(json!({
            "type": "message", "role": "assistant",
            "content": [{"type": "refusal", "refusal": "I cannot help with that."}]})),
            ErrorCode::Unsupported,
            2,
        ),
    ];
    for (why, payloads, expected, evidence) in cases {
        let error = decode_stream(&binding(), &payloads)
            .result
            .map(|outcome| outcome.items)
            .expect_err(why);
        assert_eq!(error.code, expected, "{why}");
        error
            .validate_for(&provenance())
            .unwrap_or_else(|_| panic!("{why}: bound evidence"));
        if evidence == 0 {
            assert!(error.observation.is_none(), "{why}");
            continue;
        }
        let observation = error
            .observation
            .as_ref()
            .unwrap_or_else(|| panic!("{why}: every stream refusal carries its bound evidence"));
        assert_eq!(observation.binding, provenance(), "{why}");
        if evidence == 2 {
            let usage = observation
                .usage
                .as_ref()
                .unwrap_or_else(|| panic!("{why}: the counters the refused response reported"));
            assert_eq!(usage.input_tokens, Some(10), "{why}");
            assert_eq!(usage.output_tokens, Some(3), "{why}");
            assert!(observation.final_usage, "{why}");
        }
    }
}

/// Anything ingress accepts, egress can send back — and sends back unchanged.
///
/// This is the property `docs/responses.md` states as "a gateway ingress surface and an outgoing
/// client disagree about nothing". A check egress applies and ingress does not is a gateway that
/// accepts a request it can never forward; a field ingress accepts and never reads is a request
/// silently rewritten on the way out.
#[test]
fn every_body_ingress_accepts_reprojects_unchanged() {
    let bodies = [
        wire_body(&[]),
        wire_body(&[(
            "input",
            json!([
            {"type": "message", "role": "developer",
             "content": [{"type": "input_text", "text": "Be brief"}]},
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Hi"}]},
            {"type": "message", "role": "assistant",
             "content": [{"type": "output_text", "text": "Hello"}]}]),
        )]),
        wire_body(&[
            ("tools", published("strict", json!(false))),
            (
                "tool_choice",
                json!({"type": "function", "name": "file_read"}),
            ),
        ]),
        wire_body(&[
            ("max_output_tokens", json!(128)),
            ("temperature", json!(0.5)),
            ("top_p", json!(0.9)),
            ("reasoning", json!({"effort": "medium"})),
            ("include", json!(["reasoning.encrypted_content"])),
        ]),
    ];
    for body in bodies {
        let request = ingest_request(&binding(), &body)
            .unwrap_or_else(|error| panic!("inside the pinned subset: {error:?} for {body}"));
        let reprojected = project_request(&binding(), &request).unwrap_or_else(|error| {
            panic!("a gateway must forward what its own ingress accepted: {error:?}")
        });
        assert_eq!(
            reprojected, body,
            "a body ingress accepted came back out changed"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Second round of class checks. Each answers a rule the first round stated and did not drive.

/// Every event order a server may use, for one refusal, must report the same evidence.
///
/// The first round attached the terminal object's counters to every refusal raised **inside**
/// `terminate`, and that enumeration was complete for that function. It was also reachable only
/// from a payload list no server sends: the ordinary order announces the finished item with
/// `response.output_item.done` first, which is a different function. The class is the rule, not
/// the function — *a refusal carries the best evidence the stream had, whichever event raised
/// it* — so this drives both orders and demands they agree.
#[test]
fn a_refusal_reports_the_same_evidence_whichever_event_order_produced_it() {
    let oversize = format!("{{\"path\":\"{}\"}}", "x".repeat(70_000));
    let unreadable = [
        (
            "truncated by the output cap",
            "{\"path\": \"READ".to_owned(),
        ),
        (
            "a JSON scalar rather than an object",
            "\"README.md\"".to_owned(),
        ),
        ("over the argument bound", oversize),
    ];
    for (why, arguments) in unreadable {
        let item = json!({"type": "function_call", "id": "fc_1", "call_id": "call-1",
                          "name": "file_read", "arguments": arguments});
        let terminal = json!({"type": "response.completed", "response": {
            "id": "resp_1", "model": "example/Small-Model", "status": "completed",
            "output": [item.clone()],
            "usage": {"input_tokens": 1788, "output_tokens": 115}}});
        // The order a real server produces: the item is announced, then announced done, and the
        // terminal object arrives last, in the same stream.
        let announced = vec![
            json!({"type": "response.output_item.added", "item": {
                "type": "function_call", "id": "fc_1", "call_id": "call-1",
                "name": "file_read"}}),
            json!({"type": "response.output_item.done", "item": item}),
            terminal.clone(),
        ];
        let mut seen = Vec::new();
        for (order, payloads) in [
            ("terminal object only", vec![terminal]),
            ("the item announced first, as a server sends it", announced),
        ] {
            let error = decode_stream(&binding(), &payloads)
                .result
                .map(|outcome| outcome.items)
                .expect_err("an unreadable tool call refuses");
            let observation = error
                .observation
                .as_ref()
                .unwrap_or_else(|| panic!("{why} / {order}: the refusal carries no evidence"));
            let usage = observation
                .usage
                .as_ref()
                .unwrap_or_else(|| panic!("{why} / {order}: the refusal carries no counters"));
            assert_eq!(usage.input_tokens, Some(1788), "{why} / {order}");
            assert_eq!(usage.output_tokens, Some(115), "{why} / {order}");
            assert!(observation.final_usage, "{why} / {order}");
            seen.push((error.code, observation.upstream_model.clone()));
        }
        assert_eq!(seen[0], seen[1], "{why}: the two event orders disagree");
    }
}

/// Every field this projection writes with a fixed value, driven absent, empty and different.
///
/// Absence is not agreement. On this wire an omitted `stream` is a non-streaming request and an
/// omitted `store` is a stored conversation, so reading omission as assent answers a request
/// nobody made. `all` over an empty array is true, which is why `include: []` needs its own row
/// beside `include: [something-else]`.
#[test]
fn every_fixed_field_is_refused_unless_it_carries_exactly_the_pinned_value() {
    ingest_request(&binding(), &wire_body(&[]))
        .expect("the unedited body must be inside the subset, or a guard fires before the edit");
    let loose = json!([{"type": "function", "name": "file_read", "description": "d",
                        "parameters": {"type": "object"}}]);
    let cases: [(&str, Value); 10] = [
        ("`stream` absent", wire_body(&[("stream", Value::Null)])),
        ("`stream` false", wire_body(&[("stream", json!(false))])),
        ("`store` absent", wire_body(&[("store", Value::Null)])),
        ("`store` true", wire_body(&[("store", json!(true))])),
        ("`include` absent", wire_body(&[("include", Value::Null)])),
        ("`include` empty", wire_body(&[("include", json!([]))])),
        (
            "`include` different",
            wire_body(&[("include", json!(["file_search_call.results"]))]),
        ),
        ("a tool's `strict` absent", wire_body(&[("tools", loose)])),
        (
            "a tool's `strict` true",
            wire_body(&[("tools", published("strict", json!(true)))]),
        ),
        (
            "`include` duplicated",
            wire_body(&[(
                "include",
                json!(["reasoning.encrypted_content", "reasoning.encrypted_content"]),
            )]),
        ),
    ];
    for (why, body) in cases {
        match ingest_request(&binding(), &body) {
            Ok(request) => panic!("{why} was accepted, returning {:?}", request.items),
            Err(error) => assert_eq!(error.code, ErrorCode::Unsupported, "{why}"),
        }
    }
}

/// A message this version cannot represent part for part is refused, not silently joined.
///
/// `Item::UserText` holds one string. Two `input_text` parts concatenated into it come back out
/// as one part, which is a body a client did not send. Egress writes exactly one part, so
/// requiring exactly one on ingress is the symmetric rule rather than a new restriction.
#[test]
fn a_message_this_version_cannot_represent_part_for_part_is_refused() {
    ingest_request(&binding(), &wire_body(&[]))
        .expect("the unedited body must be inside the subset, or a guard fires before the edit");
    let cases: [(&str, Value); 3] = [
        (
            "two parts would be joined into one",
            json!([{"type": "message", "role": "user", "content": [
                {"type": "input_text", "text": "Hello, "},
                {"type": "input_text", "text": "world"}]}]),
        ),
        (
            "no parts at all would come back as one empty part",
            json!([{"type": "message", "role": "user", "content": []}]),
        ),
        (
            "a part this version does not carry",
            json!([{"type": "message", "role": "user", "content": [
                {"type": "input_image", "image_url": "https://a.invalid/a.png"}]}]),
        ),
    ];
    for (why, input) in cases {
        let body = wire_body(&[("input", input)]);
        match ingest_request(&binding(), &body) {
            Ok(request) => panic!("{why}: accepted, returning {:?}", request.items),
            Err(error) => assert_eq!(error.code, ErrorCode::Unsupported, "{why}"),
        }
    }
}

/// A provider failure is classified from its code, and evidence attaches after that decision.
///
/// The code is read first and kept. Counters that disagree with themselves narrow what the
/// refusal can carry — they are dropped, because invalid evidence is worse than none — and they
/// do not overwrite what the provider said about its own failure. A rate limit reported as a
/// non-retriable protocol error is a run ended on somebody else's temporary state.
#[test]
fn a_provider_failure_keeps_its_class_when_its_counters_disagree() {
    let contradictory = json!({"input_tokens": 10, "input_tokens_details": {"cached_tokens": 11}});
    for (code, expected) in [
        ("rate_limit_exceeded", ErrorCode::RateLimited),
        ("server_error", ErrorCode::Unavailable),
        ("invalid_prompt", ErrorCode::Refused),
    ] {
        let payloads = vec![json!({"type": "response.failed", "response": {
            "id": "resp_1", "model": "example/Small-Model", "status": "failed",
            "error": {"code": code, "message": "SECRET-UPSTREAM-TEXT"},
            "usage": contradictory}})];
        let error = decode_stream(&binding(), &payloads)
            .result
            .map(|outcome| outcome.items)
            .expect_err("a failed response refuses");
        assert_eq!(error.code, expected, "`{code}`");
        assert_eq!(error.dispatch, Dispatch::Accepted, "`{code}`");
        let observation = error
            .observation
            .as_ref()
            .unwrap_or_else(|| panic!("`{code}`: the binding and identifiers are still known"));
        assert_eq!(observation.binding, provenance(), "`{code}`");
        assert_eq!(
            observation.usage, None,
            "`{code}`: counters that disagree with themselves are dropped, never reported"
        );
        error
            .validate_for(&provenance())
            .unwrap_or_else(|_| panic!("`{code}`: bound evidence"));
    }
}
