//! Adversarial cases for `story:unattributed-opaque-state`, driven from the documents the unit
//! wrote about itself. No implementation file is edited here.

use llm_core::{ErrorCode, Id, Protocol, Provenance};
use llm_responses::{Binding, ingest_request};
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

fn body_with(entry: &Value) -> Value {
    json!({
        "model": "example/Small-Model",
        "input": [
            {"type": "message", "role": "user",
             "content": [{"type": "input_text", "text": "Hi"}]},
            entry
        ],
        "tools": [], "stream": true, "store": false,
        "include": ["reasoning.encrypted_content"]
    })
}

/// `docs/responses.md:43`: `store` is "always `false`; the conversation is the caller's, replayed
/// whole every turn", and `ingest_request`'s doc comment refuses "provider-stored conversations".
///
/// An `item_reference` entry is not continuation state the caller holds: it is a pointer to an
/// item the provider stored. At the base commit it was refused by the catch-all. The new `Some(_)`
/// arm reads every typed entry as opaque continuation state, so a reference to provider-side
/// state is now carried, and one `bind_unattributed` call away from being forwarded.
#[test]
fn a_reference_to_provider_stored_state_is_still_refused_on_ingress() {
    let error = ingest_request(
        &binding(),
        &body_with(&json!({"type": "item_reference", "id": "msg_stored_elsewhere"})),
    )
    .map(|request| request.items)
    .expect_err("a provider-stored conversation reference is refused, not carried");
    assert_eq!(error.code, ErrorCode::Unsupported);
}

/// `docs/responses.md`: "An entry that names no type at all is not opaque state and is still
/// refused with `Unsupported`." An empty `type` names no type; the arm that decides is
/// `Some(_)`, which admits the empty string.
#[test]
fn an_entry_whose_type_is_empty_names_no_type_and_is_refused() {
    let error = ingest_request(&binding(), &body_with(&json!({"type": "", "id": "rs_1"})))
        .map(|request| request.items)
        .expect_err("an empty type names no type");
    assert_eq!(error.code, ErrorCode::Unsupported);
}
