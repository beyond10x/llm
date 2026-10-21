//! The client request fields Codex 0.160.0 sends, each under its written rule
//! (`docs/design.md` "Client request fields", Responses column).
//!
//! The two captured requests are `tests/fixtures/codex/`, described in that directory's
//! `README.md`. Every other body here is authored.

mod wire_support;

use std::collections::BTreeSet;

use llm_core::{
    ADVISORY_OMITTED, Advisory, CallId, Cancel, Dispatch, ErrorCode, Id, Item, Model, PromptCache,
    Protocol, Provenance, ReasoningSummary, StreamEvent, Thinking, ToolCall, ToolName, ToolSpec,
    TurnRequest, Unforwarded, VecSink, Verbosity,
};
use llm_responses::{
    ACCEPTED_BODY_FIELDS, Binding, Conversation, encode_request, ingest_request,
    omitted_advisories, project_request,
};
use serde_json::{Value, json};
use wire_support::{TEXT_STREAM, client, fixture, listener, serve};

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

/// The binding the captures were addressed to: Codex sent the model alias `code`.
fn codex_binding() -> Binding {
    Binding::new(provenance(), id("code"))
}

fn binding() -> Binding {
    Binding::new(provenance(), id("example/Small-Model"))
}

fn codex(name: &str) -> Value {
    serde_json::from_slice(&fixture(&format!("codex/{name}"))).expect("a JSON capture")
}

/// One authored wire body, with named fields overridden. A `null` removes the field.
fn wire_body(fields: &[(&str, Value)]) -> Value {
    let mut body = json!({
        "model": "example/Small-Model",
        "input": [{"type": "message", "role": "user",
                   "content": [{"type": "input_text", "text": "Hi"}]}],
        "tools": [{"type": "function", "name": "file_read", "description": "d",
                   "parameters": {"type": "object"}, "strict": false}],
        "stream": true, "store": false,
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

fn unforwarded(names: &[Unforwarded]) -> BTreeSet<Unforwarded> {
    names.iter().copied().collect()
}

/// Ingested, re-projected and ingested again: the turn that comes back.
fn round_trip(binding: &Binding, body: &Value) -> (TurnRequest, Value, TurnRequest) {
    let first = ingest_request(binding, body)
        .unwrap_or_else(|error| panic!("inside the subset: {error:?} for {body}"));
    let projected = project_request(binding, &first)
        .unwrap_or_else(|error| panic!("a gateway forwards what it accepted: {error:?}"));
    let second = ingest_request(binding, &projected)
        .unwrap_or_else(|error| panic!("egress output reads back: {error:?} for {projected}"));
    (first, projected, second)
}

fn text(value: &Value, pointer: &str) -> String {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("`{pointer}` is a string in the capture"))
        .to_owned()
}

#[test]
fn the_first_codex_turn_ingests_under_the_written_rules() {
    let body = codex("first-turn.json");
    let request = ingest_request(&codex_binding(), &body).expect("the first Codex turn ingests");
    let instructions = [
        text(&body, "/instructions"),
        text(&body, "/input/0/content/0/text"),
        text(&body, "/input/0/content/1/text"),
    ]
    .join("\n");
    assert_eq!(request.instructions, instructions);
    assert_eq!(request.model, "small");
    assert_eq!(
        request.items,
        [
            Item::user(text(&body, "/input/1/content/0/text")),
            Item::user("What does a.txt say?"),
        ]
    );
    assert_eq!(request.controls.parallel_tool_calls, Some(true));
    assert_eq!(request.controls.thinking, None);
    assert_eq!(request.sampling.reasoning_effort.as_deref(), Some("high"));
    let hints = &request.controls.hints;
    assert_eq!(hints.reasoning_summary, Some(ReasoningSummary::Auto));
    assert_eq!(
        hints.prompt_cache_key.as_deref(),
        Some("00000000-0000-7000-8000-000000000004")
    );
    assert_eq!(hints.verbosity, None);
    assert_eq!(hints.prompt_cache, None);
    assert_eq!(
        hints.unforwarded,
        unforwarded(&[Unforwarded::InputItemIds, Unforwarded::ClientMetadata])
    );
    assert_eq!(
        request
            .tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        [
            "exec_command",
            "write_stdin",
            "request_user_input",
            "view_image"
        ]
    );
    assert_eq!(
        omitted_advisories(&request),
        [Advisory::InputItemIds, Advisory::ClientMetadata]
    );
}

#[test]
fn the_second_codex_turn_ingests_the_reasoning_the_call_and_a_plain_string_result() {
    let body = codex("second-turn.json");
    let request = ingest_request(&codex_binding(), &body).expect("the second Codex turn ingests");
    let output = text(&body, "/input/5/output");
    assert_eq!(
        request.items[2..],
        [
            Item::UnattributedOpaque {
                protocol: Protocol::Responses,
                payload: body["input"][3].clone(),
            },
            Item::ToolCall(ToolCall {
                call_id: CallId::new("call_capture_1").expect("call id"),
                name: ToolName::new("exec_command").expect("tool name"),
                arguments: json!({"cmd": "cat a.txt"}),
            }),
            Item::ToolResult {
                call_id: CallId::new("call_capture_1").expect("call id"),
                output: Value::String(output),
                failed: false,
            },
        ]
    );
    assert_eq!(request.items.len(), 5);
    assert!(
        request
            .controls
            .hints
            .unforwarded
            .contains(&Unforwarded::InputItemIds)
    );
}

#[test]
fn each_carried_field_round_trips_to_an_equal_turn() {
    let cases: [(&str, Value); 8] = [
        (
            "`instructions` alone",
            wire_body(&[("instructions", json!("Be brief"))]),
        ),
        (
            "`parallel_tool_calls` true",
            wire_body(&[("parallel_tool_calls", json!(true))]),
        ),
        (
            "`parallel_tool_calls` false",
            wire_body(&[("parallel_tool_calls", json!(false))]),
        ),
        (
            "`prompt_cache_key`",
            wire_body(&[("prompt_cache_key", json!("conv-1"))]),
        ),
        (
            "`text.verbosity`",
            wire_body(&[("text", json!({"verbosity": "high"}))]),
        ),
        (
            "`reasoning.summary`",
            wire_body(&[("reasoning", json!({"summary": "concise"}))]),
        ),
        (
            "`reasoning.summary` beside an effort",
            wire_body(&[("reasoning", json!({"effort": "low", "summary": "detailed"}))]),
        ),
        (
            "effort `xhigh`",
            wire_body(&[("reasoning", json!({"effort": "xhigh"}))]),
        ),
    ];
    for (why, body) in cases {
        let (first, _, second) = round_trip(&binding(), &body);
        assert_eq!(first, second, "{why}");
    }
    let (first, projected, _) = round_trip(
        &binding(),
        &wire_body(&[("text", json!({"verbosity": "low"}))]),
    );
    assert_eq!(first.controls.hints.verbosity, Some(Verbosity::Low));
    assert_eq!(projected["text"], json!({"verbosity": "low"}));
}

/// The authored copy of the first turn with the `text` a model Codex knows was captured with.
#[test]
fn the_first_codex_turn_with_verbosity_round_trips_except_its_unforwarded_hints() {
    let mut body = codex("first-turn.json");
    body["text"] = json!({"verbosity": "low"});
    let (first, projected, second) = round_trip(&codex_binding(), &body);
    assert_eq!(first.controls.hints.verbosity, Some(Verbosity::Low));
    assert_eq!(projected["text"], json!({"verbosity": "low"}));
    assert_eq!(projected["parallel_tool_calls"], json!(true));
    assert_eq!(
        projected["prompt_cache_key"],
        json!("00000000-0000-7000-8000-000000000004")
    );
    assert_eq!(
        projected["reasoning"],
        json!({"effort": "high", "summary": "auto"})
    );
    // One-way: item ids and client metadata are not re-emitted, so they do not come back.
    assert!(projected.get("client_metadata").is_none(), "{projected}");
    assert!(
        projected["input"]
            .as_array()
            .expect("input")
            .iter()
            .all(|item| item.get("id").is_none()),
        "{projected}"
    );
    let mut expected = first.clone();
    expected.controls.hints.unforwarded.clear();
    assert_eq!(second, expected);
}

#[test]
fn the_neutral_effort_max_is_sent_as_xhigh() {
    let mut request = ingest_request(&binding(), &wire_body(&[])).expect("ingests");
    request.sampling.reasoning_effort = Some("max".to_owned());
    let body = project_request(&binding(), &request).expect("projects");
    assert_eq!(body["reasoning"], json!({"effort": "xhigh"}));
    let back = ingest_request(&binding(), &body).expect("reads back");
    assert_eq!(back.sampling.reasoning_effort.as_deref(), Some("xhigh"));
    request.sampling.reasoning_effort = None;
    request.controls.thinking = Some(Thinking::Disabled);
    let body = project_request(&binding(), &request).expect("projects");
    assert_eq!(body["reasoning"], json!({"effort": "none"}));
}

#[test]
fn the_one_way_rules_are_one_way() {
    // `instructions` with developer parts comes back as one developer message.
    let body = wire_body(&[
        ("instructions", json!("Be brief")),
        (
            "input",
            json!([
                {"type": "message", "role": "developer", "content": [
                    {"type": "input_text", "text": "first"},
                    {"type": "input_text", "text": ""}]},
                {"type": "message", "role": "developer", "content": [
                    {"type": "input_text", "text": "second"}]},
                {"type": "message", "role": "user",
                 "content": [{"type": "input_text", "text": "Hi"}]}]),
        ),
    ]);
    let (first, projected, second) = round_trip(&binding(), &body);
    assert_eq!(first.instructions, "Be brief\nfirst\n\nsecond");
    assert_eq!(
        projected["input"][0],
        json!({"type": "message", "role": "developer",
               "content": [{"type": "input_text", "text": "Be brief\nfirst\n\nsecond"}]})
    );
    assert_eq!(first, second);

    // Item ids and client metadata are not re-emitted and are reported omitted.
    let body = wire_body(&[
        (
            "input",
            json!([{"type": "message", "id": "msg_1", "role": "user",
                    "content": [{"type": "input_text", "text": "Hi"}]}]),
        ),
        ("client_metadata", json!({"turn_id": "t-1"})),
    ]);
    let (first, projected, second) = round_trip(&binding(), &body);
    assert_eq!(
        omitted_advisories(&first),
        [Advisory::InputItemIds, Advisory::ClientMetadata]
    );
    assert!(projected.get("client_metadata").is_none(), "{projected}");
    assert!(projected["input"][0].get("id").is_none(), "{projected}");
    assert_eq!(second.controls.hints.unforwarded, BTreeSet::new());

    // A plain-string result comes back in the pinned envelope.
    let body = wire_body(&[(
        "input",
        json!([
            {"type": "function_call", "call_id": "call-1", "name": "file_read",
             "arguments": "{\"path\":\"README.md\"}"},
            {"type": "function_call_output", "call_id": "call-1", "output": "# Title"}]),
    )]);
    let (first, projected, second) = round_trip(&binding(), &body);
    assert_eq!(
        first.items[1],
        Item::ToolResult {
            call_id: CallId::new("call-1").expect("call id"),
            output: json!("# Title"),
            failed: false,
        }
    );
    assert_eq!(
        projected["input"][1]["output"],
        json!("{\"ok\":true,\"output\":\"# Title\"}")
    );
    assert_eq!(first, second);
}

/// A string that is JSON but not exactly the envelope is a plain string; the envelope is read.
#[test]
fn only_exactly_the_pinned_envelope_is_read_as_one() {
    let result = |output: Value| {
        let body = wire_body(&[(
            "input",
            json!([
                {"type": "function_call", "call_id": "call-1", "name": "file_read",
                 "arguments": "{}"},
                {"type": "function_call_output", "call_id": "call-1", "output": output}]),
        )]);
        ingest_request(&binding(), &body).map(|request| request.items[1].clone())
    };
    let plain = |text: &str| Item::ToolResult {
        call_id: CallId::new("call-1").expect("call id"),
        output: Value::String(text.to_owned()),
        failed: false,
    };
    assert_eq!(
        result(json!("{\"ok\":false,\"output\":7}")),
        Ok(Item::ToolResult {
            call_id: CallId::new("call-1").expect("call id"),
            output: json!(7),
            failed: true,
        })
    );
    for text in [
        "{\"ok\":true}",
        "{\"ok\":\"yes\",\"output\":1}",
        "{\"ok\":true,\"output\":1,\"extra\":2}",
        "[1,2]",
        "42",
        "",
    ] {
        assert_eq!(result(json!(text)), Ok(plain(text)), "{text}");
    }
    let error = result(json!([{"type": "input_text", "text": "x"}]))
        .expect_err("a non-string output stays refused");
    assert_eq!(error.code, ErrorCode::InvalidRequest);
}

#[test]
fn omitted_advisories_names_exactly_the_present_unsent_hints_in_order() {
    let mut request = ingest_request(&binding(), &wire_body(&[])).expect("ingests");
    assert_eq!(omitted_advisories(&request), []);
    let hints = &mut request.controls.hints;
    hints.prompt_cache_key = Some("conv-1".to_owned());
    hints.verbosity = Some(Verbosity::Low);
    hints.reasoning_summary = Some(ReasoningSummary::Auto);
    assert_eq!(omitted_advisories(&request), []);
    let hints = &mut request.controls.hints;
    hints.unforwarded = unforwarded(&[Unforwarded::ClientMetadata, Unforwarded::InputItemIds]);
    hints.prompt_cache = Some(PromptCache::default());
    assert_eq!(
        omitted_advisories(&request),
        [
            Advisory::PromptCache,
            Advisory::InputItemIds,
            Advisory::ClientMetadata
        ]
    );
}

#[test]
fn egress_refuses_what_this_wire_cannot_hold() {
    let base = ingest_request(&binding(), &wire_body(&[])).expect("ingests");
    let mut budget = base.clone();
    budget.controls.thinking = Some(Thinking::Enabled {
        budget_tokens: 1024,
    });
    let mut contradiction = base.clone();
    contradiction.controls.thinking = Some(Thinking::Disabled);
    contradiction.sampling.reasoning_effort = Some("high".to_owned());
    for (why, request) in [
        ("an enabled thinking budget", budget),
        ("disabled thinking beside an effort", contradiction),
    ] {
        let error = project_request(&binding(), &request).expect_err(why);
        assert_eq!(error.code, ErrorCode::Unsupported, "{why}");
        assert_eq!(error.dispatch, Dispatch::NotSent, "{why}");
    }

    let conversation = Conversation::new(id("conv-1"));
    let mut keyed = base.clone();
    keyed.controls.hints.prompt_cache_key = Some("conv-2".to_owned());
    let error = encode_request(&binding(), &keyed, Some(&conversation))
        .expect_err("two cache keys for one request");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        error.message,
        "the turn's prompt_cache_key differs from the client's conversation"
    );
    keyed.controls.hints.prompt_cache_key = Some("conv-1".to_owned());
    for request in [&keyed, &base] {
        let bytes = encode_request(&binding(), request, Some(&conversation)).expect("one key");
        let body: Value = serde_json::from_slice(&bytes).expect("JSON");
        assert_eq!(body["prompt_cache_key"], json!("conv-1"));
    }
    let bytes = encode_request(&binding(), &keyed, None).expect("no conversation");
    let body: Value = serde_json::from_slice(&bytes).expect("JSON");
    assert_eq!(body["prompt_cache_key"], json!("conv-1"));
}

fn refused(body: &Value) -> llm_core::Error {
    match ingest_request(&binding(), body) {
        Ok(request) => panic!("accepted, returning {request:?} for {body}"),
        Err(error) => {
            assert_eq!(error.dispatch, Dispatch::NotSent, "{body}");
            error
        }
    }
}

fn by_path(path: &str) -> String {
    format!("request body carries a field outside the pinned subset: {path}")
}

#[test]
fn a_key_outside_the_subset_is_refused_by_its_path_at_every_level() {
    ingest_request(&binding(), &wire_body(&[])).expect("the unedited body is inside the subset");
    let user = |extra: Value| {
        let mut part = json!({"type": "input_text", "text": "Hi"});
        let mut message = json!({"type": "message", "role": "user"});
        for (key, value) in extra.as_object().expect("an object") {
            if let Some(key) = key.strip_prefix("part.") {
                part[key] = value.clone();
            } else {
                message[key] = value.clone();
            }
        }
        message["content"] = json!([part]);
        json!([message])
    };
    let call = json!({"type": "function_call", "call_id": "call-1", "name": "file_read",
                      "arguments": "{}"});
    let mut call_status = call.clone();
    call_status["status"] = json!("completed");
    let mut output_status =
        json!({"type": "function_call_output", "call_id": "call-1", "output": "x"});
    output_status["status"] = json!("completed");
    let mut tool = wire_body(&[])["tools"][0].clone();
    tool["container"] = json!({"type": "auto"});
    let cases: Vec<(Value, &str)> = vec![
        (
            wire_body(&[("previous_response_id", json!("resp_1"))]),
            "previous_response_id",
        ),
        (wire_body(&[("metadata", json!({}))]), "metadata"),
        (
            wire_body(&[("input", user(json!({"status": "completed"})))]),
            "input[].status",
        ),
        (
            wire_body(&[("input", user(json!({"part.foo": 1})))]),
            "input[].content[].foo",
        ),
        (
            wire_body(&[("input", user(json!({"part.annotations": []})))]),
            "input[].content[].annotations",
        ),
        (
            wire_body(&[(
                "input",
                json!([{"type": "message", "role": "assistant", "content": [
                    {"type": "output_text", "text": "Hi", "annotations": [{"type": "url"}]}]}]),
            )]),
            "input[].content[].annotations",
        ),
        (
            wire_body(&[(
                "input",
                json!([{"type": "message", "role": "developer", "content": [
                    {"type": "input_text", "text": "Hi", "cache": true}]}]),
            )]),
            "input[].content[].cache",
        ),
        (
            wire_body(&[("input", json!([call_status]))]),
            "input[].status",
        ),
        (
            wire_body(&[("input", json!([call, output_status]))]),
            "input[].status",
        ),
        (
            wire_body(&[(
                "tool_choice",
                json!({"type": "function", "name": "file_read", "foo": 1}),
            )]),
            "tool_choice.foo",
        ),
        (wire_body(&[("tools", json!([tool]))]), "tools[].container"),
        (
            wire_body(&[("reasoning", json!({"generate_summary": "auto"}))]),
            "reasoning.generate_summary",
        ),
        (
            wire_body(&[("text", json!({"format": {"type": "text"}}))]),
            "text.format",
        ),
        (
            wire_body(&[("text", json!({"verbosity": "low", "foo": 1}))]),
            "text.foo",
        ),
        (wire_body(&[("x y", json!(1))]), "?"),
    ];
    for (body, path) in cases {
        let error = refused(&body);
        assert_eq!(error.code, ErrorCode::Unsupported, "{path}");
        assert_eq!(error.message, by_path(path), "{path}");
    }
}

#[test]
fn a_tool_of_another_type_is_refused_by_that_type_when_it_is_a_known_one() {
    for kind in [
        "web_search",
        "custom",
        "namespace",
        "tool_search",
        "local_shell",
        "file_search",
        "computer_use_preview",
        "mcp",
        "image_generation",
        "code_interpreter",
    ] {
        let error = refused(&wire_body(&[("tools", json!([{"type": kind}]))]));
        assert_eq!(error.code, ErrorCode::Unsupported, "{kind}");
        assert_eq!(
            error.message,
            format!("a published tool of type `{kind}` is outside the pinned subset")
        );
    }
    for kind in [json!("web_search_preview_x"), json!(7), Value::Null] {
        let mut tool = json!({"name": "x"});
        if !kind.is_null() {
            tool["type"] = kind.clone();
        }
        let error = refused(&wire_body(&[("tools", json!([tool]))]));
        assert_eq!(error.code, ErrorCode::Unsupported, "{kind}");
        assert_eq!(
            error.message, "only function tools are inside the pinned subset",
            "{kind}"
        );
    }
}

#[test]
fn an_instruction_after_the_conversation_began_is_refused() {
    let error = refused(&wire_body(&[(
        "input",
        json!([
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Hi"}]},
            {"type": "message", "role": "developer",
             "content": [{"type": "input_text", "text": "Be brief"}]}]),
    )]));
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        error.message,
        "a standing instruction is carried once, at the head of `input`"
    );
    for (why, content) in [
        (
            "a developer part of another type",
            json!([{"type": "output_text", "text": "x"}]),
        ),
        ("a developer message with no parts", json!([])),
    ] {
        let error = refused(&wire_body(&[(
            "input",
            json!([{"type": "message", "role": "developer", "content": content}]),
        )]));
        assert_eq!(error.code, ErrorCode::Unsupported, "{why}");
    }
}

#[test]
fn a_field_of_the_wrong_type_is_invalid() {
    let message_id = json!([{"type": "message", "id": 7, "role": "user",
                             "content": [{"type": "input_text", "text": "Hi"}]}]);
    let cases: [(&str, Value); 9] = [
        ("instructions", wire_body(&[("instructions", json!(["a"]))])),
        (
            "parallel_tool_calls",
            wire_body(&[("parallel_tool_calls", json!("yes"))]),
        ),
        (
            "prompt_cache_key",
            wire_body(&[("prompt_cache_key", json!(4))]),
        ),
        (
            "client_metadata",
            wire_body(&[("client_metadata", json!("x"))]),
        ),
        ("text", wire_body(&[("text", json!("low"))])),
        (
            "text.verbosity",
            wire_body(&[("text", json!({"verbosity": 1}))]),
        ),
        (
            "reasoning.summary",
            wire_body(&[("reasoning", json!({"summary": true}))]),
        ),
        (
            "reasoning.effort",
            wire_body(&[("reasoning", json!({"effort": 3}))]),
        ),
        ("a message id", wire_body(&[("input", message_id)])),
    ];
    for (why, body) in cases {
        assert_eq!(refused(&body).code, ErrorCode::InvalidRequest, "{why}");
    }
    for (why, body) in [
        (
            "a verbosity outside the three",
            wire_body(&[("text", json!({"verbosity": "max"}))]),
        ),
        (
            "a summary outside the three",
            wire_body(&[("reasoning", json!({"summary": "none"}))]),
        ),
    ] {
        assert_eq!(refused(&body).code, ErrorCode::Unsupported, "{why}");
    }
    let request = ingest_request(
        &binding(),
        &wire_body(&[("reasoning", json!({"effort": "low", "summary": null}))]),
    )
    .expect("a null summary is absent");
    assert_eq!(request.controls.hints.reasoning_summary, None);
}

#[test]
fn the_accepted_body_fields_are_sorted_and_name_the_client_fields() {
    let mut sorted = ACCEPTED_BODY_FIELDS.to_vec();
    sorted.sort_unstable();
    assert_eq!(ACCEPTED_BODY_FIELDS, sorted.as_slice());
    for field in [
        "client_metadata",
        "instructions",
        "parallel_tool_calls",
        "prompt_cache_key",
        "text",
    ] {
        assert!(ACCEPTED_BODY_FIELDS.contains(&field), "{field}");
    }
}

fn warnings(events: &[StreamEvent]) -> Vec<(String, String)> {
    events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::Warning { code, message } => Some((code.clone(), message.clone())),
            _ => None,
        })
        .collect()
}

fn hinted_request() -> TurnRequest {
    let mut request = TurnRequest::new("small", vec![Item::user("Hi")]);
    request.controls.hints.unforwarded =
        unforwarded(&[Unforwarded::InputItemIds, Unforwarded::ClientMetadata]);
    request.controls.hints.verbosity = Some(Verbosity::Low);
    request
}

#[tokio::test]
async fn the_client_warns_once_after_a_successful_turn_naming_what_it_omitted() {
    let (listener, url) = listener().await;
    let server = serve(listener, vec![TEXT_STREAM.to_vec()]);
    let mut sink = VecSink::new(16, 4096);
    client(&url)
        .turn(&hinted_request(), &mut sink, &Cancel::new())
        .await
        .expect("a decoded turn");
    server.await.expect("the fixture server");
    assert_eq!(
        warnings(sink.events()),
        [(
            ADVISORY_OMITTED.to_owned(),
            "Responses carries no input-item-ids, client-metadata; the advisory hint was not sent"
                .to_owned()
        )]
    );
    assert!(
        matches!(sink.events().last(), Some(StreamEvent::Warning { .. })),
        "the warning comes after the turn's own events"
    );
}

#[tokio::test]
async fn the_client_does_not_warn_when_nothing_was_omitted_or_the_turn_failed() {
    let failed: &[u8] = b"data: {\"type\":\"response.failed\",\"response\":{\"id\":\"resp_1\",\"status\":\"failed\",\"error\":{\"code\":\"server_error\",\"message\":\"x\"}}}\n\n";
    let (listener, url) = listener().await;
    let server = serve(listener, vec![TEXT_STREAM.to_vec(), failed.to_vec()]);
    let client = client(&url);
    let mut quiet = hinted_request();
    quiet.controls.hints.unforwarded.clear();
    let mut sink = VecSink::new(16, 4096);
    client
        .turn(&quiet, &mut sink, &Cancel::new())
        .await
        .expect("a decoded turn");
    assert_eq!(warnings(sink.events()), []);
    let mut sink = VecSink::new(16, 4096);
    client
        .turn(&hinted_request(), &mut sink, &Cancel::new())
        .await
        .expect_err("a failed turn");
    server.await.expect("the fixture server");
    assert_eq!(warnings(sink.events()), []);
}

#[tokio::test]
async fn a_sink_refusing_the_warning_fails_the_turn_with_the_sinks_error() {
    let (listener, url) = listener().await;
    let server = serve(listener, vec![TEXT_STREAM.to_vec()]);
    // Room for the one text delta and nothing more.
    let mut sink = VecSink::new(1, 4096);
    let error = client(&url)
        .turn(&hinted_request(), &mut sink, &Cancel::new())
        .await
        .expect_err("the sink refused the warning");
    server.await.expect("the fixture server");
    assert_eq!(error.code, ErrorCode::TooLarge);
    assert_eq!(error.message, "stream sink event bound reached");
    assert_eq!(sink.events().len(), 1);
}

#[test]
fn tool_specs_read_from_the_capture_keep_their_schemas() {
    let body = codex("first-turn.json");
    let request = ingest_request(&codex_binding(), &body).expect("ingests");
    let first: &ToolSpec = &request.tools[0];
    assert_eq!(first.input_schema, body["tools"][0]["parameters"]);
}

/// A function tool without `parameters` is refused, as the chat ingress refuses it; a present
/// `null` was and still is refused by the turn's own schema check.
#[test]
fn a_tool_without_a_parameters_schema_is_refused() {
    let mut tool = wire_body(&[])["tools"][0].clone();
    tool.as_object_mut()
        .expect("an object")
        .remove("parameters");
    let error = ingest_request(&binding(), &wire_body(&[("tools", json!([tool]))]))
        .expect_err("an omitted schema is refused");
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert_eq!(
        error.message,
        "a published tool omits its parameters schema"
    );

    tool["parameters"] = Value::Null;
    let error = ingest_request(&binding(), &wire_body(&[("tools", json!([tool]))]))
        .expect_err("a null schema is refused");
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert_eq!(
        error.message,
        "tool schema must be a JSON Schema object or boolean"
    );
}
