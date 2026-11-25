//! Adversarial cases for `story:unattributed-opaque-state`: mutants the unit's own suite does not
//! reach. No implementation file is edited here.

use llm_core::{ErrorCode, Id, Item, Protocol, Provenance, TurnDocument, TurnRequest};
use serde_json::json;

fn target() -> Provenance {
    Provenance {
        protocol: Protocol::Responses,
        provider: Id::new("local-provider").unwrap(),
        account: Id::new("anonymous").unwrap(),
        endpoint: Id::new("development").unwrap(),
        model: Id::new("test-model").unwrap(),
        binding_revision: Id::new("fake-model-v1").unwrap(),
    }
}

fn unattributed(protocol: Protocol, n: u8) -> Item {
    Item::UnattributedOpaque {
        protocol,
        payload: json!({"type": "reasoning", "id": format!("rs_{n}")}),
    }
}

/// `TurnRequest::bind_unattributed`: "applied to the whole request or not at all". The unit's
/// case has one unattributed item, so an implementation that binds in place and stops at the
/// first refusal passes it. Here the first item is bindable and the second is not.
#[test]
fn a_refused_binding_leaves_no_earlier_item_bound() {
    let mut request = TurnRequest::new(
        "test-model",
        vec![
            Item::user("hi"),
            unattributed(Protocol::Responses, 1),
            unattributed(Protocol::Messages, 2),
        ],
    );
    let before = request.clone();
    let error = request.bind_unattributed(&target()).unwrap_err();
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(request, before);
}

/// The returned count is of unattributed items bound, not of opaque items present. Already-bound
/// state is left alone and not counted.
#[test]
fn the_count_is_of_unattributed_items_only_and_bound_state_is_untouched() {
    let mut elsewhere = target();
    elsewhere.binding_revision = Id::new("other-revision").unwrap();
    let foreign = Item::Opaque {
        provenance: elsewhere,
        payload: json!({"type": "reasoning", "id": "rs_0"}),
    };
    let mut request = TurnRequest::new(
        "test-model",
        vec![
            Item::user("hi"),
            foreign.clone(),
            unattributed(Protocol::Responses, 1),
            unattributed(Protocol::Responses, 2),
        ],
    );
    assert_eq!(request.bind_unattributed(&target()).unwrap(), 2);
    assert_eq!(request.items[1], foreign);
    assert!(
        request.items[2..]
            .iter()
            .all(|item| matches!(item, Item::Opaque { provenance, .. } if *provenance == target()))
    );
    assert_eq!(request.bind_unattributed(&target()).unwrap(), 0);
}

/// A persisted document cannot smuggle a binding into the unattributed variant: a `provenance`
/// field on it is refused, not silently ignored.
#[test]
fn an_unattributed_item_carrying_a_provenance_field_is_refused() {
    let document = TurnDocument::new(TurnRequest::new(
        "test-model",
        vec![Item::user("hi"), unattributed(Protocol::Responses, 1)],
    ));
    let mut value = serde_json::to_value(&document).unwrap();
    value["request"]["items"][1]["provenance"] = serde_json::to_value(target()).unwrap();
    assert!(serde_json::from_value::<TurnDocument>(value).is_err());
}
