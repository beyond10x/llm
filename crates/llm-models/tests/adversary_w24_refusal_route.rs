//! Adversary, wave 2026-10-30-w24, story:chat-counts-every-output-delta.
//!
//! `Item::AssistantRefusal` is the first conversation item one wire carries and the others
//! refuse: Chat Completions sends it under `refusal`; Responses, Messages and Converse refuse it.
//! Routing selection still admits every target for it (`crates/llm-routing/src/selection.rs`
//! `capability_rejections` knows only opaque state), so a route whose admitted target cannot
//! send the conversation selects it anyway, and the turn ends `unsupported` before dispatch
//! instead of being rejected at selection as opaque state is.
//!
//! The repository's own example catalog: a Chat Completions primary and a Responses secondary.

use llm_core::{Id, Item, Protocol, TurnRequest};
use llm_routing::CatalogDocument;

const EXAMPLE: &str = include_str!("../../../examples/catalog.toml");

/// A target routing selects for a conversation is one whose wire can send that conversation.
#[test]
fn a_target_selected_for_a_conversation_carrying_a_refusal_can_send_it() {
    let mut document = CatalogDocument::parse(EXAMPLE).expect("example catalog");
    document.routes[0].fallback_enabled = true;
    let catalog = document.validate().expect("valid");
    let mut request = TurnRequest::new(
        "code",
        vec![
            Item::user("Help?"),
            Item::AssistantRefusal {
                text: "I can't help with that.".to_owned(),
            },
            Item::user("Why not?"),
        ],
    );
    request.max_output_tokens = Some(128);
    // The primary cannot take a temperature; the secondary is the first compatible target.
    request.sampling.temperature = Some(0.5);

    let Ok(selection) = catalog.resolve(&request, Some(100)) else {
        // No target admitted: selection told the incompatibility apart, as it does for opaque
        // state. That is the outcome this case accepts.
        return;
    };
    let projected = match selection.binding.provenance().protocol {
        Protocol::ChatCompletions => {
            llm_chat::project_request(&selection.request, selection.binding, true).map(|_| ())
        }
        Protocol::Responses => {
            let binding = llm_responses::Binding::new(
                selection.binding.provenance().clone(),
                Id::new(selection.binding.upstream_model()).expect("an upstream model"),
            );
            llm_responses::project_request(&binding, &selection.request).map(|_| ())
        }
        other => panic!("the example catalog has no {other:?} target"),
    };
    if let Err(error) = projected {
        let explanation = catalog.explain(&request, Some(100)).expect("explained");
        panic!(
            "selection admitted target {} ({:?}) with rejections {:?}, and its wire refuses the \
             conversation before dispatch: {:?} {}",
            selection.target.id,
            selection.binding.provenance().protocol,
            explanation
                .targets
                .iter()
                .map(|target| (target.target_id.clone(), target.rejections.clone()))
                .collect::<Vec<(Id, _)>>(),
            error.code,
            error.message
        );
    }
}
