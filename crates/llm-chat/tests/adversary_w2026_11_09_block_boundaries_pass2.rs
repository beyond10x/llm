//! Adversary cases, pass 2, for story:ingress-block-boundaries (wave 2026-11-09, llm unit 1).
//!
//! The Chat Completions ingress promises a message boundary and an opaque item have no chunk,
//! and that `close` refuses an outcome carrying opaque state, as the non-streamed encoder does.

use llm_core::{
    ErrorCode, Id, Item, Protocol, Provenance, StopReason, StreamEvent, TurnObservation,
    TurnOutcome,
};
use llm_chat::{IngressStream, encode_ingress_completion};
use serde_json::json;

fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
}

fn messages_binding() -> Provenance {
    Provenance {
        protocol: Protocol::Messages,
        provider: id("anthropic"),
        account: id("team"),
        endpoint: id("messages"),
        model: id("large"),
        binding_revision: id("rev-1"),
    }
}

#[test]
fn a_relayed_opaque_item_has_no_chunk_and_its_outcome_is_refused_streamed_and_whole() {
    let payload = json!({"type": "redacted_thinking", "data": "AAAA"});
    let mut observation = TurnObservation::new(messages_binding());
    observation.final_usage = true;
    let turn = TurnOutcome {
        stop_reason: StopReason::EndTurn,
        items: vec![
            Item::assistant("A"),
            Item::Opaque {
                provenance: messages_binding(),
                payload: payload.clone(),
            },
            Item::assistant("B"),
        ],
        observation,
    };
    let mut stream = IngressStream::new("chatcmpl-gw-1", 1_772_000_800);
    assert!(
        stream
            .chunk(&StreamEvent::TextDelta { text: "A".to_owned() })
            .is_some()
    );
    assert_eq!(
        stream.chunk(&StreamEvent::OpaqueItem {
            provenance: messages_binding(),
            payload,
        }),
        None
    );
    assert_eq!(stream.chunk(&StreamEvent::MessageBoundary {}), None);
    assert!(
        stream
            .chunk(&StreamEvent::TextDelta { text: "B".to_owned() })
            .is_some()
    );
    let streamed = stream.close(&turn, false).expect_err("opaque state");
    assert_eq!(streamed.code, ErrorCode::Unsupported);
    let whole = encode_ingress_completion(&turn, "chatcmpl-gw-1", 1_772_000_800)
        .expect_err("the same outcome, not streamed");
    assert_eq!(whole.code, ErrorCode::Unsupported);
}
