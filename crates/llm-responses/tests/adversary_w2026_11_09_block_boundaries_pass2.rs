//! Adversary cases, pass 2, for story:ingress-block-boundaries (wave 2026-11-09, llm unit 1).
//!
//! An `OpaqueItem` other than one the Messages ingress reads back as itself reaching the
//! Responses ingress: the module promises it sends nothing for the event and that `close`
//! refuses opaque state of another binding, so a Messages upstream's redacted thinking relayed
//! to a Responses client is refused, streamed or not, and never half-carried.

use llm_core::{
    ErrorCode, Id, Item, Protocol, Provenance, StopReason, StreamEvent, TurnObservation,
    TurnOutcome, Usage,
};
use llm_responses::{IngressStream, encode_ingress_response};
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

fn outcome(items: Vec<Item>) -> TurnOutcome {
    let mut observation = TurnObservation::new(messages_binding());
    observation.upstream_model = Some(id("claude-x"));
    observation.usage = Some(Usage {
        input_tokens: Some(10),
        output_tokens: Some(5),
        ..Usage::default()
    });
    observation.final_usage = true;
    TurnOutcome {
        stop_reason: StopReason::EndTurn,
        items,
        observation,
    }
}

#[test]
fn a_messages_upstreams_redacted_thinking_is_refused_streamed_and_whole() {
    let payload = json!({"type": "redacted_thinking", "data": "AAAA"});
    let opaque = StreamEvent::OpaqueItem {
        provenance: messages_binding(),
        payload: payload.clone(),
    };
    let turn = outcome(vec![
        Item::assistant("A"),
        Item::Opaque {
            provenance: messages_binding(),
            payload,
        },
        Item::assistant("B"),
    ]);
    let mut stream = IngressStream::new("resp_gateway_1", 1_772_000_000);
    assert!(
        !stream
            .chunk(&StreamEvent::TextDelta {
                text: "A".to_owned()
            })
            .is_empty()
    );
    assert_eq!(stream.chunk(&opaque), Vec::<serde_json::Value>::new());
    assert_eq!(
        stream.chunk(&StreamEvent::MessageBoundary {}),
        Vec::<serde_json::Value>::new()
    );
    assert!(
        !stream
            .chunk(&StreamEvent::TextDelta {
                text: "B".to_owned()
            })
            .is_empty()
    );
    let streamed = stream
        .close(&turn)
        .expect_err("opaque state of another binding");
    assert_eq!(streamed.code, ErrorCode::Unsupported);
    let whole = encode_ingress_response(&turn, "resp_gateway_1", 1_772_000_000)
        .expect_err("the same outcome, not streamed");
    assert_eq!(whole.code, ErrorCode::Unsupported);
    assert_eq!(streamed.message, whole.message);
}
