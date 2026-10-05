//! H31: the exact request bytes the client sends are observable without a second serialisation.
//!
//! `encode_request` is the one encoder: the client sends what it returns. Each case compares the
//! bytes a local server received with what `encode_request` returns for the same turn.

mod wire_support;

use llm_core::{Cancel, ErrorCode, Item, Model, TurnRequest, VecSink};
use llm_responses::{Conversation, encode_request, project_request};
use serde_json::Value;
use wire_support::{TEXT_STREAM, canonical_request, client, id, listener, projection, serve};

#[tokio::test]
async fn the_bytes_the_client_sends_are_the_bytes_encode_request_returns() {
    let (listener, url) = listener().await;
    let server = serve(listener, vec![TEXT_STREAM.to_vec()]);
    let client = client(&url);
    let request = canonical_request(client.provenance());
    let mut sink = VecSink::new(16, 4096);
    let _ = client.turn(&request, &mut sink, &Cancel::new()).await;
    let captured = server.await.expect("the fixture server").remove(0);
    let encoded = encode_request(&projection(&url), &request, None).expect("encodes");
    assert_eq!(
        String::from_utf8_lossy(&captured.body),
        String::from_utf8_lossy(&encoded)
    );
}

#[tokio::test]
async fn a_conversation_clients_bytes_are_the_bytes_encode_request_returns_for_it() {
    let (listener, url) = listener().await;
    let server = serve(listener, vec![TEXT_STREAM.to_vec()]);
    let conversation = Conversation::new(id("conv-1")).with_originator(id("b10x-harness"));
    let client = client(&url).with_conversation(conversation.clone());
    let request = canonical_request(client.provenance());
    let mut sink = VecSink::new(16, 4096);
    let _ = client.turn(&request, &mut sink, &Cancel::new()).await;
    let captured = server.await.expect("the fixture server").remove(0);
    let encoded =
        encode_request(&projection(&url), &request, Some(&conversation)).expect("encodes");
    assert_eq!(
        String::from_utf8_lossy(&captured.body),
        String::from_utf8_lossy(&encoded)
    );
}

/// Without a conversation the bytes are the projection, compactly encoded: one contract, not a
/// second body built beside it.
#[test]
fn without_a_conversation_the_bytes_are_the_projection() {
    let binding = projection("http://127.0.0.1:9/v1");
    let request = canonical_request(binding.provenance());
    let encoded = encode_request(&binding, &request, None).expect("encodes");
    let read: Value = serde_json::from_slice(&encoded).expect("JSON bytes");
    assert_eq!(read, project_request(&binding, &request).expect("projects"));
    assert_eq!(
        encoded,
        serde_json::to_vec(&read).expect("re-encodes"),
        "compact"
    );
}

/// The encoder refuses exactly what the projection refuses, before any byte exists.
#[test]
fn the_encoder_refuses_what_the_projection_refuses() {
    let binding = projection("http://127.0.0.1:9/v1");
    let request = TurnRequest::new("another-model", vec![Item::user("Hi")]);
    let projected = project_request(&binding, &request).expect_err("another model is refused");
    let conversation = Conversation::new(id("conv-1"));
    for conversation in [None, Some(&conversation)] {
        let encoded = encode_request(&binding, &request, conversation)
            .expect_err("the encoder refuses it too");
        assert_eq!(encoded.code, projected.code);
        assert_eq!(encoded.code, ErrorCode::InvalidRequest);
    }
}
