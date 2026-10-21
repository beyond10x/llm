//! A text turn recorded from the live Codex backend on 2026-10-21, replayed from `127.0.0.1`.
//! No provider is contacted.
//!
//! `fixtures/live/codex-text-turn-head.txt` is the recorded response head and
//! `fixtures/live/codex-text-turn.sse` the recorded event stream, both scrubbed as
//! `fixtures/live/README.md` lists. The replay serves the stream in fixed 512-byte chunks, so
//! chunk boundaries fall inside events and inside multi-byte characters as they may on a real
//! connection.

use llm_core::{
    AuthKind, BillingKind, BoxFuture, Cancel, Capabilities, Id, Item, Model, Protocol, StopReason,
    StreamEvent, TurnRequest, VecSink,
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
    net::{TcpListener, TcpStream},
};

const HEAD: &str = include_str!("fixtures/live/codex-text-turn-head.txt");
const EVENTS: &[u8] = include_bytes!("fixtures/live/codex-text-turn.sse");

const UPSTREAM_MODEL: &str = "gpt-6-astra";
const RESPONSE_ID: &str = "resp_00000000000000000000000000000000000000000000000001";
const ANSWER: &str = "A portable LLM client most often speaks the OpenAI-compatible Chat \
Completions API and the Anthropic Messages API.";

fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
}

fn capabilities() -> Capabilities {
    Capabilities {
        tools: true,
        tool_choice: true,
        temperature: false,
        top_p: false,
        reasoning_efforts: vec!["medium".to_owned()],
        context_window: 272_000,
        max_output_tokens: 128_000,
        output_streaming: true,
        input_during_output: false,
    }
}

/// A bearer-authenticated Responses binding at `base_url`, naming the recorded model.
fn binding(base_url: &str) -> Binding {
    BindingDocument::new(
        Provider {
            id: id("codex"),
            category: id("hosted"),
        },
        Account {
            id: id("local"),
            provider_id: id("codex"),
            auth_kind: AuthKind::Bearer,
            billing_kind: BillingKind::Subscription,
            secret_reference_id: Some(SecretRef::new("codex-login").expect("reference")),
            api_key_header: None,
        },
        Endpoint {
            id: id("local-endpoint"),
            account_id: id("local"),
            base_url: BaseUrl::new(base_url).expect("fixture URL"),
        },
        ServedModel {
            id: id("astra"),
            upstream_name: id(UPSTREAM_MODEL),
        },
        ServingModel {
            id: id("serving"),
            endpoint_id: id("local-endpoint"),
            model_id: id("astra"),
            protocol: Protocol::Responses,
            capabilities: capabilities(),
        },
    )
    .bind()
    .expect("a valid fixture binding")
}

/// An injected resolver holding one fixture credential.
struct StaticResolver;

impl SecretResolver for StaticResolver {
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

fn client(url: &str) -> ResponsesClient {
    let total = Duration::from_secs(10);
    ResponsesClient::new(
        binding(url),
        HttpClient::new(Limits {
            response_headers: total,
            idle: total,
            total,
        })
        .expect("bounded transport"),
        Arc::new(StaticResolver),
    )
    .expect("a Responses binding")
}

/// The request the recording sent.
fn request() -> TurnRequest {
    let mut request = TurnRequest::new(
        "astra",
        vec![Item::user(
            "Name the two wire protocols a portable LLM client most often speaks, in one sentence.",
        )],
    );
    "Answer in one short sentence.".clone_into(&mut request.instructions);
    request
}

/// The recorded head with the framing headers the recorder dropped put back, CRLF-terminated.
fn head() -> Vec<u8> {
    let mut head = String::new();
    for line in HEAD.lines() {
        head.push_str(line);
        head.push_str("\r\n");
    }
    head.push_str("transfer-encoding: chunked\r\nconnection: close\r\n\r\n");
    head.into_bytes()
}

async fn listener() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let url = format!(
        "http://{}/backend-api/codex",
        listener.local_addr().expect("an address")
    );
    (listener, url)
}

/// Reads one whole request: its head and the body its `content-length` declares.
async fn read_request(socket: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        let length = socket.read(&mut buffer).await.expect("a readable socket");
        assert!(length > 0, "the client closed before sending a request");
        bytes.extend_from_slice(&buffer[..length]);
        assert!(bytes.len() < 256 * 1024);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let length: usize = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .expect("a bounded body")
                .trim()
                .parse()
                .expect("a numeric content-length");
            if bytes.len() >= end + 4 + length {
                return String::from_utf8(bytes).expect("UTF-8 request");
            }
        }
    }
}

/// Serves the recording once and returns what the client asked.
fn replay(listener: TcpListener) -> tokio::task::JoinHandle<String> {
    tokio::spawn(async move {
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(10), listener.accept())
            .await
            .expect("the client never opened a connection")
            .expect("an accepted connection");
        let captured = read_request(&mut socket).await;
        socket.write_all(&head()).await.expect("head written");
        for chunk in EVENTS.chunks(512) {
            let mut frame = format!("{:x}\r\n", chunk.len()).into_bytes();
            frame.extend_from_slice(chunk);
            frame.extend_from_slice(b"\r\n");
            socket.write_all(&frame).await.expect("chunk written");
        }
        socket.write_all(b"0\r\n\r\n").await.expect("last chunk");
        socket.shutdown().await.expect("closed");
        captured
    })
}

/// The recorded stream decodes to the turn the live run returned: a reasoning item kept opaque,
/// the answer, the preserved `response.metadata` event, `EndTurn`, and the terminal usage.
#[tokio::test]
async fn the_recorded_codex_text_turn_replays_to_its_live_outcome() {
    let (listener, url) = listener().await;
    let server = replay(listener);
    let mut sink = VecSink::new(64, 1 << 16);
    let outcome = client(&url)
        .turn(&request(), &mut sink, &Cancel::new())
        .await
        .expect("the recorded stream completes its turn");
    let captured = server.await.expect("the replay server");

    let head = captured
        .split_once("\r\n\r\n")
        .expect("a head and a body")
        .0
        .to_ascii_lowercase();
    assert!(
        head.starts_with("post /backend-api/codex/responses "),
        "{head}"
    );

    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
    assert_eq!(outcome.items.len(), 3, "{:?}", outcome.items);
    assert_eq!(opaque_type(&outcome.items[0]), Some("reasoning"));
    assert_eq!(
        outcome.items[1],
        Item::AssistantText {
            text: ANSWER.to_owned()
        }
    );
    // The backend's `response.metadata` event is outside the pinned subset, so it is preserved
    // as an item rather than dropped.
    assert_eq!(opaque_type(&outcome.items[2]), Some("response.metadata"));

    let observation = &outcome.observation;
    assert!(observation.final_usage);
    assert_eq!(observation.upstream_model, Some(id(UPSTREAM_MODEL)));
    assert_eq!(observation.response_id, Some(id(RESPONSE_ID)));
    let usage = observation.usage.as_ref().expect("reported usage");
    assert_eq!(usage.input_tokens, Some(34));
    assert_eq!(usage.output_tokens, Some(80));
    assert_eq!(usage.cached_input_tokens, Some(0));
    assert_eq!(usage.reasoning_output_tokens, Some(50));

    assert_eq!(sink.text(), ANSWER);
    let warnings: Vec<&str> = sink
        .events()
        .iter()
        .filter_map(|event| match event {
            StreamEvent::TextDelta { .. } => None,
            StreamEvent::Warning { code, .. } => Some(code.as_str()),
            other => panic!("unexpected event {other:?}"),
        })
        .collect();
    assert_eq!(warnings, ["unknown-stream-event"]);
}

fn opaque_type(item: &Item) -> Option<&str> {
    match item {
        Item::Opaque { payload, .. } => payload.get("type").and_then(|kind| kind.as_str()),
        _ => None,
    }
}
