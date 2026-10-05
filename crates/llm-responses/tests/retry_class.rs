//! The retry class of a Responses turn cut mid-stream. The client reads the whole stream before
//! the caller sees an event, so routing cannot count what the provider produced: the client
//! decides. A cut before any output stays retriable; a cut after any output item or delta is
//! final (Harness `a_turn_that_had_already_answered_is_never_retried`,
//! `harness-responses/tests/provider_emulated.rs:521`). Local sockets only.

use llm_core::{
    AuthKind, BillingKind, BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item,
    Model, Protocol, TurnRequest, VecSink,
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

const SSE_HEAD: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";

const CREATED: &str = "event: response.created\n\
data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"status\":\"in_progress\"}}\n\n\
event: response.in_progress\n\
data: {\"type\":\"response.in_progress\",\"response\":{\"id\":\"resp_1\",\"status\":\"in_progress\"}}\n\n";

const ITEM_ADDED: &str = "event: response.output_item.added\n\
data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[]}}\n\n";

/// Cut inside the next event, before any terminal object.
const CUT: &str = "event: response.output_text.delta\ndata: {\"type\":\"response.output";

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

/// Serves `body` after an event-stream head, closes, and returns the turn's refusal.
async fn cut_turn(body: String) -> (Error, VecSink) {
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
        socket.write_all(SSE_HEAD).await.expect("head written");
        socket
            .write_all(body.as_bytes())
            .await
            .expect("body written");
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
        .expect_err("the stream was cut before a terminal object");
    server.await.expect("the fixture server");
    (error, sink)
}

#[tokio::test]
async fn a_cut_after_only_lifecycle_events_stays_retriable() {
    let (error, sink) = cut_turn(format!("{CREATED}{CUT}")).await;
    assert_eq!(sink.events(), []);
    assert_eq!(
        (error.code, error.dispatch),
        (ErrorCode::Protocol, Dispatch::Accepted)
    );
    assert!(error.may_retry(), "nothing was answered yet: {error:?}");
}

#[tokio::test]
async fn a_cut_after_an_output_item_is_final() {
    let (error, sink) = cut_turn(format!("{CREATED}{ITEM_ADDED}{CUT}")).await;
    assert_eq!(sink.events(), []);
    assert_eq!(error.dispatch, Dispatch::Accepted);
    assert!(
        !error.retriable && !error.may_retry(),
        "an output item was produced, so the turn had answered: {error:?}"
    );
}
