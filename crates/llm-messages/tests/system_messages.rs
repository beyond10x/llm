//! A `system` entry inside `messages` (story:messages-ingress-system-messages, decision D262).
//!
//! A headless Claude Code client sends one. Leading entries join the instructions in order; a
//! later entry stays at its place as one user text item wrapped in the fixed mark
//! [`SYSTEM_MESSAGE_OPEN`] … [`SYSTEM_MESSAGE_CLOSE`]; the projection merges adjacent user items
//! into one user message, as it does for any two user items.
mod support;

use llm_core::{CallId, Error, ErrorCode, Item, PromptCache, ToolCall, ToolName, TurnRequest};
use llm_messages::{SYSTEM_MESSAGE_CLOSE, SYSTEM_MESSAGE_OPEN, decode_request, encode_request};
use llm_providers::Binding;
use serde_json::{Value, json};
use support::{binding_with, capabilities};

fn binding() -> Binding {
    binding_with("https://messages.example.invalid/v1", capabilities())
}

fn decode(body: &Value) -> Result<TurnRequest, Error> {
    decode_request(body.to_string().as_bytes(), binding().provenance())
        .map(|ingress| ingress.request)
}

fn refusal(body: &Value) -> Error {
    match decode(body) {
        Ok(request) => panic!("{body} was accepted as {request:?}"),
        Err(error) => error,
    }
}

/// A request whose `messages` are `messages`, with a marked top-level `system`, so the
/// projection's breakpoints read back as the hint the request already carries.
fn request(messages: &Value) -> Value {
    json!({
        "model": "internal-model",
        "max_tokens": 512,
        "system": [{"type":"text","text":"Stay terse","cache_control":{"type":"ephemeral"}}],
        "messages": messages,
    })
}

fn marked(text: &str) -> String {
    format!("{SYSTEM_MESSAGE_OPEN}{text}{SYSTEM_MESSAGE_CLOSE}")
}

#[test]
fn the_mark_is_one_fixed_documented_form() {
    assert_eq!(
        marked("Be brief"),
        "<system-message>\nBe brief\n</system-message>"
    );
}

/// The exact shape the item names: one `system` entry at the start, one in the middle.
#[test]
fn a_leading_system_entry_joins_the_instructions_and_a_later_one_stays_in_place_marked() {
    let body = request(&json!([
        {"role":"system","content":"Answer in English"},
        {"role":"user","content":"Summarise the log"},
        {"role":"assistant","content":"Looking"},
        {"role":"system","content":"Be brief"},
        {"role":"user","content":[{"type":"text","text":"Go on"}]},
    ]));
    let first = decode(&body).unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(first.instructions, "Stay terse\nAnswer in English");
    assert_eq!(
        first.items,
        [
            Item::user("Summarise the log"),
            Item::assistant("Looking"),
            Item::user(marked("Be brief")),
            Item::user("Go on"),
        ]
    );

    let wire: Value = serde_json::from_slice(
        &encode_request(&first, &binding()).unwrap_or_else(|error| panic!("{error:?}")),
    )
    .expect("the projection is JSON");
    let roles: Vec<&str> = wire["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .map(|message| message["role"].as_str().expect("a role"))
        .collect();
    assert_eq!(roles, ["user", "assistant", "user"], "{wire}");
    assert_eq!(
        wire["messages"][2]["content"][0]["text"],
        marked("Be brief")
    );
    assert_eq!(wire["messages"][2]["content"][1]["text"], "Go on");
    assert_eq!(wire["system"][0]["text"], "Stay terse\nAnswer in English");

    let mut second = decode(&wire).unwrap_or_else(|error| panic!("{wire}: {error:?}"));
    second.model.clone_from(&first.model);
    assert_eq!(first, second, "{wire}");
}

/// The user message that answers a tool call must open with its results, so a `system` entry
/// between the call and the results is placed right after the results, not before them.
#[test]
fn a_system_entry_after_a_tool_call_follows_the_results_that_answer_it() {
    let body = request(&json!([
        {"role":"user","content":"Run the tests"},
        {"role":"assistant","content":[
            {"type":"tool_use","id":"call-1","name":"lookup","input":{"q":"x"}}]},
        {"role":"system","content":"Be brief"},
        {"role":"system","content":"Cite nothing"},
        {"role":"user","content":[
            {"type":"tool_result","tool_use_id":"call-1","content":"ok"},
            {"type":"text","text":"Go on"}]},
    ]));
    let request = decode(&body).unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(
        request.items,
        [
            Item::user("Run the tests"),
            Item::ToolCall(ToolCall {
                call_id: CallId::new("call-1").unwrap(),
                name: ToolName::new("lookup").unwrap(),
                arguments: json!({"q":"x"}),
            }),
            Item::ToolResult {
                call_id: CallId::new("call-1").unwrap(),
                output: json!("ok"),
                failed: false,
            },
            Item::user(marked("Be brief")),
            Item::user(marked("Cite nothing")),
            Item::user("Go on"),
        ]
    );
}

#[test]
fn leading_system_entries_are_appended_in_order_and_need_no_top_level_system() {
    let body = json!({
        "model": "internal-model",
        "max_tokens": 512,
        "messages": [
            {"role":"system","content":"First"},
            {"role":"system","content":[{"type":"text","text":"Second"},{"type":"text","text":"Third"}]},
            {"role":"user","content":"hi"},
        ],
    });
    let request = decode(&body).unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(request.instructions, "First\nSecond\nThird");
    assert_eq!(request.items, [Item::user("hi")]);
}

#[test]
fn a_cache_mark_on_a_system_entry_block_is_the_prompt_cache_hint() {
    let body = json!({
        "model": "internal-model",
        "max_tokens": 512,
        "messages": [
            {"role":"user","content":"hi"},
            {"role":"system","content":[{"type":"text","text":"Be brief",
                "cache_control":{"type":"ephemeral","ttl":"1h"}}]},
        ],
    });
    let request = decode(&body).unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(
        request.items,
        [Item::user("hi"), Item::user(marked("Be brief"))]
    );
    assert_eq!(
        request.controls.hints.prompt_cache,
        Some(PromptCache {
            ttl: Some(llm_core::CacheTtl::OneHour)
        })
    );
}

#[test]
fn an_empty_system_entry_adds_nothing() {
    let body = request(&json!([
        {"role":"system","content":""},
        {"role":"user","content":"hi"},
        {"role":"system","content":[]},
    ]));
    let request = decode(&body).unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(request.instructions, "Stay terse");
    assert_eq!(request.items, [Item::user("hi")]);
}

#[test]
fn a_system_entry_block_outside_text_is_refused_by_its_path() {
    for (block, path) in [
        (
            json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":"AA=="}}),
            "messages[].content[].type",
        ),
        (
            json!({"type":"tool_result","tool_use_id":"call-1","content":"ok"}),
            "messages[].content[].type",
        ),
        (
            json!({"type":"text","text":"Be brief","citations":[{"type":"char_location"}]}),
            "messages[].content[].citations",
        ),
    ] {
        let body = request(&json!([
            {"role":"user","content":"hi"},
            {"role":"system","content":[block]},
        ]));
        let error = refusal(&body);
        assert_eq!(error.code, ErrorCode::Unsupported, "{body}");
        assert_eq!(
            error.message,
            format!("Messages field is outside the declared subset: {path}"),
            "{body}"
        );
    }
}

#[test]
fn a_system_entry_of_another_content_shape_or_key_is_refused() {
    let shape = refusal(&request(&json!([
        {"role":"user","content":"hi"},
        {"role":"system","content":{"text":"Be brief"}},
    ])));
    assert_eq!(shape.message, "Messages content must be text or blocks");
    let key = refusal(&request(&json!([
        {"role":"system","content":"Be brief","name":"ops"},
        {"role":"user","content":"hi"},
    ])));
    assert_eq!(
        key.message,
        "Messages field is outside the declared subset: messages[].name"
    );
}

#[test]
fn system_entries_alone_are_still_without_initial_user_content() {
    let error = refusal(&request(&json!([{"role":"system","content":"Be brief"}])));
    assert_eq!(error.message, "Messages requires initial user content");
}

#[test]
fn a_role_outside_user_assistant_and_system_is_still_refused() {
    let error = refusal(&request(&json!([
        {"role":"user","content":"hi"},
        {"role":"tool","content":"ok"},
    ])));
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        error.message,
        "Messages role is outside the declared subset"
    );
}
