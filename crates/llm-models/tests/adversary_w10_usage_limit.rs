//! Adversary, wave 2026-10-20-w10, unit http, pass 1 (story:quota-429-is-final, "one request per
//! quota case"). A routed Responses turn whose endpoint answers the Codex backend's subscription
//! usage limit: HTTP 429, `error.type` `usage_limit_reached`, no `error.code` (the shape the
//! official Codex CLI's fixtures use, `openai/codex` 8196bcdc
//! `codex-rs/codex-api/src/api_bridge_tests.rs:547`; terminal there,
//! `codex-rs/protocol/src/error.rs:433`). The limit resets in hours, so every retry meets it again.
//! Local sockets only; the pause port does not wait.

use llm_core::{BoxFuture, Cancel, Error, ErrorCode, Id, Item, TurnRequest, VecSink};
use llm_credentials::SecretResolver;
use llm_http::{HttpClient, Limits};
use llm_models::CatalogModels;
use llm_routing::{Catalog, CatalogDocument, FallbackPolicy, Ports, Selection};
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

const USAGE_LIMIT: &str = r#"{"error":{"type":"usage_limit_reached","message":"SECRET-UPSTREAM-TEXT","plan_type":"pro","resets_at":1900000000}}"#;

fn catalog(base_url: &str) -> Catalog {
    let source = format!(
        r#"format = "llm.catalog/1"

[[providers]]
id = "my-lab"
category = "hosted"

[[accounts]]
id = "local"
provider_id = "my-lab"
auth_kind = "anonymous"
billing_kind = "self-hosted"

[[endpoints]]
id = "backend"
account_id = "local"
base_url = "{base_url}"

[[models]]
id = "small"
upstream_name = "example/Small-Model"

[[serving_models]]
id = "backend-small"
endpoint_id = "backend"
model_id = "small"
protocol = "responses"
[serving_models.capabilities]
tools = false
tool_choice = false
temperature = false
top_p = false
reasoning_efforts = []
context_window = 4096
max_output_tokens = 1024

[[routes]]
id = "subscription"
alias = "code"

[[targets]]
id = "subscription-primary"
route_id = "subscription"
serving_model_id = "backend-small"
position = 0
"#
    );
    CatalogDocument::parse(&source)
        .expect("a catalog document")
        .validate()
        .expect("a valid catalog")
}

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

#[tokio::test]
async fn a_subscription_usage_limit_429_is_asked_once_and_ends_the_run() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let url = format!(
        "http://{}/backend-api/codex",
        listener.local_addr().expect("an address")
    );
    let requests = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&requests);
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.expect("an accepted connection");
            read_request(&mut socket).await;
            seen.fetch_add(1, Ordering::SeqCst);
            let answer = format!(
                "HTTP/1.1 429 Too Many Requests\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{USAGE_LIMIT}",
                USAGE_LIMIT.len()
            );
            socket
                .write_all(answer.as_bytes())
                .await
                .expect("answer written");
            let _ = socket.shutdown().await;
        }
    });
    let catalog = catalog(&url);
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
    server.abort();
    let error = run.result.expect_err("a usage limit is a failure");
    assert_eq!(
        requests.load(Ordering::SeqCst),
        1,
        "the backend was asked again after a usage limit that resets in hours: {:?}",
        run.attempts
    );
    assert_eq!(error.code, ErrorCode::Refused, "{error:?}");
}
