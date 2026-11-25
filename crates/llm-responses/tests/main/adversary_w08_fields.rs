//! Adversary cases for story:responses-ingress-client-fields (wave 2026-10-16-w08).
//!
//! Each case asserts a rule of `docs/design.md` "Client request fields" (Responses column) or of
//! the unit brief that the unit's own suite leaves unpinned: the suite stays green when the line
//! that implements the rule is removed. They are written against the public API only.

use std::collections::BTreeSet;

use llm_core::{Dispatch, ErrorCode, Id, Item, Protocol, Provenance, Unforwarded};
use llm_responses::{Binding, ingest_request, project_request};
use serde_json::{Value, json};

fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
}

fn binding() -> Binding {
    Binding::new(
        Provenance {
            protocol: Protocol::Responses,
            provider: id("my-lab"),
            account: id("local"),
            endpoint: id("local-vllm"),
            model: id("small"),
            binding_revision: id("rev-1"),
        },
        id("example/Small-Model"),
    )
}

// `json!` borrows its values; taking them by value keeps every call site short.
#[allow(clippy::needless_pass_by_value)]
fn body(input: Value) -> Value {
    json!({
        "model": "example/Small-Model",
        "input": input,
        "tools": [{"type": "function", "name": "file_read", "description": "d",
                   "parameters": {"type": "object"}, "strict": false}],
        "stream": true, "store": false,
        "include": ["reasoning.encrypted_content"]
    })
}

fn user() -> Value {
    json!({"type": "message", "role": "user",
           "content": [{"type": "input_text", "text": "Hi"}]})
}

fn call() -> Value {
    json!({"type": "function_call", "call_id": "call-1", "name": "file_read",
           "arguments": "{}"})
}

fn output() -> Value {
    json!({"type": "function_call_output", "call_id": "call-1", "output": "x"})
}

fn developer() -> Value {
    json!({"type": "message", "role": "developer",
           "content": [{"type": "input_text", "text": "Be brief"}]})
}

fn with(mut item: Value, key: &str, value: Value) -> Value {
    item[key] = value;
    item
}

fn only_item_ids() -> BTreeSet<Unforwarded> {
    [Unforwarded::InputItemIds].into_iter().collect()
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

/// Design: an `id` on a `function_call` item is `input-item-ids`. The unit's tests only ever
/// carry a function-call id beside message ids, so dropping `item_id` from that branch is green.
#[test]
fn an_id_on_a_function_call_alone_is_the_input_item_ids_hint() {
    let request = ingest_request(
        &binding(),
        &body(json!([user(), with(call(), "id", json!("fc_1")), output()])),
    )
    .expect("an item id is read, not refused");
    assert_eq!(request.controls.hints.unforwarded, only_item_ids());
}

/// Design: an `id` on a `function_call_output` item is `input-item-ids`. Same gap as above.
#[test]
fn an_id_on_a_function_call_output_alone_is_the_input_item_ids_hint() {
    let request = ingest_request(
        &binding(),
        &body(json!([
            user(),
            call(),
            with(output(), "id", json!("fco_1"))
        ])),
    )
    .expect("an item id is read, not refused");
    assert_eq!(request.controls.hints.unforwarded, only_item_ids());
}

/// Design: a developer message is a `message` item; its `id` is `input-item-ids` like any other.
#[test]
fn an_id_on_a_developer_message_alone_is_the_input_item_ids_hint() {
    let request = ingest_request(
        &binding(),
        &body(json!([with(developer(), "id", json!("msg_0")), user()])),
    )
    .expect("an item id is read, not refused");
    assert_eq!(request.instructions, "Be brief");
    assert_eq!(request.controls.hints.unforwarded, only_item_ids());
}

/// Brief: "a non-string `id` is `invalid`" on `message`, `function_call` and
/// `function_call_output`. The unit pins it for a user message only.
#[test]
fn a_non_string_id_is_invalid_on_every_item_kind_that_carries_one() {
    for (why, input) in [
        (
            "function_call",
            json!([user(), with(call(), "id", json!(7)), output()]),
        ),
        (
            "function_call_output",
            json!([user(), call(), with(output(), "id", json!(7))]),
        ),
        (
            "developer message",
            json!([with(developer(), "id", json!(7)), user()]),
        ),
        (
            "assistant message",
            json!([user(), {"type": "message", "role": "assistant", "id": 7,
                            "content": [{"type": "output_text", "text": "Hello"}]}]),
        ),
    ] {
        assert_eq!(
            refused(&body(input)).code,
            ErrorCode::InvalidRequest,
            "{why}"
        );
    }
}

/// Design: a `reasoning` item is carried whole, its `id` inside the payload; only `message`,
/// `function_call` and `function_call_output` ids are `input-item-ids`.
#[test]
fn a_reasoning_item_id_stays_in_the_payload_and_is_not_an_item_id_hint() {
    let reasoning = json!({"type": "reasoning", "id": "rs_1", "summary": [],
                           "encrypted_content": "Zm9v"});
    let request =
        ingest_request(&binding(), &body(json!([user(), reasoning.clone()]))).expect("carried");
    assert_eq!(
        request.items[1],
        Item::UnattributedOpaque {
            protocol: Protocol::Responses,
            payload: reasoning,
        }
    );
    assert_eq!(request.controls.hints.unforwarded, BTreeSet::new());
}

/// Design: each `input[]` item type is closed, developer messages included. The unit pins a key
/// on a developer *part*, never on the developer message itself, so dropping `closed` from
/// `developer_parts` is green.
#[test]
fn a_key_on_a_developer_message_is_refused_by_its_path() {
    let error = refused(&body(json!([
        with(developer(), "status", json!("completed")),
        user()
    ])));
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        error.message,
        "request body carries a field outside the pinned subset: input[].status"
    );
}

/// Design: `annotations` is accepted as an empty array on an `output_text` part. The unit pins
/// only the two refusals around it; refusing every `annotations` key is green.
#[test]
fn an_empty_annotations_array_on_an_assistant_part_is_accepted_and_dropped() {
    let input = json!([
        user(),
        {"type": "message", "role": "assistant",
         "content": [{"type": "output_text", "text": "Hello", "annotations": []}]},
        {"type": "message", "role": "user",
         "content": [{"type": "input_text", "text": "Again"}]}
    ]);
    let first = ingest_request(&binding(), &body(input)).expect("an empty annotations array");
    assert_eq!(first.items[1], Item::assistant("Hello"));
    let projected = project_request(&binding(), &first).expect("projects");
    assert_eq!(
        projected["input"][1]["content"],
        json!([{"type": "output_text", "text": "Hello"}])
    );
    assert_eq!(
        ingest_request(&binding(), &projected).expect("reads back"),
        first
    );
}

/// Design: a developer message after the first item of another kind is refused, and a carried
/// `reasoning` item is such an item.
#[test]
fn a_developer_message_after_a_carried_reasoning_item_is_refused() {
    let error = refused(&body(json!([
        {"type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "Zm9v"},
        developer(),
        user()
    ])));
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        error.message,
        "a standing instruction is carried once, at the head of `input`"
    );
}

/// Design: the instructions go back out as one developer message, never as a top-level
/// `instructions` field, and the item ids and `client_metadata` are not re-emitted.
#[test]
fn joined_instructions_are_never_written_back_as_a_top_level_field() {
    let mut wire = body(json!([
        with(developer(), "id", json!("msg_0")),
        with(user(), "id", json!("msg_1"))
    ]));
    wire["instructions"] = json!("Head");
    wire["client_metadata"] = json!({"turn_id": "t-1"});
    let first = ingest_request(&binding(), &wire).expect("ingests");
    assert_eq!(first.instructions, "Head\nBe brief");
    let projected = project_request(&binding(), &first).expect("projects");
    assert!(projected.get("instructions").is_none(), "{projected}");
    assert!(projected.get("client_metadata").is_none(), "{projected}");
    assert_eq!(
        projected["input"][0],
        json!({"type": "message", "role": "developer",
               "content": [{"type": "input_text", "text": "Head\nBe brief"}]})
    );
}

/// Brief: a string that *parses to* exactly the pinned envelope is read as one, whatever its
/// key order or whitespace.
#[test]
fn an_envelope_with_other_key_order_and_whitespace_is_still_the_envelope() {
    let result = with(
        output(),
        "output",
        json!(" { \"output\": 3, \"ok\": false } "),
    );
    let request =
        ingest_request(&binding(), &body(json!([user(), call(), result]))).expect("ingests");
    assert_eq!(
        request.items[2],
        Item::ToolResult {
            call_id: llm_core::CallId::new("call-1").expect("call id"),
            output: json!(3),
            failed: true,
        }
    );
}
