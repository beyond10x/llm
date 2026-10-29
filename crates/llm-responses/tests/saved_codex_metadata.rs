//! story:saved-codex-metadata-item-on-resume (wave 2026-10-29-w22, orchestrator item D208).
//!
//! llm 0.16.0 kept the Codex `response.metadata` stream event as an `Opaque` item of the binding.
//! A conversation saved then and resumed now must not send it: the endpoint refuses the entry with
//! HTTP 400 "Invalid value: 'response.metadata'". Every other item is sent as before.
//!
//! `fixtures/live/codex-text-turn-saved-0.16.0.json` is what 0.16.0's `decode_stream` made of the
//! recorded live Codex turn, after the user message, serialized with `serde_json` (see the
//! fixture README). No provider is contacted.

use llm_core::{ErrorCode, Id, Item, Protocol, Provenance, TurnRequest};
use llm_responses::{Binding, encode_request, project_request};
use serde_json::{Value, json};

const SAVED: &str = include_str!("fixtures/live/codex-text-turn-saved-0.16.0.json");

fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
}

/// The binding coordinates the fixture was saved under.
fn provenance() -> Provenance {
    Provenance {
        protocol: Protocol::Responses,
        provider: id("codex"),
        account: id("local"),
        endpoint: id("local-endpoint"),
        model: id("astra"),
        binding_revision: id("rev-1"),
    }
}

fn binding() -> Binding {
    Binding::new(provenance(), id("gpt-6-astra"))
}

fn saved() -> Vec<Item> {
    serde_json::from_str(SAVED).expect("the saved 0.16.0 conversation deserializes")
}

fn is_metadata(item: &Item) -> bool {
    matches!(item, Item::Opaque { payload, .. } if payload["type"] == "response.metadata")
}

fn resumed(items: Vec<Item>) -> TurnRequest {
    let mut items = items;
    items.push(Item::user("And which one does Codex speak?"));
    TurnRequest::new("astra", items)
}

fn input(body: &Value) -> &Vec<Value> {
    body["input"].as_array().expect("an input list")
}

/// The fixture is the shape the story is about: 0.16.0 kept the event as an opaque item of the
/// binding, beside the reasoning item and the answer.
#[test]
fn the_saved_conversation_holds_the_metadata_event_as_an_opaque_item_of_the_binding() {
    let items = saved();
    assert_eq!(items.len(), 4, "{items:?}");
    let metadata: Vec<&Item> = items.iter().filter(|item| is_metadata(item)).collect();
    assert_eq!(metadata.len(), 1, "{items:?}");
    let Item::Opaque { provenance: kept, .. } = metadata[0] else {
        unreachable!("is_metadata matched an opaque item");
    };
    assert_eq!(kept, &provenance());
}

/// The resumed turn's request carries no `response.metadata` entry, and is byte for byte the
/// request of the same conversation without that item: user message, reasoning, answer and the
/// new user message, in order.
#[test]
fn a_conversation_saved_under_0_16_0_resumes_without_the_metadata_entry() {
    let request = resumed(saved());
    let body = project_request(&binding(), &request).expect("the resumed turn projects");
    let types: Vec<&str> = input(&body)
        .iter()
        .filter_map(|entry| entry["type"].as_str())
        .collect();
    assert!(!types.contains(&"response.metadata"), "{types:?}");

    let without = resumed(saved().into_iter().filter(|item| !is_metadata(item)).collect());
    let expected = project_request(&binding(), &without).expect("projects");
    assert_eq!(body, expected);
    assert_eq!(input(&body).len(), 4, "{:?}", input(&body));
    assert_eq!(input(&body)[1]["type"], "reasoning");
    assert_eq!(
        input(&body)[1]["encrypted_content"],
        "gAAAAA-scrubbed-reasoning"
    );

    let encoded = encode_request(&binding(), &request, None).expect("the resumed turn encodes");
    let encoded: Value = serde_json::from_slice(&encoded).expect("the encoded body is JSON");
    assert_eq!(encoded["input"], expected["input"]);
}

/// The item is left out wherever the conversation holds it, and every copy of it.
#[test]
fn a_saved_metadata_item_is_left_out_at_every_position_and_every_copy() {
    let items = saved();
    let metadata = items
        .iter()
        .find(|item| is_metadata(item))
        .cloned()
        .expect("the fixture holds one");
    let others: Vec<Item> = items.into_iter().filter(|item| !is_metadata(item)).collect();
    let expected = project_request(&binding(), &resumed(others.clone())).expect("projects");
    for at in 0..=others.len() {
        for copies in [1, 2] {
            let mut conversation = others.clone();
            for _ in 0..copies {
                conversation.insert(at, metadata.clone());
            }
            let body = project_request(&binding(), &resumed(conversation))
                .unwrap_or_else(|error| panic!("x{copies} at {at}: {error:?}"));
            assert_eq!(body, expected, "x{copies} at {at}");
        }
    }
}

/// Opaque state of another binding is refused as before, before anything is left out: the
/// change narrows what is sent and never what is checked.
#[test]
fn a_saved_metadata_item_of_another_binding_is_still_refused() {
    let mut other = provenance();
    other.binding_revision = id("rev-2");
    let items = vec![
        Item::user("first question"),
        Item::Opaque {
            provenance: other,
            payload: json!({"type": "response.metadata", "metadata": {}}),
        },
    ];
    let error = project_request(&binding(), &resumed(items)).expect_err("another binding's state");
    assert_eq!(error.code, ErrorCode::Unsupported);
}

/// Only the metadata event is left out. Every other opaque item of the binding, among them one
/// whose `type` names another stream event (an event outside the pinned subset is still kept as an
/// opaque item and warned about), one with no `type` and one whose `type` is not a string, is
/// sent verbatim, in place.
#[test]
fn every_other_opaque_item_is_still_sent_verbatim() {
    let payloads = [
        json!({"type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "x"}),
        json!({"type": "response.not_in_the_pin", "sequence_number": 7}),
        json!({"type": "response.metadata.v2", "metadata": {}}),
        json!({"type": "Response.Metadata", "metadata": {}}),
        json!({"type": ["response.metadata"]}),
        json!({"metadata": {"type": "response.metadata"}}),
        json!({}),
    ];
    for payload in payloads {
        let items = vec![
            Item::user("first question"),
            Item::Opaque {
                provenance: provenance(),
                payload: payload.clone(),
            },
            Item::assistant("first answer"),
        ];
        let body = project_request(&binding(), &resumed(items)).expect("projects");
        assert_eq!(input(&body).len(), 4, "{payload}");
        assert_eq!(input(&body)[1], payload);
    }
}
