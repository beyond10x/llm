//! Adversarial pass over `story:chat-ingress-closed-levels`, driven from the contract the unit
//! wrote about itself: `docs/chat.md` "Gateway ingress", the paragraph under the closed-level
//! table. No production source is changed by this file.
use llm_chat::decode_ingress_request;
use llm_core::ErrorCode;
use serde_json::{Value, json};

/// A request reaching every level that carries an optional key: a content part, an assistant
/// message, a tool call and its function, a published tool and its function.
fn base() -> Value {
    json!({
        "model": "code",
        "messages": [
            {"role": "user", "content": [{"type": "text", "text": "Weather?"}]},
            {"role": "assistant", "content": "Looking.", "tool_calls": [
                {"id": "call-1", "type": "function",
                 "function": {"name": "lookup", "arguments": "{}"}}
            ]},
            {"role": "tool", "tool_call_id": "call-1", "content": "sunny"},
            {"role": "assistant", "content": "Sunny.", "tool_calls": []}
        ],
        "tools": [{"type": "function", "function": {
            "name": "lookup", "description": "Look up", "parameters": {"type": "object"},
            "strict": false
        }}]
    })
}

/// `base` with `key` under the object at `parent` set to `null`, and with it removed.
fn null_and_absent(base: &Value, parent: &str, key: &str) -> (Value, Value) {
    let mut nulled = base.clone();
    nulled
        .pointer_mut(parent)
        .and_then(Value::as_object_mut)
        .expect("fixture parent")
        .insert(key.to_owned(), Value::Null);
    let mut absent = base.clone();
    absent
        .pointer_mut(parent)
        .and_then(Value::as_object_mut)
        .expect("fixture parent")
        .remove(key);
    (nulled, absent)
}

/// `docs/chat.md`: "an explicit `null` for an optional key is absent". Every optional key the
/// decoder reads, at every level, is set to `null` and compared with the same request without
/// it. `tools` is the documented exception (an array when present), so its row is left out.
#[test]
fn an_explicit_null_for_every_optional_key_is_read_as_absent() {
    let mut with_choice = base();
    with_choice["tool_choice"] = json!({"type": "function", "function": {"name": "lookup"}});

    let rows: [(&str, Value, &str, &str); 19] = [
        ("tool_choice", base(), "", "tool_choice"),
        ("max_completion_tokens", base(), "", "max_completion_tokens"),
        ("max_tokens", base(), "", "max_tokens"),
        ("temperature", base(), "", "temperature"),
        ("top_p", base(), "", "top_p"),
        ("reasoning_effort", base(), "", "reasoning_effort"),
        ("stream", base(), "", "stream"),
        ("stream_options", base(), "", "stream_options"),
        ("parallel_tool_calls", base(), "", "parallel_tool_calls"),
        ("prompt_cache_key", base(), "", "prompt_cache_key"),
        ("verbosity", base(), "", "verbosity"),
        (
            "messages[].content[].text",
            base(),
            "/messages/0/content/0",
            "text",
        ),
        ("messages[].content", base(), "/messages/3", "content"),
        ("messages[].tool_calls", base(), "/messages/3", "tool_calls"),
        (
            "messages[].tool_calls[].type",
            base(),
            "/messages/1/tool_calls/0",
            "type",
        ),
        (
            "messages[].tool_calls[].function.arguments",
            base(),
            "/messages/1/tool_calls/0/function",
            "arguments",
        ),
        (
            "tools[].function.description",
            base(),
            "/tools/0/function",
            "description",
        ),
        (
            "tools[].function.strict",
            base(),
            "/tools/0/function",
            "strict",
        ),
        ("tool_choice.type", with_choice, "/tool_choice", "type"),
    ];

    let mut failures = Vec::new();
    for (label, body, parent, key) in rows {
        let (nulled, absent) = null_and_absent(&body, parent, key);
        let absent = decode_ingress_request(&absent)
            .unwrap_or_else(|error| panic!("{label}: the fixture without it: {}", error.message));
        match decode_ingress_request(&nulled) {
            Ok(nulled) => {
                if nulled.request != absent.request
                    || nulled.stream != absent.stream
                    || nulled.include_usage != absent.include_usage
                {
                    failures.push(format!("{label}: decoded to another turn"));
                }
            }
            Err(error) => failures.push(format!(
                "{label}: refused ({:?}: {})",
                error.code, error.message
            )),
        }
    }
    assert!(
        failures.is_empty(),
        "explicit null was not read as absent: {failures:?}"
    );
}

/// `docs/chat.md`: "A key the subset reads with a value of the wrong type is refused rather than
/// read as absent" (`unsupported` for a `tool_choice` or a `type` outside the subset). A tool choice that is neither a string nor an object, and a
/// `type` that is not a string, are values of the wrong type at keys the subset reads.
#[test]
fn a_read_key_of_the_wrong_type_is_refused_as_invalid() {
    let mut choice_number = base();
    choice_number["tool_choice"] = json!(5);
    let mut choice_bool = base();
    choice_bool["tool_choice"] = json!(true);
    let mut call_type = base();
    call_type["messages"][1]["tool_calls"][0]["type"] = json!(5);
    let mut tool_type = base();
    tool_type["tools"][0]["type"] = json!(5);
    let mut part_type = base();
    part_type["messages"][0]["content"][0]["type"] = json!(5);

    let rows = [
        ("tool_choice: 5", choice_number),
        ("tool_choice: true", choice_bool),
        ("messages[].tool_calls[].type: 5", call_type),
        ("tools[].type: 5", tool_type),
        ("messages[].content[].type: 5", part_type),
    ];
    let mut failures = Vec::new();
    for (label, body) in rows {
        match decode_ingress_request(&body) {
            Ok(_) => failures.push(format!("{label}: accepted")),
            Err(error)
                if !matches!(
                    error.code,
                    ErrorCode::InvalidRequest | ErrorCode::Unsupported
                ) =>
            {
                failures.push(format!(
                    "{label}: refused as {:?} ({})",
                    error.code, error.message
                ));
            }
            Err(_) => {}
        }
    }
    assert!(
        failures.is_empty(),
        "a wrong-typed value was not refused as invalid: {failures:?}"
    );
}
