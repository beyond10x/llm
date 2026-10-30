//! Claude Code's first turn, and every field it sends, under the written rules of
//! `docs/design.md` "Client request fields" (Messages column and ingress rule).
//!
//! Each round trip decodes a request, projects it with `encode_request` and decodes the
//! projection back: the two turns are equal except for the model, which the projection sends as
//! the binding's upstream name. The one-way rules are asserted as one-way.
mod support;

use llm_core::{
    Advisory, CacheTtl, Error, ErrorCode, Item, PromptCache, ReasoningSummary, Thinking,
    ToolChoice, TurnDocument, TurnRequest, Unforwarded, Verbosity,
};
use llm_messages::{decode_request, encode_request, omitted_advisories};
use llm_providers::Binding;
use serde_json::{Value, json};
use support::{binding_with, capabilities};

const FIXTURE: &str = include_str!("fixtures/claude-code-first-turn.json");

/// The fixture binding, admitting the effort `max` and the output bound Claude Code sends.
fn binding() -> Binding {
    let mut capabilities = capabilities();
    capabilities.reasoning_efforts.push("max".to_owned());
    capabilities.max_output_tokens = 32_000;
    binding_with("https://messages.example.invalid/v1", capabilities)
}

fn decode(body: &[u8]) -> Result<TurnRequest, Error> {
    decode_request(body, binding().provenance()).map(|ingress| ingress.request)
}

fn decode_json(body: &Value) -> Result<TurnRequest, Error> {
    decode(body.to_string().as_bytes())
}

fn refusal(body: &Value) -> Error {
    match decode_json(body) {
        Ok(request) => panic!("{body} was accepted as {request:?}"),
        Err(error) => error,
    }
}

/// A minimal request the fixture binding can carry, with `extra` set at the top level.
fn body(extra: &[(&str, Value)]) -> Value {
    let mut body = json!({
        "model": "internal-model",
        "max_tokens": 512,
        "system": [{"type":"text","text":"Stay terse","cache_control":{"type":"ephemeral"}}],
        "messages": [{"role":"user","content":[
            {"type":"text","text":"Summarise the log","cache_control":{"type":"ephemeral"}}]}],
        "tools": [{"name":"lookup","description":"Look up a record",
            "input_schema":{"type":"object"}}],
    });
    for (key, value) in extra {
        body[*key] = value.clone();
    }
    body
}

/// Decode, project, decode the projection: returns the first turn, the wire and the second.
fn round_trip(body: &Value) -> (TurnRequest, Value, TurnRequest) {
    let first = decode_json(body).unwrap_or_else(|error| panic!("{body}: {error:?}"));
    let wire: Value = serde_json::from_slice(
        &encode_request(&first, &binding()).unwrap_or_else(|error| panic!("{body}: {error:?}")),
    )
    .expect("the projection is JSON");
    let mut second = decode_json(&wire).unwrap_or_else(|error| panic!("{wire}: {error:?}"));
    assert_eq!(second.model, "example/Model-Revision", "{wire}");
    second.model.clone_from(&first.model);
    (first, wire, second)
}

fn assert_round_trips(body: &Value) -> (TurnRequest, Value) {
    let (first, wire, second) = round_trip(body);
    assert_eq!(first, second, "{wire}");
    (first, wire)
}

#[test]
fn claude_codes_first_turn_decodes_under_the_written_rules() {
    let ingress =
        decode_request(FIXTURE.as_bytes(), binding().provenance()).expect("the first turn");
    assert!(ingress.stream);
    let request = ingress.request;
    assert_eq!(request.model, "code");
    assert_eq!(request.max_output_tokens, Some(32_000));
    assert_eq!(
        request.instructions,
        "[attribution block: client version and conversation fingerprint]\n\
         You are Claude Code, an interactive command line tool."
    );
    assert_eq!(request.controls.thinking, Some(Thinking::Adaptive));
    assert_eq!(request.sampling.reasoning_effort.as_deref(), Some("max"));
    assert_eq!(
        request.controls.hints.prompt_cache,
        Some(PromptCache { ttl: None })
    );
    assert_eq!(request.items, [Item::user("What does /tmp/a.txt say?")]);
    assert_eq!(request.tools.len(), 1);
    assert_eq!(request.tools[0].name.as_str(), "Read");
    assert_eq!(request.controls.parallel_tool_calls, None);
    assert!(request.controls.hints.unforwarded.is_empty());
    assert_eq!(omitted_advisories(&request), []);
}

#[test]
fn thinking_round_trips_in_each_of_its_three_forms() {
    for (thinking, neutral) in [
        (json!({"type":"adaptive"}), Thinking::Adaptive),
        (
            json!({"type":"enabled","budget_tokens":1024}),
            Thinking::Enabled {
                budget_tokens: 1024,
            },
        ),
        (json!({"type":"disabled"}), Thinking::Disabled),
    ] {
        let (first, wire) = assert_round_trips(&body(&[("thinking", thinking.clone())]));
        assert_eq!(first.controls.thinking, Some(neutral), "{thinking}");
        assert_eq!(wire["thinking"], thinking, "{wire}");
    }
}

#[test]
fn disable_parallel_tool_use_round_trips_true_and_false() {
    for disable in [true, false] {
        let choice = json!({"type":"auto","disable_parallel_tool_use":disable});
        let (first, wire) = assert_round_trips(&body(&[("tool_choice", choice.clone())]));
        assert_eq!(first.controls.parallel_tool_calls, Some(!disable));
        assert_eq!(first.tool_choice, ToolChoice::Auto);
        assert_eq!(wire["tool_choice"], choice, "{wire}");
    }
    for choice in [
        json!({"type":"any","disable_parallel_tool_use":true}),
        json!({"type":"tool","name":"lookup","disable_parallel_tool_use":false}),
    ] {
        let (_, wire) = assert_round_trips(&body(&[("tool_choice", choice.clone())]));
        assert_eq!(wire["tool_choice"], choice, "{wire}");
    }
}

#[test]
fn a_one_hour_prompt_cache_lifetime_round_trips_onto_every_breakpoint() {
    let one_hour = json!({"type":"ephemeral","ttl":"1h"});
    let request = json!({
        "model": "internal-model",
        "max_tokens": 512,
        "system": [{"type":"text","text":"Stay terse","cache_control":one_hour}],
        "messages": [{"role":"user","content":[{"type":"text","text":"Summarise the log"}]}],
        "tools": [{"name":"lookup","description":"Look up a record",
            "input_schema":{"type":"object"},"cache_control":one_hour}],
    });
    let (first, wire) = assert_round_trips(&request);
    assert_eq!(
        first.controls.hints.prompt_cache,
        Some(PromptCache {
            ttl: Some(CacheTtl::OneHour)
        })
    );
    // The projection places its own breakpoints, the system block and the rolling tail, and
    // every one of them takes the lifetime; it does not reproduce the caller's positions.
    assert_eq!(wire["system"][0]["cache_control"], one_hour, "{wire}");
    assert_eq!(
        wire["messages"][0]["content"][0]["cache_control"], one_hour,
        "{wire}"
    );
    assert!(wire["tools"][0].get("cache_control").is_none(), "{wire}");
}

#[test]
fn a_five_minute_mark_is_the_default_lifetime_and_reads_as_none() {
    let five = json!({"type":"ephemeral","ttl":"5m"});
    let request = json!({
        "model": "internal-model",
        "max_tokens": 512,
        "system": [{"type":"text","text":"Stay terse","cache_control":five}],
        "messages": [{"role":"user","content":[
            {"type":"text","text":"Summarise the log","cache_control":{"type":"ephemeral"}}]}],
    });
    let (first, wire) = assert_round_trips(&request);
    assert_eq!(
        first.controls.hints.prompt_cache,
        Some(PromptCache { ttl: None })
    );
    assert_eq!(
        wire["system"][0]["cache_control"],
        json!({"type":"ephemeral"}),
        "{wire}"
    );
}

#[test]
fn effort_max_round_trips_as_its_own_name() {
    let (first, wire) = assert_round_trips(&body(&[("output_config", json!({"effort":"max"}))]));
    assert_eq!(first.sampling.reasoning_effort.as_deref(), Some("max"));
    assert_eq!(wire["output_config"], json!({"effort":"max"}), "{wire}");
}

#[test]
fn claude_codes_first_turn_round_trips_except_the_joined_system() {
    let mut first = decode(FIXTURE.as_bytes()).expect("the first turn");
    first.model = "internal-model".to_owned();
    let wire: Value =
        serde_json::from_slice(&encode_request(&first, &binding()).expect("projected"))
            .expect("JSON");
    let system = wire["system"].as_array().expect("a system block list");
    assert_eq!(system.len(), 1, "the joined system comes back as one block");
    assert_eq!(system[0]["text"], json!(first.instructions));
    let mut second = decode_json(&wire).expect("the projection decodes");
    second.model.clone_from(&first.model);
    assert_eq!(first, second, "{wire}");
}

#[test]
fn a_system_of_several_text_blocks_joins_with_a_line_feed_and_comes_back_as_one_block() {
    let request = body(&[(
        "system",
        json!([{"type":"text","text":"Stay terse"},
            {"type":"text","text":"Cite nothing","cache_control":{"type":"ephemeral"}}]),
    )]);
    let (first, wire, second) = round_trip(&request);
    assert_eq!(first.instructions, "Stay terse\nCite nothing");
    assert_eq!(
        wire["system"],
        json!([{"type":"text","text":"Stay terse\nCite nothing",
            "cache_control":{"type":"ephemeral"}}])
    );
    assert_eq!(first, second);
}

#[test]
fn an_empty_system_array_is_empty_instructions() {
    let request = decode_json(&body(&[("system", json!([]))])).expect("accepted");
    assert_eq!(request.instructions, "");
}

#[test]
fn metadata_is_client_metadata_never_kept_never_re_emitted_and_reported_omitted() {
    let secret = "user-7f3a-telemetry-value";
    let request = body(&[("metadata", json!({"user_id":secret}))]);
    let (first, wire, second) = round_trip(&request);
    assert_eq!(
        first.controls.hints.unforwarded,
        [Unforwarded::ClientMetadata].into_iter().collect()
    );
    assert_eq!(omitted_advisories(&first), [Advisory::ClientMetadata]);
    let document = serde_json::to_string(&TurnDocument::new(first.clone())).expect("a document");
    assert!(!document.contains(secret), "{document}");
    assert!(wire.get("metadata").is_none(), "{wire}");
    assert!(!wire.to_string().contains(secret), "{wire}");
    assert!(second.controls.hints.unforwarded.is_empty());
    assert_eq!(omitted_advisories(&second), []);
}

#[test]
fn every_refusal_names_its_field_by_path() {
    let thinking_block = json!({"type":"thinking","thinking":"weighing","signature":"sig-1",
        "cache_control":{"type":"ephemeral"}});
    let cases = [
        (
            body(&[(
                "thinking",
                json!({"type":"adaptive","display":"summarized"}),
            )]),
            "thinking.display",
        ),
        (
            body(&[("thinking", json!({"type":"adaptive","budget_tokens":1024}))]),
            "thinking.budget_tokens",
        ),
        (
            body(&[("thinking", json!({"type":"disabled","budget_tokens":1024}))]),
            "thinking.budget_tokens",
        ),
        (
            body(&[(
                "context_management",
                json!({"edits":[{"type":"clear_tool_uses_20250919"}]}),
            )]),
            "context_management",
        ),
        (body(&[("safeguards", json!({}))]), "safeguards"),
        (
            body(&[(
                "output_config",
                json!({"effort":"high","format":{"type":"json_schema","schema":{}}}),
            )]),
            "output_config.format",
        ),
        (
            body(&[(
                "system",
                json!([{"type":"text","text":"Stay terse","citations":{"enabled":true}}]),
            )]),
            "system[].citations",
        ),
        (
            body(&[(
                "messages",
                json!([{"role":"user","content":[{"type":"text","text":"hi",
                    "cache_control":{"type":"ephemeral","scope":"global"}}]}]),
            )]),
            "messages[].content[].cache_control.scope",
        ),
        (
            body(&[(
                "messages",
                json!([{"role":"user","content":"hi"},
                    {"role":"assistant","content":[thinking_block]},
                    {"role":"user","content":"go on"}]),
            )]),
            "messages[].content[].cache_control",
        ),
        (
            body(&[(
                "tools",
                json!([{"name":"lookup","input_schema":{"type":"object"},
                    "cache_control":{"type":"ephemeral","scope":"global"}}]),
            )]),
            "tools[].cache_control.scope",
        ),
        (
            body(&[("metadata", json!({"user_id":"u","session":"s"}))]),
            "metadata.session",
        ),
    ];
    for (request, path) in cases {
        let error = refusal(&request);
        assert_eq!(error.code, ErrorCode::Unsupported, "{request}");
        assert_eq!(
            error.message,
            format!("Messages field is outside the declared subset: {path}"),
            "{request}"
        );
    }
}

#[test]
fn two_cache_lifetimes_in_one_request_are_refused() {
    let request = body(&[(
        "system",
        json!([{"type":"text","text":"Stay terse",
            "cache_control":{"type":"ephemeral","ttl":"1h"}}]),
    )]);
    let error = refusal(&request);
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        error.message,
        "Messages cache marks carry more than one lifetime"
    );
}

#[test]
fn a_cache_mark_of_another_shape_is_refused_with_a_fixed_diagnostic() {
    for (mark, message) in [
        (
            json!({"type":"persistent"}),
            "Messages cache mark type is outside the declared subset",
        ),
        (
            json!({"type":"ephemeral","ttl":"10m"}),
            "Messages cache mark lifetime is outside the declared subset",
        ),
        (
            json!("ephemeral"),
            "Messages cache mark is outside the declared subset",
        ),
    ] {
        let request = body(&[(
            "system",
            json!([{"type":"text","text":"Stay terse","cache_control":mark}]),
        )]);
        let error = refusal(&request);
        assert_eq!(error.code, ErrorCode::Unsupported, "{request}");
        assert_eq!(error.message, message, "{request}");
    }
}

/// A `system` message inside `messages` is read since story:messages-ingress-system-messages
/// (`tests/system_messages.rs`); every other role outside user and assistant is still refused.
#[test]
fn a_role_outside_user_assistant_and_system_is_outside_the_subset() {
    let request = body(&[(
        "messages",
        json!([{"role":"user","content":"hi"},{"role":"tool","content":"ok"}]),
    )]);
    let error = refusal(&request);
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        error.message, "Messages role is outside the declared subset",
        "story:messages-ingress-system-messages: role system is read, role tool is not"
    );
}

#[test]
fn a_system_block_of_another_type_is_not_text() {
    let request = body(&[(
        "system",
        json!([{"type":"image","source":{"type":"base64","media_type":"image/png","data":"AA=="}}]),
    )]);
    let error = refusal(&request);
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(error.message, "Messages system block is not text");
}

#[test]
fn thinking_outside_its_three_forms_is_refused() {
    for thinking in [
        json!({"type":"enabled"}),
        json!({"type":"enabled","budget_tokens":0}),
        json!({"type":"enabled","budget_tokens":"1024"}),
        json!({"type":"auto"}),
    ] {
        let request = body(&[("thinking", thinking.clone())]);
        assert!(decode_json(&request).is_err(), "{thinking} was accepted");
    }
}

#[test]
fn disable_parallel_tool_use_must_be_boolean() {
    let request = body(&[(
        "tool_choice",
        json!({"type":"auto","disable_parallel_tool_use":"yes"}),
    )]);
    assert_eq!(refusal(&request).code, ErrorCode::InvalidRequest);
}

#[test]
fn egress_writes_an_automatic_choice_to_carry_the_parallel_control() {
    let mut request = decode_json(&body(&[])).expect("accepted");
    request.controls.parallel_tool_calls = Some(true);
    let wire: Value =
        serde_json::from_slice(&encode_request(&request, &binding()).unwrap()).expect("JSON");
    assert_eq!(
        wire["tool_choice"],
        json!({"type":"auto","disable_parallel_tool_use":false})
    );
}

#[test]
fn omitted_advisories_names_every_hint_this_wire_does_not_send_in_table_order() {
    let mut request = decode_json(&body(&[])).expect("accepted");
    assert_eq!(omitted_advisories(&request), []);
    let hints = &mut request.controls.hints;
    hints.prompt_cache = Some(PromptCache {
        ttl: Some(CacheTtl::OneHour),
    });
    assert_eq!(omitted_advisories(&request), []);
    let hints = &mut request.controls.hints;
    hints.prompt_cache_key = Some("conversation-1".to_owned());
    hints.verbosity = Some(Verbosity::Low);
    hints.reasoning_summary = Some(ReasoningSummary::Auto);
    hints.unforwarded = [Unforwarded::ClientMetadata, Unforwarded::InputItemIds]
        .into_iter()
        .collect();
    assert_eq!(
        omitted_advisories(&request),
        [
            Advisory::PromptCacheKey,
            Advisory::Verbosity,
            Advisory::ReasoningSummary,
            Advisory::InputItemIds,
            Advisory::ClientMetadata,
        ]
    );
}
