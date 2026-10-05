//! R8, R13 and R44 for a route that opts in: a client given a conversation sends its cache key
//! and its identity headers, and its recorded request is pinned as a fixture.
//!
//! A client without a conversation sends neither (`wire_parity.rs`).

mod wire_support;

use llm_core::{Cancel, Item, Model, TurnRequest, VecSink};
use llm_responses::{Conversation, request_headers};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use wire_support::{
    Captured, TEXT_STREAM, body_fixture, canonical_request, client, fixture, id, listener, serve,
};

fn turn() -> TurnRequest {
    TurnRequest::new("small", vec![Item::user("Hi")])
}

async fn two_requests(conversation: Conversation) -> Vec<Captured> {
    let (listener, url) = listener().await;
    let server = serve(listener, vec![TEXT_STREAM.to_vec(), TEXT_STREAM.to_vec()]);
    let client = client(&url).with_conversation(conversation);
    for _ in 0..2 {
        let mut sink = VecSink::new(16, 4096);
        client
            .turn(&turn(), &mut sink, &Cancel::new())
            .await
            .expect("a decoded turn");
    }
    server.await.expect("the fixture server")
}

/// R13 and R8, as Harness sends them (`project.rs:360`, `lib.rs:289`-`297`): the cache key is the
/// conversation, the session header names it, and each request carries a new request id of that
/// conversation.
#[tokio::test]
async fn a_conversation_client_sends_its_cache_key_and_identity_headers_on_every_request() {
    let captured =
        two_requests(Conversation::new(id("conv-1")).with_originator(id("b10x-harness"))).await;
    assert_eq!(captured.len(), 2);
    for (number, request) in captured.iter().enumerate() {
        let body: Value = serde_json::from_slice(&request.body).expect("a JSON body");
        assert_eq!(body["prompt_cache_key"], json!("conv-1"), "{body}");
        assert_eq!(request.header("session-id").as_deref(), Some("conv-1"));
        assert_eq!(
            request.header("x-client-request-id"),
            Some(format!("conv-1-{number}"))
        );
        assert_eq!(
            request.header("originator").as_deref(),
            Some("b10x-harness")
        );
    }
}

/// The originator is the caller's to name; a conversation without one sends no such header.
#[tokio::test]
async fn a_conversation_without_an_originator_sends_no_originator_header() {
    let captured = two_requests(Conversation::new(id("conv-1"))).await;
    for request in &captured {
        assert_eq!(request.header("originator"), None, "{}", request.head);
        assert_eq!(request.header("session-id").as_deref(), Some("conv-1"));
    }
}

/// `request_headers` is the production list: every header the client sets except authentication,
/// in the order the specification names.
#[tokio::test]
async fn request_headers_are_every_header_the_client_sets_but_authentication() {
    let conversation = Conversation::new(id("conv-1")).with_originator(id("b10x-harness"));
    assert_eq!(
        request_headers(None, 0),
        vec![
            ("accept", "text/event-stream".to_owned()),
            ("content-type", "application/json".to_owned()),
        ]
    );
    let expected = vec![
        ("accept", "text/event-stream".to_owned()),
        ("content-type", "application/json".to_owned()),
        ("originator", "b10x-harness".to_owned()),
        ("session-id", "conv-1".to_owned()),
        ("x-client-request-id", "conv-1-0".to_owned()),
    ];
    assert_eq!(request_headers(Some(&conversation), 0), expected);

    let captured = two_requests(conversation).await.remove(0);
    let sent: BTreeSet<(String, String)> = captured
        .headers()
        .into_iter()
        .filter(|(name, _)| !matches!(name.as_str(), "authorization" | "host" | "content-length"))
        .collect();
    let listed: BTreeSet<(String, String)> = expected
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect();
    assert_eq!(sent, listed, "{}", captured.head);
}

/// R44 for the opted-in route: the first request of a conversation, body bytes and every
/// header, matches the recorded fixture.
#[tokio::test]
async fn the_recorded_conversation_request_matches_its_fixture() {
    let (listener, url) = listener().await;
    let server = serve(listener, vec![TEXT_STREAM.to_vec()]);
    let client = client(&url)
        .with_conversation(Conversation::new(id("conv-1")).with_originator(id("b10x-harness")));
    let request = canonical_request(client.provenance());
    let mut sink = VecSink::new(16, 4096);
    // What the turn answers is not this case's business; the request it sent is.
    let _ = client.turn(&request, &mut sink, &Cancel::new()).await;
    let captured = server.await.expect("the fixture server").remove(0);
    assert_eq!(
        String::from_utf8_lossy(&captured.body),
        String::from_utf8_lossy(&body_fixture("conversation-request.json")),
        "the exact request bytes changed; regenerate the fixture only on purpose"
    );
    assert_eq!(
        captured.recorded_head(),
        String::from_utf8(fixture("conversation-request-head.txt")).expect("UTF-8 fixture")
    );
}
