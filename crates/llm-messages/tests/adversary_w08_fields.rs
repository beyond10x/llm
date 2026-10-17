//! Adversary cases for the Messages client request fields (wave 2026-10-16-w08).
//!
//! Each case drives the public decoder and projection against the rules of `docs/design.md`
//! "Client request fields" and the acceptance of `story:messages-ingress-client-fields`.
mod support;

use llm_core::{CacheTtl, Error, ErrorCode, Item, PromptCache, TurnRequest};
use llm_messages::{decode_request, encode_request};
use llm_providers::Binding;
use serde_json::{Value, json};
use support::{binding_with, capabilities};

/// The fixture binding, admitting the effort `max`.
fn binding() -> Binding {
    let mut capabilities = capabilities();
    capabilities.reasoning_efforts.push("max".to_owned());
    binding_with("https://messages.example.invalid/v1", capabilities)
}

fn decode(body: &Value) -> Result<TurnRequest, Error> {
    decode_request(body.to_string().as_bytes(), binding().provenance())
        .map(|ingress| ingress.request)
}

/// A request as a client that places no cache marks sends it, with `extra` set at the top level.
fn unmarked(extra: &[(&str, Value)]) -> Value {
    let mut body = json!({
        "model": "internal-model",
        "max_tokens": 512,
        "system": "Stay terse",
        "messages": [{"role":"user","content":"Summarise the log"}],
        "tools": [{"name":"lookup","description":"Look up a record",
            "input_schema":{"type":"object"}}],
    });
    for (key, value) in extra {
        body[*key] = value.clone();
    }
    body
}

/// The acceptance: "Round trips, each decoded, re-projected with `encode_request` and decoded back
/// to an equal turn: `thinking` in each of its three forms, `disable_parallel_tool_use` true and
/// false, ..., effort `max`." The statement does not require the arriving body to carry a cache
/// mark, and most Messages clients send none. The projection places its own breakpoints on every
/// turn, so the turn read back carries a `prompt-cache` hint the first turn never had.
#[test]
fn a_request_without_cache_marks_round_trips_to_an_equal_turn() {
    let cases = [
        ("thinking", json!({"type":"adaptive"})),
        ("thinking", json!({"type":"enabled","budget_tokens":256})),
        ("thinking", json!({"type":"disabled"})),
        (
            "tool_choice",
            json!({"type":"auto","disable_parallel_tool_use":true}),
        ),
        (
            "tool_choice",
            json!({"type":"auto","disable_parallel_tool_use":false}),
        ),
        ("output_config", json!({"effort":"max"})),
    ];
    for (key, value) in cases {
        let body = unmarked(&[(key, value)]);
        let first = decode(&body).unwrap_or_else(|error| panic!("{body}: {error:?}"));
        assert_eq!(first.controls.hints.prompt_cache, None, "{body}");
        let wire: Value = serde_json::from_slice(
            &encode_request(&first, &binding()).unwrap_or_else(|error| panic!("{body}: {error:?}")),
        )
        .expect("the projection is JSON");
        let mut second = decode(&wire).unwrap_or_else(|error| panic!("{wire}: {error:?}"));
        second.model.clone_from(&first.model);
        // Decided one-way rule (docs/messages.md): the breakpoints the projection always places
        // read back as the prompt-cache hint, so an unmarked request returns with it set.
        let mut expected = first.clone();
        expected.controls.hints.prompt_cache = Some(PromptCache::default());
        assert_eq!(expected, second, "{body} came back from {wire}");
    }
}

/// Design: `cache_control` is accepted on "a `text`, `tool_use` or `tool_result` block". No case
/// in the unit's suite or scenarios puts a mark on a `tool_use` block, so dropping `tool_use` from
/// the ingress markable set (the reader would then refuse the mark by its path) stays green.
#[test]
fn a_cache_mark_on_an_assistant_tool_use_block_is_the_prompt_cache_hint() {
    let body = json!({
        "model": "internal-model",
        "max_tokens": 512,
        "messages": [
            {"role":"user","content":"Summarise the log"},
            {"role":"assistant","content":[{"type":"tool_use","id":"call-1","name":"lookup",
                "input":{"query":"errors"},
                "cache_control":{"type":"ephemeral","ttl":"1h"}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"call-1",
                "content":"not found"}]},
        ],
        "tools": [{"name":"lookup","description":"Look up a record",
            "input_schema":{"type":"object"}}],
    });
    let request = decode(&body).unwrap_or_else(|error| panic!("{body}: {error:?}"));
    assert_eq!(
        request.controls.hints.prompt_cache,
        Some(PromptCache {
            ttl: Some(CacheTtl::OneHour)
        })
    );
    assert!(
        matches!(request.items.get(1), Some(Item::ToolCall(call)) if call.call_id.as_str() == "call-1"),
        "{:?}",
        request.items
    );
}

/// Design: a mark "on ... a tool" is the hint. The unit's one-hour case marks a tool and the
/// system block alike, so a tool mark that never reached the hint would still pass it.
#[test]
fn a_one_hour_mark_on_a_tool_alone_is_the_prompt_cache_hint() {
    let body = json!({
        "model": "internal-model",
        "max_tokens": 512,
        "messages": [{"role":"user","content":"Summarise the log"}],
        "tools": [{"name":"lookup","description":"Look up a record",
            "input_schema":{"type":"object"},
            "cache_control":{"type":"ephemeral","ttl":"1h"}}],
    });
    let request = decode(&body).unwrap_or_else(|error| panic!("{body}: {error:?}"));
    assert_eq!(
        request.controls.hints.prompt_cache,
        Some(PromptCache {
            ttl: Some(CacheTtl::OneHour)
        })
    );
}

/// Design: "two different lifetimes in one request are refused", wherever the marks sit. The
/// unit's case puts both on the system and a content block; a tool mark read into a lifetime
/// apart from the others would pass it.
#[test]
fn a_one_hour_tool_mark_beside_a_default_system_mark_is_two_lifetimes() {
    let body = json!({
        "model": "internal-model",
        "max_tokens": 512,
        "system": [{"type":"text","text":"Stay terse","cache_control":{"type":"ephemeral"}}],
        "messages": [{"role":"user","content":"Summarise the log"}],
        "tools": [{"name":"lookup","description":"Look up a record",
            "input_schema":{"type":"object"},
            "cache_control":{"type":"ephemeral","ttl":"1h"}}],
    });
    let error = match decode(&body) {
        Ok(request) => panic!("{body} was accepted as {request:?}"),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::Unsupported, "{body}");
    assert_eq!(
        error.message, "Messages cache marks carry more than one lifetime",
        "{body}"
    );
}
