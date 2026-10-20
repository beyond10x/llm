//! The do-not-retry signal on an error status, through the Chat Completions client
//! (story:relayed-failures-keep-their-retry-decision). A gateway that spent its retries on a
//! failure before output answers HTTP 502 with `x-should-retry: false`; the client reads it final
//! and sends once, so routing does not call the gateway again. The same 502 without the signal
//! stays retriable, and the signal never makes a final status retriable. Local sockets only.
mod common;

use common::binding_at;
use llm_chat::ChatClient;
use llm_core::{
    Cancel, Capabilities, Dispatch, Error, ErrorCode, Item, Model, Protocol, TurnRequest, VecSink,
};
use llm_credentials::{ResolvedSecret, SecretError, SecretRef, SecretResolver};
use llm_http::{HttpClient, Limits};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

/// The provider prose every fixture failure carries, which no diagnostic may repeat.
const PROSE: &str = "SECRET-UPSTREAM-TEXT";

/// An anonymous binding never reaches a resolver; this one proves it by refusing.
struct NoResolver;
impl SecretResolver for NoResolver {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> llm_core::BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async { Err(SecretError::Missing) })
    }
}

/// Answers the first request with `status` and llm-gateway's refusal body (code
/// `upstream-failed`) carrying the head `fields`, and returns the turn's refusal, what the caller
/// was shown and how many requests arrived.
async fn refused_turn(status: &str, fields: &str) -> (Error, VecSink, usize) {
    let body = format!(r#"{{"error":{{"code":"upstream-failed","message":"{PROSE}"}}}}"#);
    let head = format!(
        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\n{fields}content-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("an accepted connection");
        let mut seen = Vec::new();
        let mut buffer = vec![0; 1 << 16];
        while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
            let length = socket.read(&mut buffer).await.expect("a readable socket");
            assert!(length > 0, "the client closed before sending a request");
            seen.extend_from_slice(&buffer[..length]);
        }
        socket
            .write_all(head.as_bytes())
            .await
            .expect("head written");
        socket
            .write_all(body.as_bytes())
            .await
            .expect("body written");
        let _ = socket.shutdown().await;
        let resent = tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_ok();
        1 + usize::from(resent)
    });
    let client = ChatClient::new(
        binding_at(
            Protocol::ChatCompletions,
            Capabilities::text(32_768, 4_096),
            &url,
        ),
        HttpClient::new(Limits::default()).expect("bounded transport"),
        Arc::new(NoResolver),
    );
    let mut sink = VecSink::new(64, 64 * 1024);
    let error = client
        .turn(
            &TurnRequest::new("small", vec![Item::user("Hallo")]),
            &mut sink,
            &Cancel::new(),
        )
        .await
        .expect_err("an error status is a refusal");
    let requests = server.await.expect("the fixture server");
    (error, sink, requests)
}

#[tokio::test]
async fn a_gateway_502_that_says_do_not_retry_is_final_and_without_it_is_retriable() {
    for (fields, retriable) in [
        ("x-should-retry: false\r\n", false),
        ("X-Should-Retry: false\r\n", false),
        ("", true),
    ] {
        let (error, sink, requests) = refused_turn("502 Bad Gateway", fields).await;
        assert_eq!(
            (error.code, error.dispatch),
            (ErrorCode::Transport, Dispatch::Unknown),
            "`{fields}`"
        );
        assert_eq!(error.may_retry(), retriable, "`{fields}`: {error:?}");
        assert_eq!(requests, 1, "`{fields}`: the client sent the turn again");
        assert_eq!(sink.events(), [], "`{fields}`");
        assert!(
            !format!("{error:?} {error}").contains(PROSE),
            "`{fields}`: {error:?}"
        );
    }
}

/// `x-should-retry: true` never widens: a 400 stays final.
#[tokio::test]
async fn the_signal_never_makes_a_final_status_retriable() {
    let (error, _, requests) = refused_turn("400 Bad Request", "x-should-retry: true\r\n").await;
    assert_eq!(
        (error.code, error.dispatch),
        (ErrorCode::Refused, Dispatch::Rejected)
    );
    assert!(!error.retriable && !error.may_retry(), "{error:?}");
    assert_eq!(requests, 1);
}
