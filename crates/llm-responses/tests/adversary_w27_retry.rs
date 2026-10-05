//! Adversary pass 1 (wave 2026-10-05-w27): the retry class of a Responses turn that had
//! already answered. Local sockets only; no provider is contacted.
//!
//! Routing retries a failure when `Error::may_retry` holds and no event of the attempt reached
//! the caller's sink (`crates/llm-routing/src/fallback.rs`, `settle`). When this case was
//! written `ResponsesClient` read the whole stream before it emitted a single event, so a stream
//! that streamed its answer and was then cut reached routing with nothing emitted. It now hands
//! each event over as it arrives (`spec/domains/responses.yaml`, "Live delivery"), and the
//! assertion holds either way: the answer was shown, or the failure is final. Harness's own
//! emulated run says that turn is final:
//! `a_turn_that_had_already_answered_is_never_retried`
//! (`harness-responses/tests/provider_emulated.rs:521`), the row R42 marks covered.

use llm_core::{
    AuthKind, BillingKind, BoxFuture, Cancel, Capabilities, Id, Item, Model, Protocol, TurnRequest,
    VecSink,
};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_http::{HttpClient, Limits};
use llm_providers::{
    Account, BaseUrl, Binding, BindingDocument, Endpoint, Provider, ServedModel, ServingModel,
};
use llm_responses::ResponsesClient;
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

/// The answer `Hello` streamed in two deltas, then the connection closes inside the next event,
/// before `response.completed`.
const ANSWERED_THEN_CUT: &str = "event: response.created\n\
data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"status\":\"in_progress\"}}\n\n\
event: response.output_text.delta\n\
data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"delta\":\"Hel\"}\n\n\
event: response.output_text.delta\n\
data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"delta\":\"lo\"}\n\n\
event: response.output_text.delta\n\
data: {\"type\":\"response.output_text.del";

const SSE_HEAD: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";

fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
}

fn binding(base_url: &str) -> Binding {
    BindingDocument::new(
        Provider {
            id: id("my-lab"),
            category: id("hosted"),
        },
        Account {
            id: id("local"),
            provider_id: id("my-lab"),
            auth_kind: AuthKind::Bearer,
            billing_kind: BillingKind::Metered,
            secret_reference_id: Some(SecretRef::new("responses-key").expect("reference")),
            api_key_header: None,
        },
        Endpoint {
            id: id("local-endpoint"),
            account_id: id("local"),
            base_url: BaseUrl::new(base_url).expect("fixture URL"),
        },
        ServedModel {
            id: id("small"),
            upstream_name: id("example/Small-Model"),
        },
        ServingModel {
            id: id("serving"),
            endpoint_id: id("local-endpoint"),
            model_id: id("small"),
            protocol: Protocol::Responses,
            capabilities: Capabilities {
                tools: true,
                tool_choice: true,
                temperature: true,
                top_p: true,
                reasoning_efforts: vec!["medium".to_owned()],
                context_window: 32_768,
                max_output_tokens: 2_048,
            },
        },
    )
    .bind()
    .expect("a valid fixture binding")
}

struct Resolver;

impl SecretResolver for Resolver {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move {
            Ok(ResolvedSecret {
                secret: Secret::new(b"fixture-token".to_vec())?,
                version: SecretVersion::new("generation-1".to_owned())?,
            })
        })
    }
}

#[tokio::test]
async fn a_responses_turn_that_had_already_answered_is_not_offered_for_another_attempt() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
    let server = tokio::spawn(async move {
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(20), listener.accept())
            .await
            .expect("the client never connected")
            .expect("an accepted connection");
        let mut seen = Vec::new();
        let mut buffer = vec![0; 1 << 16];
        // Read the head and whatever body arrives with it; the answer does not depend on it.
        while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
            let length = socket.read(&mut buffer).await.expect("a readable socket");
            assert!(length > 0, "the client closed before sending a request");
            seen.extend_from_slice(&buffer[..length]);
        }
        socket.write_all(SSE_HEAD).await.expect("head written");
        socket
            .write_all(ANSWERED_THEN_CUT.as_bytes())
            .await
            .expect("body written");
        socket.flush().await.expect("flushed");
        let _ = socket.shutdown().await;
    });
    let client = ResponsesClient::new(
        binding(&url),
        HttpClient::new(Limits {
            response_headers: Duration::from_secs(20),
            idle: Duration::from_secs(20),
            total: Duration::from_secs(30),
        })
        .expect("bounded transport"),
        Arc::new(Resolver),
    )
    .expect("a Responses binding");
    let request = TurnRequest::new("small", vec![Item::user("Say hello")]);
    let mut sink = VecSink::new(64, 64 * 1024);
    let error = client
        .turn(&request, &mut sink, &Cancel::new())
        .await
        .expect_err("the stream was cut before response.completed");
    server.await.expect("the fixture server");
    assert!(
        !sink.events().is_empty() || !error.may_retry(),
        "the provider streamed the answer `Hello` before the cut, yet nothing reached the sink \
         and the failure may be retried, so routing replays an answered turn: {error:?}"
    );
}
