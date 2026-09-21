//! Adversarial pass 2: the refusals the contract names by name, and nothing exercises.
//!
//! `docs/messages.md:38` lists what the projection refuses "not dropped", and closes with the
//! sentence the whole crate rests on: "A translation that silently loses a field is a translation
//! that appeared to succeed." Three entries on that list, and three more promises elsewhere in the
//! same document, had no case and no scenario: deleting each guard left all 52 Rust tests and all
//! 39 conformance scenarios green. Every case below was written from the document, and each one
//! kills exactly one of those mutations.
//!
//! The mutations, measured on a copy of this tree outside the worktree:
//!   decode.rs `Header::reason`   bounded-name check -> `if false`
//!   codec.rs  `decode_messages`  role check         -> `if false`
//!   codec.rs  `instructions`     `blocks.len() == 1` -> `!blocks.is_empty()`
//!   codec.rs  `decode_tool_choice` unknown arm      -> `ToolChoice::Auto`
//!   decode.rs `append`           `ok_or_else(...)?` -> `unwrap_or("")`
//!   decode.rs `stop_block`       `map_err(...)?`    -> `unwrap_or_else(|_| json!({}))`
//!   usage.rs  `count`            `ok_or_else(...)`  -> `Ok(n.as_u64())`
mod support;

use llm_core::{Cancel, ErrorCode, Item, ToolChoice, ToolName, ToolSpec, TurnRequest, VecSink};
use llm_messages::{decode_request, decode_stream};
use serde_json::{Value, json};
use support::binding;

fn ingress(body: &Value) -> Result<llm_messages::IngressRequest, llm_core::Error> {
    decode_request(body.to_string().as_bytes(), binding().provenance())
}

fn minimal(extra: &[(&str, Value)]) -> Value {
    let mut body = json!({
        "model": "caller-alias",
        "max_tokens": 512,
        "messages": [{"role":"user","content":"Summarise the log"}],
    });
    for (key, value) in extra {
        body[*key] = value.clone();
    }
    body
}

/// `docs/messages.md:40`: "a role outside user/assistant" is refused, not dropped.
#[test]
fn ingress_refuses_a_role_outside_user_and_assistant() {
    let body = minimal(&[(
        "messages",
        json!([
            {"role":"user","content":"Summarise the log"},
            {"role":"system","content":"Stay terse"},
        ]),
    )]);
    let error = ingress(&body).expect_err("a role outside the declared subset");
    assert_eq!(error.code, ErrorCode::Unsupported);
}

/// `docs/messages.md:40`: "a system prompt that is not one text block" is refused. Two blocks are
/// not one, and keeping the first of them is the silent loss the document names.
#[test]
fn ingress_refuses_a_system_prompt_of_more_than_one_block() {
    let body = minimal(&[(
        "system",
        json!([{"type":"text","text":"Stay terse"}, {"type":"text","text":"Cite nothing"}]),
    )]);
    let error = ingress(&body).expect_err("a system prompt that is not one text block");
    assert_eq!(error.code, ErrorCode::Unsupported);
}

/// `docs/messages.md:40`: "a `none` tool choice" is refused. Reading it as `auto` would let a
/// caller that forbade tool use receive tool calls.
#[test]
fn ingress_refuses_a_tool_choice_outside_the_declared_subset() {
    for choice in [json!({"type":"none"}), json!({"type":"auto_or_any"})] {
        let body = minimal(&[
            ("tool_choice", choice.clone()),
            (
                "tools",
                json!([{"name":"lookup","description":"Look up a record",
                    "input_schema":{"type":"object"}}]),
            ),
        ]);
        let decoded = ingress(&body);
        let error = decoded
            .as_ref()
            .err()
            .unwrap_or_else(|| panic!("{choice} was accepted as {:?}", tool_choice(&decoded)));
        assert_eq!(error.code, ErrorCode::Unsupported, "{choice}");
    }
}

fn tool_choice(decoded: &Result<llm_messages::IngressRequest, llm_core::Error>) -> Option<String> {
    decoded
        .as_ref()
        .ok()
        .map(|ingress| format!("{:?}", ingress.request.tool_choice))
}

fn request() -> TurnRequest {
    let mut request = TurnRequest::new("internal-model", vec![Item::user("Summarise the log")]);
    request.tools = vec![ToolSpec {
        name: ToolName::new("lookup").unwrap(),
        description: "Look up a record".to_owned(),
        input_schema: json!({"type":"object"}),
    }];
    request.tool_choice = ToolChoice::Auto;
    request
}

fn sse(events: &[Value]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for event in events {
        let name = event["type"].as_str().expect("event type");
        bytes.extend_from_slice(format!("event: {name}\ndata: {event}\n\n").as_bytes());
    }
    bytes
}

fn message_start(usage: Value) -> Value {
    let mut message = json!({"id":"msg_014a","type":"message","role":"assistant",
        "model":"example-model-20260201","content":[],
        "stop_reason":null,"stop_sequence":null});
    message["usage"] = usage;
    json!({"type":"message_start","message":message})
}

fn message_delta(stop_reason: Value, usage: Value) -> Value {
    let mut event = json!({"type":"message_delta","delta":{"stop_sequence":null}});
    event["delta"]["stop_reason"] = stop_reason;
    event["usage"] = usage;
    event
}

async fn decode(events: &[Value]) -> Result<llm_core::TurnOutcome, llm_core::Error> {
    let mut sink = VecSink::new(64, 64 * 1024);
    decode_stream(
        &sse(events),
        &request(),
        binding().provenance(),
        &mut sink,
        &Cancel::new(),
    )
    .await
}

const START_USAGE: fn() -> Value = || json!({"input_tokens":11,"output_tokens":1});

/// `src/decode.rs:80`: a terminal reason is "a bounded name". An unbounded one travels into
/// `StopReason::Incomplete` and out to every consumer of the neutral outcome.
#[tokio::test]
async fn a_terminal_reason_that_is_not_a_bounded_name_is_refused() {
    for reason in [
        Value::String("x".repeat(65)),
        Value::String("stop reason with spaces".to_owned()),
        Value::String(String::new()),
    ] {
        let events = vec![
            message_start(START_USAGE()),
            message_delta(reason.clone(), json!({"output_tokens":8})),
            json!({"type":"message_stop"}),
        ];
        let error = decode(&events)
            .await
            .expect_err("a terminal reason that is not a bounded name");
        assert_eq!(error.code, ErrorCode::Protocol, "{reason}");
    }
}

/// `src/decode.rs:533`: a delta must extend its block, not invent a field on it. A `thinking_delta`
/// on a text block would otherwise attach reasoning to content that is not opaque.
///
/// The turn is otherwise complete and otherwise correct, because a truncated stream refuses
/// whatever else is wrong with it — which is the reason the unit gave for rewriting three of its
/// own scenarios, and the reason this case carries a terminal event.
#[tokio::test]
async fn a_delta_that_does_not_extend_its_block_is_refused() {
    let events = vec![
        message_start(START_USAGE()),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"thinking_delta","thinking":"weighing"}}),
        json!({"type":"content_block_stop","index":0}),
        message_delta(json!("end_turn"), json!({"output_tokens":8})),
        json!({"type":"message_stop"}),
    ];
    let error = decode(&events)
        .await
        .expect_err("a thinking delta against a text block");
    assert_eq!(
        error.code,
        ErrorCode::Protocol,
        "the delta was folded into the text block and refused later, or not at all",
    );
}

/// `docs/messages.md:63`: streamed tool arguments are "parsed once when the block closes, so half
/// an argument object never reaches a caller". Half an object is not an empty one.
#[tokio::test]
async fn streamed_tool_arguments_that_never_completed_are_refused_not_emptied() {
    let events = vec![
        message_start(START_USAGE()),
        json!({"type":"content_block_start","index":0,
            "content_block":{"type":"tool_use","id":"call-1","name":"lookup","input":{}}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"input_json_delta","partial_json":"{\"query\":\"err"}}),
        json!({"type":"content_block_stop","index":0}),
        message_delta(json!("tool_use"), json!({"output_tokens":8})),
        json!({"type":"message_stop"}),
    ];
    let outcome = decode(&events).await;
    let error = outcome
        .as_ref()
        .err()
        .unwrap_or_else(|| panic!("half an argument object reached the caller: {outcome:?}"));
    assert_eq!(error.code, ErrorCode::Protocol);
}

/// The repository invariant, and `docs/messages.md:80`: a counter is exact or it is unknown. A
/// counter the route reported in a shape this codec cannot read is neither, and is refused rather
/// than quietly becoming an unreported one.
#[tokio::test]
async fn a_usage_counter_that_is_not_an_unsigned_integer_is_refused() {
    for counter in [json!(12.5), json!("11"), json!(-3), json!([11])] {
        let events = vec![
            message_start(json!({"input_tokens": counter, "output_tokens":1})),
            message_delta(json!("end_turn"), json!({"output_tokens":8})),
            json!({"type":"message_stop"}),
        ];
        let outcome = decode(&events).await;
        let error = outcome.as_ref().err().unwrap_or_else(|| {
            panic!("input_tokens {counter} was read as unknown rather than refused: {outcome:?}")
        });
        assert_eq!(error.code, ErrorCode::Protocol, "{counter}");
    }
}
