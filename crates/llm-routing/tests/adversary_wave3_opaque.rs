//! Adversarial cases for `story:unattributed-opaque-state`, pass 2, driven from the contract text
//! the unit wrote. No implementation file is edited here.

use llm_core::{Id, Item, Protocol, TurnRequest};
use llm_routing::{Catalog, CatalogDocument, Rejection};
use serde_json::json;

const EXAMPLE: &str = include_str!("../../../examples/catalog.toml");

fn request_with(item: Item) -> TurnRequest {
    let mut request = TurnRequest::new("code", vec![Item::user("hello"), item]);
    request.max_output_tokens = Some(128);
    request
}

/// `docs/contract-v1.md`, turn ownership: route selection refuses unattributed state as
/// `opaque-state`, the same rejection as state bound to another binding.
///
/// **Coordinator decision, correction round 2:** routing reuses `Rejection::OpaqueState` for the
/// unbound state rather than adding a variant, so `Catalog::resolve` gives both the same
/// diagnostic. The two call for opposite remedies — unattributed state is sendable after the
/// caller binds it, foreign state never is — so the caller tells them apart **before** routing,
/// where `TurnRequest::validate_for` refuses the first with `Item::UNATTRIBUTED_REFUSAL` and the
/// second with its own message. This case pins both halves.
#[test]
fn route_selection_refuses_unattributed_state_by_a_name_distinct_from_foreign_state() {
    let mut document = CatalogDocument::parse(EXAMPLE).unwrap();
    document.routes[0].fallback_enabled = true;
    let catalog: Catalog = document.validate().unwrap();

    let unattributed = request_with(Item::UnattributedOpaque {
        protocol: Protocol::ChatCompletions,
        payload: json!({"state": "carried"}),
    });
    let binding = catalog.binding(&Id::new("local-small").unwrap()).unwrap();
    let mut elsewhere = binding.provenance().clone();
    elsewhere.binding_revision = Id::new("a-revision-no-target-has").unwrap();
    let foreign = request_with(Item::Opaque {
        provenance: elsewhere,
        payload: json!({"state": "carried"}),
    });

    // Routing: one rejection for both, by the decision above.
    for request in [&unattributed, &foreign] {
        let explanation = catalog.explain(request, Some(100)).unwrap();
        assert!(explanation.selected_target_id.is_none());
        for target in &explanation.targets {
            assert!(
                target.rejections.contains(&Rejection::OpaqueState),
                "route selection refuses both unattributed and foreign state as opaque-state \
                 (decision: reuse Rejection::OpaqueState): {:?}",
                target.rejections
            );
        }
    }
    let refused_unattributed = catalog
        .resolve(&unattributed, Some(100))
        .map(|selection| selection.target.id.clone())
        .expect_err("no target is compatible with unattributed state");
    let refused_foreign = catalog
        .resolve(&foreign, Some(100))
        .map(|selection| selection.target.id.clone())
        .expect_err("no target is compatible with foreign state");
    assert_eq!(
        (refused_unattributed.code, &refused_unattributed.message),
        (refused_foreign.code, &refused_foreign.message),
        "route selection gives both the same diagnostic, by the decision to reuse opaque-state"
    );

    // Before routing, the caller tells them apart by name.
    let at_target = |request: &TurnRequest| {
        let mut request = request.clone();
        binding
            .provenance()
            .model
            .as_str()
            .clone_into(&mut request.model);
        request
            .validate_for(binding.provenance(), binding.capabilities())
            .unwrap_err()
    };
    let unattributed = at_target(&unattributed);
    let foreign = at_target(&foreign);
    assert_eq!(unattributed.message, Item::UNATTRIBUTED_REFUSAL);
    assert_eq!(
        foreign.message, "opaque state belongs to a different serving binding",
        "validate_for is where unattributed state is named apart from foreign state"
    );
}
