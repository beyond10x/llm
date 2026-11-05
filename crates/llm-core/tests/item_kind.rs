//! `Item::kind` names an item by the `kind` its serialized form carries. `Item` may gain kinds, so
//! a match on it outside `llm-core` ends in a wildcard arm, and that arm refuses by this name: fixed
//! text, never a byte of the item's content.

use llm_core::{CallId, Id, Item, Protocol, Provenance, ToolCall, ToolName};
use serde_json::json;

fn every_kind() -> Vec<Item> {
    vec![
        Item::user("What does a.txt say?"),
        Item::assistant("Reading it."),
        Item::AssistantRefusal {
            text: "I cannot help with that.".to_owned(),
        },
        Item::ToolCall(ToolCall {
            call_id: CallId::new("call-1").unwrap(),
            name: ToolName::new("read").unwrap(),
            arguments: json!({"path": "a.txt"}),
        }),
        Item::ToolResult {
            call_id: CallId::new("call-1").unwrap(),
            output: json!("hello"),
            failed: false,
        },
        Item::Opaque {
            provenance: Provenance {
                protocol: Protocol::Messages,
                provider: Id::new("lab").unwrap(),
                account: Id::new("account").unwrap(),
                endpoint: Id::new("endpoint").unwrap(),
                model: Id::new("code").unwrap(),
                binding_revision: Id::new("revision-1").unwrap(),
            },
            payload: json!({"type": "thinking", "thinking": "t", "signature": "s"}),
        },
        Item::UnattributedOpaque {
            protocol: Protocol::Responses,
            payload: json!({"type": "reasoning", "id": "rs_1"}),
        },
    ]
}

#[test]
fn every_item_names_the_kind_its_serialized_form_carries() {
    let items = every_kind();
    for item in &items {
        let written = serde_json::to_value(item).unwrap();
        assert_eq!(written["kind"], item.kind(), "{item:?}");
    }
    let mut kinds: Vec<&str> = items.iter().map(Item::kind).collect();
    kinds.sort_unstable();
    kinds.dedup();
    assert_eq!(
        kinds.len(),
        items.len(),
        "two kinds share a name: {kinds:?}"
    );
}
