//! The neutral controls, the one effort table and the turn envelope's two versions
//! (`docs/design.md` "Client request fields").
use llm_core::{
    ADVISORY_OMITTED, Advisory, CacheTtl, CallId, Capabilities, Controls, ErrorCode, Hints, Id,
    Item, PromptCache, Protocol, Provenance, ReasoningSummary, Sampling, StreamEvent, Thinking,
    ToolCall, ToolChoice, ToolName, ToolSpec, TurnDocument, TurnRequest, Unforwarded, Verbosity,
    advisory_warning, field_path, wire_effort,
};
use serde_json::json;

fn binding(protocol: Protocol) -> Provenance {
    Provenance {
        protocol,
        provider: Id::new("lab").unwrap(),
        account: Id::new("account").unwrap(),
        endpoint: Id::new("endpoint").unwrap(),
        model: Id::new("code").unwrap(),
        binding_revision: Id::new("revision-1").unwrap(),
    }
}

fn efforts(names: &[&str]) -> Capabilities {
    let mut capabilities = Capabilities::text(32768, 4096);
    capabilities.tools = true;
    capabilities.tool_choice = true;
    capabilities.reasoning_efforts = names.iter().map(|&name| name.to_owned()).collect();
    capabilities
}

fn tool(name: &str) -> ToolSpec {
    ToolSpec {
        name: ToolName::new(name).unwrap(),
        description: "Reads a file".to_owned(),
        input_schema: json!({"type":"object","properties":{"path":{"type":"string"}}}),
    }
}

fn turn(effort: Option<&str>, thinking: Option<Thinking>) -> TurnRequest {
    let mut request = TurnRequest::new("code", vec![Item::user("What does a.txt say?")]);
    request.sampling.reasoning_effort = effort.map(ToOwned::to_owned);
    request.controls.thinking = thinking;
    request
}

/// One turn with tools, sampling, a named choice and every item kind except unbound opaque
/// state, which `validate` would refuse to persist only at `validate_for`.
fn every_kind() -> TurnRequest {
    let call = CallId::new("call-1").unwrap();
    let mut request = TurnRequest::new(
        "code",
        vec![
            Item::user("What does a.txt say?"),
            Item::assistant("Reading it."),
            Item::ToolCall(ToolCall {
                call_id: call.clone(),
                name: ToolName::new("read").unwrap(),
                arguments: json!({"path":"a.txt"}),
            }),
            Item::ToolResult {
                call_id: call,
                output: json!("hello"),
                failed: false,
            },
            Item::Opaque {
                provenance: binding(Protocol::Messages),
                payload: json!({"type":"thinking","thinking":"t","signature":"s"}),
            },
            Item::UnattributedOpaque {
                protocol: Protocol::Responses,
                payload: json!({"type":"reasoning","id":"rs_1"}),
            },
        ],
    );
    "Be brief.".clone_into(&mut request.instructions);
    request.tools.push(tool("read"));
    request.max_output_tokens = Some(512);
    request.sampling = Sampling {
        temperature: Some(0.5),
        top_p: Some(0.9),
        reasoning_effort: Some("high".to_owned()),
    };
    request.tool_choice = ToolChoice::Named(ToolName::new("read").unwrap());
    request
}

/// What llm 0.6.0 (`5657d2f5`) wrote for [`every_kind`], captured by serializing the same turn
/// with that revision's `TurnDocument::new`.
const LLM_0_6_0_BYTES: &str = include_str!("fixtures/llm-0.6.0-turn.json");

#[test]
fn a_turn_without_controls_is_written_as_llm_0_6_0_wrote_it() {
    let written = serde_json::to_string(&TurnDocument::new(every_kind())).unwrap();
    assert_eq!(written, LLM_0_6_0_BYTES.trim_end());
    let read: TurnDocument = serde_json::from_str(LLM_0_6_0_BYTES).unwrap();
    assert_eq!(read.request, every_kind());
}

#[test]
fn a_turn_with_controls_is_llm_turn_4_and_reads_back_equal() {
    let mut request = every_kind();
    request.controls = Controls {
        thinking: Some(Thinking::Enabled {
            budget_tokens: 2048,
        }),
        parallel_tool_calls: Some(false),
        return_unpublished_calls: true,
        hints: Hints {
            prompt_cache: Some(PromptCache {
                ttl: Some(CacheTtl::OneHour),
            }),
            prompt_cache_key: Some("session-1".to_owned()),
            verbosity: Some(Verbosity::Low),
            reasoning_summary: Some(ReasoningSummary::Auto),
            anthropic_beta: Vec::new(),
            unforwarded: [Unforwarded::InputItemIds, Unforwarded::ClientMetadata].into(),
        },
    };
    let written = serde_json::to_value(TurnDocument::new(request.clone())).unwrap();
    assert_eq!(written["format"], json!("llm.turn/4"));
    assert_eq!(
        written["request"]["controls"],
        json!({
            "thinking": {"mode": "enabled", "budget_tokens": 2048},
            "parallel_tool_calls": false,
            "hints": {
                "prompt_cache": {"ttl": "1h"},
                "prompt_cache_key": "session-1",
                "verbosity": "low",
                "reasoning_summary": "auto",
                "unforwarded": ["input-item-ids", "client-metadata"],
            },
            "return_unpublished_calls": true,
        })
    );
    let read: TurnDocument = serde_json::from_value(written).unwrap();
    assert_eq!(read.request, request);
}

#[test]
fn a_version_that_disagrees_with_its_request_is_refused() {
    let mut with_controls = serde_json::to_value(TurnDocument::new({
        let mut request = every_kind();
        request.controls.parallel_tool_calls = Some(true);
        request
    }))
    .unwrap();
    with_controls["format"] = json!("llm.turn/3");
    assert!(serde_json::from_value::<TurnDocument>(with_controls).is_err());

    let mut without = serde_json::to_value(TurnDocument::new(every_kind())).unwrap();
    without["format"] = json!("llm.turn/4");
    assert!(serde_json::from_value::<TurnDocument>(without).is_err());

    let mut unknown = serde_json::to_value(TurnDocument::new(every_kind())).unwrap();
    unknown["request"]["controls"] = json!({"thinking": {"mode": "enabled"}});
    unknown["format"] = json!("llm.turn/4");
    assert!(serde_json::from_value::<TurnDocument>(unknown).is_err());

    let mut unknown_key = serde_json::to_value(TurnDocument::new(every_kind())).unwrap();
    unknown_key["request"]["controls"] = json!({"hints": {"verbosity": "low", "tone": "dry"}});
    unknown_key["format"] = json!("llm.turn/4");
    assert!(serde_json::from_value::<TurnDocument>(unknown_key).is_err());
}

// The reasoning table, row by row, on each wire.

#[test]
fn effort_max_is_sent_as_xhigh_on_chat_and_responses_and_as_max_on_messages() {
    let request = turn(Some("max"), None);
    assert_eq!(
        wire_effort(Protocol::Messages, &request)
            .unwrap()
            .as_deref(),
        Some("max")
    );
    for protocol in [Protocol::ChatCompletions, Protocol::Responses] {
        assert_eq!(
            wire_effort(protocol, &request).unwrap().as_deref(),
            Some("xhigh"),
            "{protocol:?}"
        );
    }
}

#[test]
fn another_effort_is_sent_under_its_own_name_on_every_wire() {
    for effort in ["low", "medium", "high", "xhigh", "none", "minimal"] {
        let request = turn(Some(effort), None);
        for protocol in [
            Protocol::Messages,
            Protocol::ChatCompletions,
            Protocol::Responses,
        ] {
            assert_eq!(
                wire_effort(protocol, &request).unwrap().as_deref(),
                Some(effort),
                "{effort} on {protocol:?}"
            );
        }
    }
    for protocol in [
        Protocol::Messages,
        Protocol::ChatCompletions,
        Protocol::Responses,
    ] {
        assert_eq!(wire_effort(protocol, &turn(None, None)).unwrap(), None);
    }
}

#[test]
fn adaptive_thinking_changes_no_effort_on_any_wire() {
    for (effort, chat) in [
        (None, None),
        (Some("max"), Some("xhigh")),
        (Some("low"), Some("low")),
    ] {
        let request = turn(effort, Some(Thinking::Adaptive));
        assert_eq!(
            wire_effort(Protocol::Messages, &request)
                .unwrap()
                .as_deref(),
            effort
        );
        for protocol in [Protocol::ChatCompletions, Protocol::Responses] {
            assert_eq!(wire_effort(protocol, &request).unwrap().as_deref(), chat);
        }
    }
}

#[test]
fn a_thinking_budget_reaches_messages_and_is_refused_on_chat_and_responses() {
    let request = turn(
        Some("high"),
        Some(Thinking::Enabled {
            budget_tokens: 1024,
        }),
    );
    assert_eq!(
        wire_effort(Protocol::Messages, &request)
            .unwrap()
            .as_deref(),
        Some("high")
    );
    for (protocol, message) in [
        (
            Protocol::ChatCompletions,
            "Chat Completions has no thinking budget",
        ),
        (Protocol::Responses, "Responses has no thinking budget"),
    ] {
        let error = wire_effort(protocol, &request).unwrap_err();
        assert_eq!(
            (error.code, error.message.as_str()),
            (ErrorCode::Unsupported, message)
        );
    }
}

#[test]
fn disabled_thinking_alone_is_effort_none_on_chat_and_responses() {
    for effort in [None, Some("none")] {
        let request = turn(effort, Some(Thinking::Disabled));
        assert_eq!(
            wire_effort(Protocol::Messages, &request)
                .unwrap()
                .as_deref(),
            effort
        );
        for protocol in [Protocol::ChatCompletions, Protocol::Responses] {
            assert_eq!(
                wire_effort(protocol, &request).unwrap().as_deref(),
                Some("none")
            );
        }
    }
}

#[test]
fn disabled_thinking_beside_an_effort_reaches_messages_and_is_refused_elsewhere() {
    let request = turn(Some("low"), Some(Thinking::Disabled));
    assert_eq!(
        wire_effort(Protocol::Messages, &request)
            .unwrap()
            .as_deref(),
        Some("low")
    );
    for (protocol, message) in [
        (
            Protocol::ChatCompletions,
            "Chat Completions cannot hold disabled thinking beside an effort",
        ),
        (
            Protocol::Responses,
            "Responses cannot hold disabled thinking beside an effort",
        ),
    ] {
        let error = wire_effort(protocol, &request).unwrap_err();
        assert_eq!(
            (error.code, error.message.as_str()),
            (ErrorCode::Unsupported, message)
        );
    }
}

#[test]
fn capabilities_are_checked_against_the_effort_the_wire_receives() {
    let request = turn(Some("max"), None);
    for protocol in [Protocol::ChatCompletions, Protocol::Responses] {
        request
            .validate_for(&binding(protocol), &efforts(&["high", "xhigh"]))
            .unwrap();
        let error = request
            .validate_for(&binding(protocol), &efforts(&["high", "max"]))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Unsupported);
    }
    let error = request
        .validate_for(&binding(Protocol::Messages), &efforts(&["high", "xhigh"]))
        .unwrap_err();
    assert_eq!(
        (error.code, error.message.as_str()),
        (
            ErrorCode::Unsupported,
            "binding does not support a requested tool or sampling setting"
        )
    );
    request
        .validate_for(&binding(Protocol::Messages), &efforts(&["max"]))
        .unwrap();
    let disabled = turn(None, Some(Thinking::Disabled));
    assert!(
        disabled
            .validate_for(&binding(Protocol::ChatCompletions), &efforts(&["high"]))
            .is_err()
    );
    disabled
        .validate_for(&binding(Protocol::ChatCompletions), &efforts(&["none"]))
        .unwrap();
}

#[test]
fn invalid_controls_are_refused_each_with_its_own_diagnostic() {
    let refused = |request: TurnRequest| {
        let error = request.validate().unwrap_err();
        (error.code, error.message)
    };
    assert_eq!(
        refused(turn(None, Some(Thinking::Enabled { budget_tokens: 0 }))),
        (
            ErrorCode::InvalidRequest,
            "thinking budget cannot be zero".to_owned()
        )
    );
    let mut parallel = turn(None, None);
    parallel.controls.parallel_tool_calls = Some(true);
    assert_eq!(
        refused(parallel.clone()),
        (
            ErrorCode::InvalidRequest,
            "parallel tool calls need published tools".to_owned()
        )
    );
    parallel.tools.push(tool("read"));
    parallel.validate().unwrap();
    for key in [String::new(), "k".repeat(513)] {
        let mut keyed = turn(None, None);
        keyed.controls.hints.prompt_cache_key = Some(key);
        assert_eq!(
            refused(keyed),
            (
                ErrorCode::InvalidRequest,
                "prompt cache key must be non-empty and at most 512 bytes".to_owned()
            )
        );
    }
    let mut longest = turn(None, None);
    longest.controls.hints.prompt_cache_key = Some("k".repeat(512));
    longest.validate().unwrap();
}

#[test]
fn omitted_hints_are_exactly_the_present_ones_a_wire_does_not_carry_in_advisory_order() {
    let hints = Hints {
        prompt_cache: Some(PromptCache::default()),
        prompt_cache_key: Some("session-1".to_owned()),
        verbosity: None,
        reasoning_summary: Some(ReasoningSummary::Concise),
        anthropic_beta: vec!["context-1m-2025-08-07".to_owned()],
        unforwarded: [Unforwarded::ClientMetadata, Unforwarded::InputItemIds].into(),
    };
    assert_eq!(
        hints.advisories(),
        [
            Advisory::PromptCache,
            Advisory::PromptCacheKey,
            Advisory::ReasoningSummary,
            Advisory::AnthropicBeta,
            Advisory::InputItemIds,
            Advisory::ClientMetadata,
        ]
    );
    assert_eq!(
        hints.omitted_by(&[Advisory::PromptCacheKey, Advisory::Verbosity]),
        [
            Advisory::PromptCache,
            Advisory::ReasoningSummary,
            Advisory::AnthropicBeta,
            Advisory::InputItemIds,
            Advisory::ClientMetadata,
        ]
    );
    assert_eq!(Hints::default().omitted_by(&[]), []);
}

#[test]
fn the_omission_warning_names_each_hint_and_is_absent_when_nothing_is_omitted() {
    assert_eq!(advisory_warning("Chat Completions", &[]), None);
    assert_eq!(
        advisory_warning(
            "Messages",
            &[Advisory::PromptCacheKey, Advisory::ClientMetadata]
        ),
        Some(StreamEvent::Warning {
            code: ADVISORY_OMITTED.to_owned(),
            message: "Messages carries no prompt-cache-key, client-metadata; the advisory hint was not sent".to_owned(),
        })
    );
    assert_eq!(ADVISORY_OMITTED, "advisory-omitted");
}

#[test]
fn a_field_path_copies_only_a_bounded_key_of_the_wires_own_shape() {
    assert_eq!(field_path("", "thinking"), "thinking");
    assert_eq!(
        field_path("messages[].content[]", "cache_control"),
        "messages[].content[].cache_control"
    );
    assert_eq!(field_path("tools[]", "x-y_1"), "tools[].x-y_1");
    let longest = "k".repeat(64);
    assert_eq!(field_path("", &longest), longest);
    for foreign in [
        "",
        "has space",
        "dotted.key",
        "ключ",
        &"k".repeat(65),
        "a\"b",
    ] {
        assert_eq!(field_path("input[]", foreign), "input[].?", "{foreign:?}");
    }
}
