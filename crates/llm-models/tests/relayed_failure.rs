//! A gateway's HTTP 502 before output, end to end: a real protocol client built from a catalog,
//! routing's retry loop over it, and a local server answering as llm-gateway does
//! (story:relayed-failures-keep-their-retry-decision). With `x-should-retry: false` the target
//! is attempted once, on every wire; without it the same 502 is retried on the target until the
//! retry policy is spent. Local sockets only; nothing waits on a real clock between attempts.

use llm_core::{BoxFuture, Cancel, Dispatch, Error, ErrorCode, Id, Item, TurnRequest, VecSink};
use llm_credentials::SecretResolver;
use llm_http::{HttpClient, Limits};
use llm_models::CatalogModels;
use llm_routing::{
    Catalog, CatalogDocument, FallbackPolicy, FallbackRun, Halt, Ports, RetryPolicy, Selection,
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

/// llm-gateway's `upstream-failed` refusal body (`crates/llm-gateway/src/server.rs`
/// `refusal_body`, `crates/llm-gateway/src/error.rs:157`).
const GATEWAY_BODY: &str = r#"{"error":{"code":"upstream-failed","message":"the model target could not be reached or failed; the next request asks for a replacement"}}"#;

/// One anonymous endpoint at `base_url` serving one model over `protocol`, and one route to it.
fn catalog(protocol: &str, base_url: &str) -> Catalog {
    let source = format!(
        r#"format = "llm.catalog/1"

[[providers]]
id = "my-lab"
category = "self-hosted"

[[accounts]]
id = "local"
provider_id = "my-lab"
auth_kind = "anonymous"
billing_kind = "self-hosted"

[[endpoints]]
id = "gateway"
account_id = "local"
base_url = "{base_url}"

[[models]]
id = "small"
upstream_name = "example/Small-Model"

[[serving_models]]
id = "relayed-small"
endpoint_id = "gateway"
model_id = "small"
protocol = "{protocol}"
[serving_models.capabilities]
tools = false
tool_choice = false
temperature = false
top_p = false
reasoning_efforts = []
context_window = 4096
max_output_tokens = 1024

[[routes]]
id = "relayed"
alias = "code"

[[targets]]
id = "relayed-primary"
route_id = "relayed"
serving_model_id = "relayed-small"
position = 0
"#
    );
    CatalogDocument::parse(&source)
        .expect("a catalog document")
        .validate()
        .expect("a valid catalog")
}

/// Reads one request head and the body its `content-length` declares.
async fn read_request(socket: &mut TcpStream) {
    let mut bytes = Vec::new();
    let mut buffer = vec![0; 1 << 16];
    loop {
        let length = socket.read(&mut buffer).await.expect("a readable socket");
        assert!(length > 0, "the client closed before sending a request");
        bytes.extend_from_slice(&buffer[..length]);
        let text = String::from_utf8_lossy(&bytes);
        let Some((head, body)) = text.split_once("\r\n\r\n") else {
            continue;
        };
        let declared = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        if body.len() >= declared {
            return;
        }
    }
}

/// Answers every request with the gateway's 502, with or without the do-not-retry signal, and
/// counts the requests.
async fn gateway(signal: bool) -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
    let requests = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&requests);
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.expect("an accepted connection");
            read_request(&mut socket).await;
            seen.fetch_add(1, Ordering::SeqCst);
            let signal = if signal {
                "x-should-retry: false\r\n"
            } else {
                ""
            };
            let answer = format!(
                "HTTP/1.1 502 Bad Gateway\r\ncontent-type: application/json\r\n{signal}content-length: {}\r\nconnection: close\r\n\r\n{GATEWAY_BODY}",
                GATEWAY_BODY.len()
            );
            socket
                .write_all(answer.as_bytes())
                .await
                .expect("answer written");
            let _ = socket.shutdown().await;
        }
    });
    (url, requests, server)
}

/// One routed turn over `protocol` against the gateway; the run and the requests it saw.
async fn routed(protocol: &str, signal: bool) -> (FallbackRun, usize) {
    let (url, requests, server) = gateway(signal).await;
    let catalog = catalog(protocol, &url);
    let http = HttpClient::new(Limits {
        response_headers: Duration::from_secs(20),
        idle: Duration::from_secs(20),
        total: Duration::from_secs(30),
    })
    .expect("bounded transport");
    let resolvers: BTreeMap<Id, Arc<dyn SecretResolver>> = BTreeMap::new();
    let models = CatalogModels::build(&catalog, &http, &resolvers).expect("ports");
    let mut request = TurnRequest::new("code", vec![Item::user("Say hello")]);
    request.max_output_tokens = Some(64);
    let mut admit = |_: &Selection<'_>| -> Result<(), Error> { Ok(()) };
    let mut sink = VecSink::new(64, 64 * 1024);
    let cancel = Cancel::new();
    let pause = |_: Duration| -> BoxFuture<'static, ()> { Box::pin(std::future::ready(())) };
    let run = catalog
        .run_turn(
            &request,
            Some(16),
            FallbackPolicy::default(),
            Ports {
                models: &models,
                admit: &mut admit,
                sink: &mut sink,
                cancel: &cancel,
                pause: &pause,
            },
        )
        .await
        .expect("a run");
    // A second attempt would already have been answered: the run awaited it.
    server.abort();
    (run, requests.load(Ordering::SeqCst))
}

#[tokio::test]
async fn a_gateway_502_that_says_do_not_retry_is_attempted_once_on_every_wire() {
    for protocol in ["responses", "messages", "chat-completions"] {
        let (run, requests) = routed(protocol, true).await;
        assert_eq!(requests, 1, "{protocol}: the gateway was asked again");
        assert_eq!(run.attempts.len(), 1, "{protocol}: {:?}", run.attempts);
        assert_eq!(run.halt, Halt::AmbiguousDispatch, "{protocol}");
        let error = run.result.expect_err("a 502 is a failure");
        assert_eq!(
            (error.code, error.dispatch),
            (ErrorCode::Transport, Dispatch::Unknown),
            "{protocol}"
        );
        assert!(!error.may_retry(), "{protocol}: {error:?}");
        assert!(
            !format!("{error:?}").contains("upstream-failed"),
            "{protocol}: {error:?}"
        );
    }
}

#[tokio::test]
async fn the_same_502_without_the_signal_is_retried_on_its_target() {
    let attempts = usize::try_from(RetryPolicy::DEFAULT.max_attempts).expect("a small bound");
    for protocol in ["responses", "messages", "chat-completions"] {
        let (run, requests) = routed(protocol, false).await;
        assert_eq!(requests, attempts, "{protocol}");
        assert_eq!(
            run.attempts.len(),
            attempts,
            "{protocol}: {:?}",
            run.attempts
        );
        let error = run.result.expect_err("every attempt failed");
        assert_eq!(error.code, ErrorCode::Transport, "{protocol}");
        assert!(
            !error.may_retry(),
            "{protocol}: spent retries go up final: {error:?}"
        );
    }
}
