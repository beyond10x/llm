//! Adversary, wave 2026-10-20-w11, unit 1 (story:a-resolver-for-anonymous-bindings), pass 1.
//!
//! `llm_credentials::AnonymousResolver` handed to protocol clients built without the catalog, and
//! through a caller's resolver lookup to the catalog builder. A binding that really is anonymous
//! never reaches it and sends no credential on any wire. A credentialed binding that is handed it
//! by mistake can never authenticate: that is a configuration error the caller has to fix, the
//! class `prepare_auth` reports as `unauthorized` and routing never falls back from
//! (`crates/llm-providers/src/auth.rs:111`, `crates/llm-credentials/src/lib.rs:88`). Local
//! sockets only; no provider call.

use llm_chat::ChatClient;
use llm_core::{
    BoxFuture, Cancel, Dispatch, Error, ErrorCode, Id, Item, Model, Protocol, TurnOutcome,
    TurnRequest, VecSink,
};
use llm_credentials::{AnonymousResolver, SecretResolver};
use llm_http::{HttpClient, Limits};
use llm_messages::MessagesClient;
use llm_models::CatalogModels;
use llm_providers::Binding;
use llm_responses::ResponsesClient;
use llm_routing::{Catalog, FallbackPolicy, Halt, Ports, Selection};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};

const VLLM_TEXT: &[u8] = include_bytes!("../../llm-chat/fixtures/vllm-text-no-usage.sse");

/// Headers that carry a credential or a credential's presentation on any of the three wires.
const CREDENTIAL_HEADERS: [&str; 5] = [
    "authorization",
    "x-api-key",
    "api-key",
    "anthropic-beta",
    "cookie",
];

fn id(value: &str) -> Id {
    Id::new(value).expect("an identifier")
}

fn http() -> HttpClient {
    HttpClient::new(Limits {
        response_headers: Duration::from_secs(20),
        idle: Duration::from_secs(20),
        total: Duration::from_secs(30),
    })
    .expect("a bounded transport")
}

fn serving_model(id: &str, endpoint: &str, protocol: &str) -> String {
    format!(
        r#"
[[serving_models]]
id = "{id}"
endpoint_id = "{endpoint}"
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
"#
    )
}

/// An anonymous account at `anonymous_url` and a bearer account at `credentialed_url`, each
/// serving the one model over all three protocols; route `code` tries the credentialed Chat
/// Completions target first and may fall back to the anonymous one.
fn catalog(anonymous_url: &str, credentialed_url: &str) -> Catalog {
    let mut source = format!(
        r#"format = "llm.catalog/1"

[[providers]]
id = "my-lab"
category = "self-hosted"

[[accounts]]
id = "local"
provider_id = "my-lab"
auth_kind = "anonymous"
billing_kind = "self-hosted"

[[accounts]]
id = "lab"
provider_id = "my-lab"
auth_kind = "bearer"
billing_kind = "metered"
secret_reference_id = "lab-login"

[[endpoints]]
id = "local-models"
account_id = "local"
base_url = "{anonymous_url}"

[[endpoints]]
id = "lab-models"
account_id = "lab"
base_url = "{credentialed_url}"

[[models]]
id = "small"
upstream_name = "example/Small-Model"
"#
    );
    for (prefix, endpoint) in [("local", "local-models"), ("lab", "lab-models")] {
        for (suffix, protocol) in [
            ("chat", "chat-completions"),
            ("responses", "responses"),
            ("messages", "messages"),
        ] {
            source.push_str(&serving_model(
                &format!("{prefix}-{suffix}"),
                endpoint,
                protocol,
            ));
        }
    }
    source.push_str(
        r#"
[[routes]]
id = "coding"
alias = "code"
fallback_enabled = true

[[targets]]
id = "coding-primary"
route_id = "coding"
serving_model_id = "lab-chat"
position = 0

[[targets]]
id = "coding-secondary"
route_id = "coding"
serving_model_id = "local-chat"
position = 1
"#,
    );
    Catalog::parse(&source).expect("a valid catalog")
}

/// Reads one request head and the body its `content-length` declares; returns the head.
async fn read_head(socket: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = vec![0; 1 << 16];
    loop {
        let length = socket.read(&mut buffer).await.expect("a readable socket");
        if length == 0 {
            return String::from_utf8_lossy(&bytes).into_owned();
        }
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
            return head.to_owned();
        }
    }
}

type Heads = Arc<Mutex<Vec<String>>>;

/// Answers every request with `answer` and records every request head it was sent.
async fn serve(answer: Vec<u8>) -> (String, Heads, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
    let heads = Heads::default();
    let seen = Arc::clone(&heads);
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.expect("an accepted connection");
            let head = read_head(&mut socket).await;
            seen.lock().expect("the head log").push(head);
            socket.write_all(&answer).await.expect("answer written");
            let _ = socket.shutdown().await;
        }
    });
    (url, heads, server)
}

/// A final refusal, so a turn that reached the server ends there without a retry.
fn refusal() -> Vec<u8> {
    b"HTTP/1.1 502 Bad Gateway\r\nx-should-retry: false\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
        .to_vec()
}

fn chat_answer() -> Vec<u8> {
    let mut answer = Vec::from(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
    );
    answer.extend_from_slice(VLLM_TEXT);
    answer
}

/// The protocol client a caller builds without the catalog, as the guide shows it.
fn client(binding: Binding, resolver: Arc<dyn SecretResolver>) -> Box<dyn Model> {
    match binding.provenance().protocol {
        Protocol::ChatCompletions => Box::new(ChatClient::new(binding, http(), resolver)),
        Protocol::Responses => {
            Box::new(ResponsesClient::new(binding, http(), resolver).expect("a Responses client"))
        }
        Protocol::Messages => {
            Box::new(MessagesClient::new(binding, http(), resolver).expect("a Messages client"))
        }
        Protocol::BedrockConverse => panic!("no binding serves bedrock-converse"),
    }
}

async fn turn(model: &dyn Model) -> Result<TurnOutcome, Error> {
    let mut request = TurnRequest::new("small", vec![Item::user("Hallo")]);
    request.max_output_tokens = Some(64);
    let mut sink = VecSink::new(64, 64 * 1024);
    model.turn(&request, &mut sink, &Cancel::new()).await
}

fn credential_headers(head: &str) -> Vec<&'static str> {
    let lower = head.to_ascii_lowercase();
    CREDENTIAL_HEADERS
        .into_iter()
        .filter(|name| lower.contains(&format!("\r\n{name}:")))
        .collect()
}

/// Every wire: a client built directly with the exported resolver for an anonymous binding sends
/// its request, the resolver is never reached, and no credential header goes out.
#[tokio::test]
async fn every_protocol_client_built_with_the_anonymous_resolver_sends_no_credential() {
    let (url, heads, server) = serve(refusal()).await;
    let catalog = catalog(&url, "http://127.0.0.1:9/v1");
    for serving in ["local-chat", "local-responses", "local-messages"] {
        let binding = catalog.binding(&id(serving)).expect("declared").clone();
        let model = client(binding, Arc::new(AnonymousResolver));
        let error = turn(&*model)
            .await
            .expect_err("the server refuses every turn");
        assert!(
            !error.message.contains("secret reference"),
            "{serving}: the resolver was reached: {error:?}"
        );
        assert_ne!(error.dispatch, Dispatch::NotSent, "{serving}: {error:?}");
    }
    server.abort();
    let heads = heads.lock().expect("the head log").clone();
    assert_eq!(heads.len(), 3, "one request per wire: {heads:?}");
    for head in &heads {
        assert_eq!(credential_headers(head), Vec::<&str>::new(), "{head}");
    }
}

/// A credentialed binding handed the anonymous resolver can never authenticate. It is refused
/// before anything is sent, and as the caller's configuration error, `unauthorized`, the class
/// no retry and no fallback takes; not as a store that is merely `unavailable` for now.
#[tokio::test]
async fn a_credentialed_binding_handed_the_anonymous_resolver_is_refused_unauthorized_before_sending()
 {
    let (url, heads, server) = serve(refusal()).await;
    let catalog = catalog("http://127.0.0.1:9/v1", &url);
    for serving in ["lab-chat", "lab-responses", "lab-messages"] {
        let binding = catalog.binding(&id(serving)).expect("declared").clone();
        let model = client(binding, Arc::new(AnonymousResolver));
        let error = turn(&*model)
            .await
            .expect_err("no credential can be presented");
        assert!(
            heads.lock().expect("the head log").is_empty(),
            "{serving}: a request was sent"
        );
        assert_eq!(error.dispatch, Dispatch::NotSent, "{serving}: {error:?}");
        assert!(!error.may_retry(), "{serving}: {error:?}");
        assert_eq!(error.code, ErrorCode::Unauthorized, "{serving}: {error:?}");
    }
    server.abort();
}

/// The same mistake through the catalog: a lookup that answers every account with the anonymous
/// resolver gets past construction's `unauthorized` check, and the turn must then stop on the
/// credentialed target instead of moving to the next one.
#[tokio::test]
async fn a_credentialed_target_handed_the_anonymous_resolver_through_a_lookup_does_not_fall_back() {
    let (anonymous_url, anonymous_heads, anonymous_server) = serve(chat_answer()).await;
    let (credentialed_url, credentialed_heads, credentialed_server) = serve(refusal()).await;
    let catalog = catalog(&anonymous_url, &credentialed_url);
    let lookup = |_: &Id| Some(Arc::new(AnonymousResolver) as Arc<dyn SecretResolver>);
    let models = CatalogModels::build(&catalog, &http(), &lookup)
        .expect("every port builds: the lookup answers every account");
    let mut request = TurnRequest::new("code", vec![Item::user("Hallo")]);
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
    anonymous_server.abort();
    credentialed_server.abort();
    assert!(
        credentialed_heads.lock().expect("the head log").is_empty(),
        "the credentialed target was sent a request"
    );
    let fell_back_to = anonymous_heads.lock().expect("the head log").len();
    assert_eq!(
        (run.halt, run.attempts.len(), fell_back_to),
        (Halt::IneligibleFailure, 1, 0),
        "attempts {:?}, result {:?}",
        run.attempts,
        run.result
    );
    let error = run
        .result
        .expect_err("the run stops on the credentialed target");
    assert_eq!(error.code, ErrorCode::Unauthorized, "{error:?}");
}
