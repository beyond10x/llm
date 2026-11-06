//! The `OpenAI` embeddings wire against a loopback fixture server whose answers are the shapes of
//! the `OpenAI` API reference (`POST /v1/embeddings`): no live call.

use llm_calls::{CallIdentity, CallLog, CallRecord, Clock, Kind, Outcome, Recording, Role};
use llm_core::{BoxFuture, Cancel, Dispatch, EmbedRequest, Embedder, ErrorCode, Id};
use llm_cost::{Amount, Currency, ModelPrice, PriceBook, PriceDocument, Rate};
use llm_credentials::{
    AnonymousResolver, ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver,
    SecretVersion,
};
use llm_embed::OpenAiEmbedder;
use llm_http::{HttpClient, Limits};
use llm_routing::Catalog;
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const TEXT_MARKER: &str = "text-marker-pangolin-4471";

/// One answer of the fixture server, in request order; the last one repeats.
#[derive(Clone)]
enum Answer {
    Json(Value),
    Status(u16),
    Hang,
}

#[derive(Default)]
struct Seen {
    connections: AtomicUsize,
    bodies: Mutex<Vec<Value>>,
    paths: Mutex<Vec<String>>,
    authorizations: Mutex<Vec<Option<String>>>,
}

impl Seen {
    fn bodies(&self) -> Vec<Value> {
        self.bodies
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

async fn read_request(socket: &mut TcpStream) -> Option<(String, Option<String>, Vec<u8>)> {
    let mut bytes = Vec::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    let end = loop {
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break end;
        }
        let read = socket.read(&mut buffer).await.ok()?;
        if read == 0 {
            return None;
        }
        bytes.extend_from_slice(&buffer[..read]);
    };
    let head = String::from_utf8_lossy(&bytes[..end]).into_owned();
    let header = |wanted: &str| {
        head.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case(wanted)
                .then(|| value.trim().to_owned())
        })
    };
    let path = head.split_whitespace().nth(1)?.to_owned();
    let length: usize = header("content-length")?.parse().ok()?;
    let mut body = bytes[end + 4..].to_vec();
    while body.len() < length {
        let read = socket.read(&mut buffer).await.ok()?;
        if read == 0 {
            return None;
        }
        body.extend_from_slice(&buffer[..read]);
    }
    Some((path, header("authorization"), body))
}

async fn serve(answers: Vec<Answer>) -> (SocketAddr, Arc<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let seen = Arc::new(Seen::default());
    let shared = seen.clone();
    tokio::spawn(async move {
        let mut served = 0_usize;
        while let Ok((mut socket, _)) = listener.accept().await {
            shared.connections.fetch_add(1, Ordering::SeqCst);
            let Some((path, authorization, body)) = read_request(&mut socket).await else {
                continue;
            };
            shared
                .bodies
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(serde_json::from_slice(&body).unwrap_or(Value::Null));
            shared
                .paths
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(path);
            shared
                .authorizations
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(authorization);
            let answer = answers[served.min(answers.len() - 1)].clone();
            served += 1;
            let (status, body) = match answer {
                Answer::Json(value) => (200, serde_json::to_vec(&value).unwrap()),
                Answer::Status(status) => (status, br#"{"error":{"message":"no"}}"#.to_vec()),
                Answer::Hang => {
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_secs(30)).await;
                        drop(socket);
                    });
                    continue;
                }
            };
            let head = format!(
                "HTTP/1.1 {status} Fixture\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            let _ = socket.write_all(head.as_bytes()).await;
            let _ = socket.write_all(&body).await;
            let _ = socket.shutdown().await;
        }
    });
    (address, seen)
}

/// An embed route `embed` (target `embed-primary`, upstream `text-embedding-3-small`) and a turn
/// route `chat`, both served by the fixture at `address`.
fn catalog_toml(address: SocketAddr, capabilities: &str, auth: &str) -> String {
    format!(
        r#"format = "llm.catalog/1"

[[providers]]
id = "openai-compatible"
category = "hosted"

[[accounts]]
id = "main"
provider_id = "openai-compatible"
{auth}

[[endpoints]]
id = "fixture"
account_id = "main"
base_url = "http://{address}/v1"

[[models]]
id = "embedding-small"
upstream_name = "text-embedding-3-small"

[[models]]
id = "chat-small"
upstream_name = "gpt-4o-mini"

[[serving_models]]
id = "embedder"
endpoint_id = "fixture"
model_id = "embedding-small"
protocol = "chat-completions"
[serving_models.capabilities]
tools = false
tool_choice = false
temperature = false
top_p = false
reasoning_efforts = []
context_window = 8191
max_output_tokens = 1
{capabilities}

[[serving_models]]
id = "chatter"
endpoint_id = "fixture"
model_id = "chat-small"
protocol = "chat-completions"
[serving_models.capabilities]
tools = false
tool_choice = false
temperature = false
top_p = false
reasoning_efforts = []
context_window = 4096
max_output_tokens = 1024

[[routes]]
id = "embedding"
alias = "embed"
kind = "embed"

[[routes]]
id = "chatting"
alias = "chat"

[[targets]]
id = "embed-primary"
route_id = "embedding"
serving_model_id = "embedder"
position = 0

[[targets]]
id = "chat-primary"
route_id = "chatting"
serving_model_id = "chatter"
position = 0
"#
    )
}

const ANONYMOUS: &str = "auth_kind = \"anonymous\"\nbilling_kind = \"self-hosted\"";

fn catalog(address: SocketAddr, capabilities: &str) -> Catalog {
    Catalog::parse(&catalog_toml(address, capabilities, ANONYMOUS)).unwrap()
}

fn http() -> HttpClient {
    HttpClient::new(Limits {
        response_headers: Duration::from_secs(20),
        idle: Duration::from_secs(20),
        total: Duration::from_secs(20),
    })
    .unwrap()
}

fn embedder(catalog: &Catalog) -> OpenAiEmbedder {
    OpenAiEmbedder::for_route(catalog, "embed", http(), Arc::new(AnonymousResolver)).unwrap()
}

fn texts(count: usize) -> Vec<String> {
    (0..count).map(|i| format!("{TEXT_MARKER} {i}")).collect()
}

fn request(count: usize) -> EmbedRequest {
    EmbedRequest::new(texts(count), Duration::from_secs(10))
}

/// The vector the fixture returns for the text at `position` of the whole request.
fn vector(position: usize) -> Vec<f32> {
    let base = f32::from(u16::try_from(position).unwrap());
    vec![base + 0.25, -base - 0.5, 0.002_306_425_5]
}

/// An answer in the shape of the `OpenAI` API reference, for the texts at `positions` of the whole
/// request, listed in `order` (indexes into the batch).
fn answer(positions: &[usize], order: &[usize], usage: Option<u64>) -> Value {
    let data: Vec<Value> = order
        .iter()
        .map(|&index| {
            json!({"object": "embedding", "embedding": vector(positions[index]), "index": index})
        })
        .collect();
    let mut body = json!({"object": "list", "data": data, "model": "text-embedding-3-small"});
    if let Some(tokens) = usage {
        body["usage"] = json!({"prompt_tokens": tokens, "total_tokens": tokens});
    }
    body
}

/// A clock that never moves: every record starts at `unix_ms` and takes no time.
struct Frozen(u64);

impl Clock for Frozen {
    fn unix_ms(&self) -> u64 {
        self.0
    }
    fn monotonic(&self) -> Duration {
        Duration::ZERO
    }
}

fn recording<'r>(
    log: &'r CallLog,
    clock: &'r Frozen,
    prices: Option<&'r PriceBook>,
) -> Recording<'r> {
    Recording {
        sink: log,
        clock,
        prices,
    }
}

fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}

#[tokio::test]
async fn embed_returns_one_vector_per_text_in_input_order() {
    let (address, seen) = serve(vec![Answer::Json(answer(&[0, 1, 2], &[2, 0, 1], Some(8)))]).await;
    let catalog = catalog(address, "");
    let embeddings = embedder(&catalog)
        .embed(&request(3), &Cancel::new())
        .await
        .unwrap();
    assert_eq!(embeddings.vectors, vec![vector(0), vector(1), vector(2)]);
    assert_eq!(embeddings.usage.input_tokens, Some(8));
    assert_eq!(embeddings.usage.output_tokens, None);
    assert_eq!(
        embeddings.upstream_model.as_ref().map(Id::as_str),
        Some("text-embedding-3-small")
    );
    assert_eq!(
        seen.bodies(),
        [json!({
            "model": "text-embedding-3-small",
            "input": texts(3),
            "encoding_format": "float"
        })]
    );
    assert_eq!(*seen.paths.lock().unwrap(), ["/v1/embeddings".to_owned()]);
}

#[tokio::test]
async fn a_request_over_the_batch_limit_is_split_and_returned_in_input_order() {
    let (address, seen) = serve(vec![
        Answer::Json(answer(&[0, 1], &[1, 0], Some(4))),
        Answer::Json(answer(&[2, 3], &[0, 1], Some(5))),
        Answer::Json(answer(&[4], &[0], Some(2))),
    ])
    .await;
    let catalog = catalog(address, "embed_batch_limit = 2");
    let embeddings = embedder(&catalog)
        .embed(&request(5), &Cancel::new())
        .await
        .unwrap();
    assert_eq!(embeddings.vectors, (0..5).map(vector).collect::<Vec<_>>());
    assert_eq!(embeddings.usage.input_tokens, Some(11));
    let inputs: Vec<Value> = seen
        .bodies()
        .iter()
        .map(|body| body["input"].clone())
        .collect();
    let all = texts(5);
    assert_eq!(
        inputs,
        [json!(all[0..2]), json!(all[2..4]), json!(all[4..5])]
    );
}

#[tokio::test]
async fn an_unreported_token_count_stays_unknown() {
    let (address, _) = serve(vec![Answer::Json(answer(&[0, 1], &[0, 1], None))]).await;
    let catalog = catalog(address, "");
    let embeddings = embedder(&catalog)
        .embed(&request(2), &Cancel::new())
        .await
        .unwrap();
    assert_eq!(embeddings.usage.input_tokens, None);

    // One batch reporting and one not: the request's count is unknown, never the known part.
    let (address, _) = serve(vec![
        Answer::Json(answer(&[0], &[0], Some(3))),
        Answer::Json(answer(&[1], &[0], None)),
    ])
    .await;
    let catalog = crate::catalog(address, "embed_batch_limit = 1");
    let log = CallLog::new();
    let clock = Frozen(1_762_430_400_000);
    let embeddings = embedder(&catalog)
        .embed_recorded(
            &request(2),
            &Cancel::new(),
            CallIdentity::primary(id("req-1")),
            recording(&log, &clock, None),
        )
        .await
        .unwrap();
    assert_eq!(embeddings.usage.input_tokens, None);
    let records = log.records();
    assert_eq!(records[0].usage.input_tokens, Some(3));
    assert_eq!(records[1].usage.input_tokens, None);
}

#[tokio::test]
async fn an_embed_request_past_its_timeout_fails_deadline() {
    let (address, seen) = serve(vec![Answer::Hang]).await;
    let catalog = catalog(address, "");
    let log = CallLog::new();
    let clock = Frozen(1_762_430_400_000);
    let mut slow = request(2);
    slow.timeout = Duration::from_millis(300);
    let started = Instant::now();
    let error = embedder(&catalog)
        .embed_recorded(
            &slow,
            &Cancel::new(),
            CallIdentity::primary(id("req-1")),
            recording(&log, &clock, None),
        )
        .await
        .unwrap_err();
    let took = started.elapsed();
    assert_eq!(error.code, ErrorCode::Deadline);
    assert_eq!(error.dispatch, Dispatch::Unknown);
    assert!(!error.retriable);
    assert!(error.message.contains("timeout"), "{}", error.message);
    assert!(took < Duration::from_secs(3), "returned after {took:?}");
    assert_eq!(seen.connections.load(Ordering::SeqCst), 1);
    let records = log.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].outcome, Outcome::Failed);
    assert_eq!(records[0].error_code, Some(ErrorCode::Deadline));
    assert_eq!(records[0].dispatch, Some(Dispatch::Unknown));
}

#[tokio::test]
async fn an_embed_request_to_a_turn_route_is_refused_before_io() {
    let (address, seen) = serve(vec![Answer::Json(answer(&[0], &[0], Some(1)))]).await;
    let catalog = catalog(address, "");
    let error = OpenAiEmbedder::for_route(&catalog, "chat", http(), Arc::new(AnonymousResolver))
        .map(|_| ())
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(error.dispatch, Dispatch::NotSent);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(seen.connections.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn each_batch_yields_a_call_record_of_kind_embed_batch() {
    let (address, _) = serve(vec![
        Answer::Json(answer(&[0, 1], &[0, 1], Some(4))),
        Answer::Json(answer(&[2], &[0], Some(2))),
    ])
    .await;
    let catalog = catalog(address, "embed_batch_limit = 2");
    let log = CallLog::new();
    let clock = Frozen(1_762_430_400_000);
    embedder(&catalog)
        .embed_recorded(
            &request(3),
            &Cancel::new(),
            CallIdentity::primary(id("req-1")),
            recording(&log, &clock, None),
        )
        .await
        .unwrap();
    let records = log.records();
    let expected = |position: &str, tokens: u64| CallRecord {
        id: id(&format!("req-1/{position}")),
        role: Role::Primary,
        kind: Kind::EmbedBatch,
        route_id: id("embedding"),
        route_target_id: Some(id("embed-primary")),
        upstream_model: Some(id("text-embedding-3-small")),
        outcome: Outcome::Succeeded,
        error_code: None,
        dispatch: None,
        attempts: 1,
        started_at_unix_ms: 1_762_430_400_000,
        latency_ms: Some(0),
        time_to_first_event_ms: None,
        usage: llm_core::Usage {
            input_tokens: Some(tokens),
            ..llm_core::Usage::default()
        },
        cost: None,
        primary_record_id: None,
    };
    assert_eq!(records, [expected("0", 4), expected("1", 2)]);
    let serialized = serde_json::to_string(&records).unwrap();
    assert!(!serialized.contains(TEXT_MARKER), "{serialized}");
}

#[tokio::test]
async fn a_priced_batch_record_carries_the_cost_of_its_input_tokens() {
    let (address, _) = serve(vec![Answer::Json(answer(&[0], &[0], Some(1000)))]).await;
    let catalog = catalog(address, "");
    let binding = catalog.binding(&id("embedder")).unwrap();
    let prices = PriceBook::from_document(PriceDocument::new(
        id("prices-r1"),
        "fixture price list",
        1,
        Currency::new("USD").unwrap(),
        vec![ModelPrice {
            id: id("price-embed"),
            serving_model_id: id("embedder"),
            binding_revision: binding.provenance().binding_revision.clone(),
            upstream_model: id("text-embedding-3-small"),
            input: Some(Rate {
                amount: Amount::parse("0.02").unwrap(),
                per_units: 1000,
            }),
            cached_input: None,
            cache_creation_input: None,
            output: None,
        }],
    ))
    .unwrap();
    let log = CallLog::new();
    let clock = Frozen(0);
    embedder(&catalog)
        .embed_recorded(
            &request(1),
            &Cancel::new(),
            CallIdentity::primary(id("req-1")),
            recording(&log, &clock, Some(&prices)),
        )
        .await
        .unwrap();
    let record = log.records()[0].clone();
    // Only the reported count is recorded; output and cache classes are priced as the zeros the
    // embeddings wire has, never written as reported zeros.
    assert_eq!(
        record.usage,
        llm_core::Usage {
            input_tokens: Some(1000),
            ..llm_core::Usage::default()
        }
    );
    let cost = record.cost.unwrap();
    assert!(cost.complete);
    assert_eq!(cost.known_subtotal, Amount::parse("0.02").unwrap());
}

#[tokio::test]
async fn an_answer_whose_indexes_or_count_do_not_match_its_batch_is_refused() {
    let item =
        |index: Value| json!({"object": "embedding", "embedding": [0.5, 0.5, 0.5], "index": index});
    let list = |data: Vec<Value>| {
        json!({"object": "list", "data": data, "model": "text-embedding-3-small",
               "usage": {"prompt_tokens": 2, "total_tokens": 2}})
    };
    for (case, body) in [
        ("one item short", list(vec![item(json!(0))])),
        (
            "one item over",
            list(vec![item(json!(0)), item(json!(1)), item(json!(2))]),
        ),
        (
            "a repeated index",
            list(vec![item(json!(0)), item(json!(0))]),
        ),
        (
            "an index past the batch",
            list(vec![item(json!(0)), item(json!(2))]),
        ),
        (
            "a negative index",
            list(vec![item(json!(0)), item(json!(-1))]),
        ),
        (
            "an index that is no integer",
            list(vec![item(json!(0)), item(json!("1"))]),
        ),
        (
            "no index",
            list(vec![
                item(json!(0)),
                json!({"object": "embedding", "embedding": [0.5, 0.5, 0.5]}),
            ]),
        ),
        (
            "an empty vector",
            list(vec![
                item(json!(0)),
                json!({"object": "embedding", "embedding": [], "index": 1}),
            ]),
        ),
        (
            "vectors of two lengths",
            list(vec![
                item(json!(0)),
                json!({"object": "embedding", "embedding": [0.5], "index": 1}),
            ]),
        ),
        (
            "a base64 vector",
            list(vec![
                item(json!(0)),
                json!({"object": "embedding", "embedding": "AAAA", "index": 1}),
            ]),
        ),
        ("no data", json!({"object": "list", "model": "m"})),
    ] {
        let (address, _) = serve(vec![Answer::Json(body)]).await;
        let catalog = catalog(address, "");
        let log = CallLog::new();
        let clock = Frozen(0);
        let error = embedder(&catalog)
            .embed_recorded(
                &request(2),
                &Cancel::new(),
                CallIdentity::primary(id("req-1")),
                recording(&log, &clock, None),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Protocol, "{case}");
        assert_eq!(error.dispatch, Dispatch::Accepted, "{case}");
        assert!(!error.retriable, "{case}");
        let records = log.records();
        assert_eq!(records.len(), 1, "{case}");
        assert_eq!(records[0].outcome, Outcome::Failed, "{case}");
        assert_eq!(records[0].error_code, Some(ErrorCode::Protocol), "{case}");
    }
}

#[tokio::test]
async fn batches_answered_with_vectors_of_two_lengths_are_refused() {
    let (address, _) = serve(vec![
        Answer::Json(answer(&[0], &[0], Some(1))),
        Answer::Json(json!({"object": "list", "data": [{"object": "embedding", "embedding": [0.5], "index": 0}],
                            "model": "text-embedding-3-small"})),
    ])
    .await;
    let catalog = catalog(address, "embed_batch_limit = 1");
    let error = embedder(&catalog)
        .embed(&request(2), &Cancel::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Protocol);
    assert_eq!(error.dispatch, Dispatch::Accepted);
}

#[tokio::test]
async fn asked_dimensions_are_sent_and_a_vector_of_another_length_is_refused() {
    let (address, seen) = serve(vec![Answer::Json(answer(&[0], &[0], Some(1)))]).await;
    let catalog = catalog(address, "");
    let mut asked = request(1);
    asked.dimensions = Some(3);
    embedder(&catalog)
        .embed(&asked, &Cancel::new())
        .await
        .unwrap();
    assert_eq!(seen.bodies()[0]["dimensions"], json!(3));

    let (address, _) = serve(vec![Answer::Json(answer(&[0], &[0], Some(1)))]).await;
    let catalog = crate::catalog(address, "");
    asked.dimensions = Some(256);
    let error = embedder(&catalog)
        .embed(&asked, &Cancel::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Protocol);
}

#[tokio::test]
async fn a_failed_batch_stops_the_request_and_keeps_its_retry_class() {
    let (address, seen) = serve(vec![
        Answer::Json(answer(&[0], &[0], Some(1))),
        Answer::Status(429),
    ])
    .await;
    let catalog = catalog(address, "embed_batch_limit = 1");
    let log = CallLog::new();
    let clock = Frozen(0);
    let error = embedder(&catalog)
        .embed_recorded(
            &request(3),
            &Cancel::new(),
            CallIdentity::primary(id("req-1")),
            recording(&log, &clock, None),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::RateLimited);
    assert_eq!(error.dispatch, Dispatch::Rejected);
    assert!(error.retriable);
    // The third batch is never sent.
    assert_eq!(seen.bodies().len(), 2);
    let outcomes: Vec<Outcome> = log.records().iter().map(|record| record.outcome).collect();
    assert_eq!(outcomes, [Outcome::Succeeded, Outcome::Failed]);
}

#[tokio::test]
async fn a_cancelled_request_records_its_batch_cancelled() {
    let (address, _) = serve(vec![Answer::Hang]).await;
    let catalog = catalog(address, "");
    let log = CallLog::new();
    let clock = Frozen(0);
    let cancel = Cancel::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        trigger.cancel();
    });
    let error = embedder(&catalog)
        .embed_recorded(
            &request(1),
            &cancel,
            CallIdentity::primary(id("req-1")),
            recording(&log, &clock, None),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_eq!(log.records()[0].outcome, Outcome::Cancelled);
}

#[tokio::test]
async fn invalid_requests_are_refused_before_io_and_write_no_record() {
    let (address, seen) = serve(vec![Answer::Json(answer(&[0], &[0], Some(1)))]).await;
    let catalog = catalog(address, "");
    let mut empty_text = request(2);
    empty_text.texts[1] = String::new();
    let mut zero_dimensions = request(1);
    zero_dimensions.dimensions = Some(0);
    let mut no_time = request(1);
    no_time.timeout = Duration::ZERO;
    for (refused, code) in [
        (
            EmbedRequest::new(Vec::new(), Duration::from_secs(1)),
            ErrorCode::InvalidRequest,
        ),
        (empty_text, ErrorCode::InvalidRequest),
        (zero_dimensions, ErrorCode::InvalidRequest),
        (no_time, ErrorCode::InvalidRequest),
        (request(llm_core::MAX_EMBED_TEXTS + 1), ErrorCode::TooLarge),
    ] {
        let log = CallLog::new();
        let clock = Frozen(0);
        let error = embedder(&catalog)
            .embed_recorded(
                &refused,
                &Cancel::new(),
                CallIdentity::primary(id("req-1")),
                recording(&log, &clock, None),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, code);
        assert_eq!(error.dispatch, Dispatch::NotSent);
        assert!(!error.message.contains(TEXT_MARKER));
        assert!(log.records().is_empty());
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(seen.connections.load(Ordering::SeqCst), 0);
}

/// Hands out one fixed bearer token.
struct Fixed;

impl SecretResolver for Fixed {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async {
            Ok(ResolvedSecret {
                secret: Secret::new(b"fixture-token".to_vec())?,
                version: SecretVersion::new("v1".to_owned())?,
            })
        })
    }
}

#[tokio::test]
async fn the_injected_credential_is_sent_as_the_account_declares() {
    let (address, seen) = serve(vec![Answer::Json(answer(&[0], &[0], Some(1)))]).await;
    let catalog = Catalog::parse(&catalog_toml(
        address,
        "",
        "auth_kind = \"bearer\"\nbilling_kind = \"metered\"\nsecret_reference_id = \"embed-token\"",
    ))
    .unwrap();
    OpenAiEmbedder::for_route(&catalog, "embed", http(), Arc::new(Fixed))
        .unwrap()
        .embed(&request(1), &Cancel::new())
        .await
        .unwrap();
    assert_eq!(
        *seen.authorizations.lock().unwrap(),
        [Some("Bearer fixture-token".to_owned())]
    );
}
