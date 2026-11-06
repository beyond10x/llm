//! Adversary cases for the embed route kind and the `OpenAI` embeddings wire, against a loopback
//! fixture server: no live call.

use llm_calls::{CallIdentity, CallLog, Clock, Outcome, Recording};
use llm_core::{BoxFuture, Cancel, Dispatch, EmbedRequest, Embedder, ErrorCode, Id};
use llm_cost::{Amount, Currency, ModelPrice, PriceBook, PriceDocument, Rate};
use llm_credentials::{AnonymousResolver, ResolvedSecret, SecretError, SecretRef, SecretResolver};
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
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const TEXT_MARKER: &str = "adversary-text-okapi-9031";

#[derive(Clone)]
enum Answer {
    Json(Value),
    Status(u16, Value),
    Hang,
}

#[derive(Default)]
struct Seen {
    connections: AtomicUsize,
    bodies: Mutex<Vec<Value>>,
}

impl Seen {
    fn inputs(&self) -> Vec<usize> {
        self.bodies
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|body| body["input"].as_array().map_or(0, Vec::len))
            .collect()
    }
}

async fn read_request(socket: &mut TcpStream) -> Option<Vec<u8>> {
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
    let length: usize = head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse().ok())?
    })?;
    let mut body = bytes[end + 4..].to_vec();
    while body.len() < length {
        let read = socket.read(&mut buffer).await.ok()?;
        if read == 0 {
            return None;
        }
        body.extend_from_slice(&buffer[..read]);
    }
    Some(body)
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
            let Some(body) = read_request(&mut socket).await else {
                continue;
            };
            shared
                .bodies
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(serde_json::from_slice(&body).unwrap_or(Value::Null));
            let answer = answers[served.min(answers.len() - 1)].clone();
            served += 1;
            let (status, body) = match answer {
                Answer::Json(value) => (200, serde_json::to_vec(&value).unwrap()),
                Answer::Status(status, value) => (status, serde_json::to_vec(&value).unwrap()),
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

[[routes]]
id = "embedding"
alias = "embed"
kind = "embed"

[[targets]]
id = "embed-primary"
route_id = "embedding"
serving_model_id = "embedder"
position = 0
"#
    )
}

const ANONYMOUS: &str = "auth_kind = \"anonymous\"\nbilling_kind = \"self-hosted\"";
const BEARER: &str =
    "auth_kind = \"bearer\"\nbilling_kind = \"metered\"\nsecret_reference_id = \"embed-token\"";

fn catalog(address: SocketAddr, capabilities: &str) -> Catalog {
    Catalog::parse(&catalog_toml(address, capabilities, ANONYMOUS)).unwrap()
}

fn client(headers: Duration) -> HttpClient {
    HttpClient::new(Limits {
        response_headers: headers,
        idle: Duration::from_secs(20),
        total: Duration::from_secs(20),
    })
    .unwrap()
}

fn embedder(catalog: &Catalog) -> OpenAiEmbedder {
    OpenAiEmbedder::for_route(
        catalog,
        "embed",
        client(Duration::from_secs(20)),
        Arc::new(AnonymousResolver),
    )
    .unwrap()
}

fn request(count: usize) -> EmbedRequest {
    EmbedRequest::new(
        (0..count).map(|i| format!("{TEXT_MARKER} {i}")).collect(),
        Duration::from_secs(10),
    )
}

/// An answer for `count` texts in batch order, each vector led by `first + position`.
fn answer(first: usize, count: usize, model: &str, usage: Option<u64>) -> Value {
    let data: Vec<Value> = (0..count)
        .map(|index| {
            let lead = f64::from(u32::try_from(first + index).unwrap());
            json!({"object": "embedding", "embedding": [lead, 0.5, 0.5], "index": index})
        })
        .collect();
    let mut body = json!({"object": "list", "data": data, "model": model});
    if let Some(tokens) = usage {
        body["usage"] = json!({"prompt_tokens": tokens, "total_tokens": tokens});
    }
    body
}

struct Frozen;

impl Clock for Frozen {
    fn unix_ms(&self) -> u64 {
        1_762_430_400_000
    }
    fn monotonic(&self) -> Duration {
        Duration::ZERO
    }
}

fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}

fn prices(catalog: &Catalog) -> PriceBook {
    let binding = catalog.binding(&id("embedder")).unwrap();
    PriceBook::from_document(PriceDocument::new(
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
    .unwrap()
}

/// The record id is `<request id>/<batch position>`. A request id at the `Id` bound (256 bytes,
/// valid) leaves no room for the suffix, and every batch must still get its own record id.
#[tokio::test]
async fn a_request_id_at_its_bound_still_gives_each_batch_its_own_record_id() {
    let (address, _) = serve(vec![
        Answer::Json(answer(0, 1, "text-embedding-3-small", Some(1))),
        Answer::Json(answer(1, 1, "text-embedding-3-small", Some(1))),
    ])
    .await;
    let catalog = catalog(address, "embed_batch_limit = 1");
    let long = "r".repeat(256);
    let log = CallLog::new();
    embedder(&catalog)
        .embed_recorded(
            &request(2),
            &Cancel::new(),
            CallIdentity::primary(id(&long)),
            Recording {
                sink: &log,
                clock: &Frozen,
                prices: None,
            },
        )
        .await
        .unwrap();
    let ids: Vec<String> = log
        .records()
        .iter()
        .map(|record| record.id.as_str().to_owned())
        .collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(
        ids[0], ids[1],
        "two batches of one request share one record id"
    );
}

#[tokio::test]
async fn an_unreported_count_under_a_price_book_leaves_the_cost_incomplete() {
    let (address, _) = serve(vec![Answer::Json(answer(
        0,
        2,
        "text-embedding-3-small",
        None,
    ))])
    .await;
    let catalog = catalog(address, "");
    let book = prices(&catalog);
    let log = CallLog::new();
    let embeddings = embedder(&catalog)
        .embed_recorded(
            &request(2),
            &Cancel::new(),
            CallIdentity::primary(id("req-1")),
            Recording {
                sink: &log,
                clock: &Frozen,
                prices: Some(&book),
            },
        )
        .await
        .unwrap();
    assert_eq!(embeddings.usage.input_tokens, None);
    let record = log.records()[0].clone();
    assert_eq!(record.usage.input_tokens, None);
    let cost = record
        .cost
        .expect("a sent batch under a price book is priced");
    assert!(!cost.complete, "an unknown count priced as complete");
}

#[tokio::test]
async fn a_batch_past_the_timeout_on_the_wire_is_priced_incomplete() {
    let (address, _) = serve(vec![Answer::Hang]).await;
    let catalog = catalog(address, "");
    let book = prices(&catalog);
    let log = CallLog::new();
    let mut slow = request(1);
    slow.timeout = Duration::from_millis(300);
    let error = embedder(&catalog)
        .embed_recorded(
            &slow,
            &Cancel::new(),
            CallIdentity::primary(id("req-1")),
            Recording {
                sink: &log,
                clock: &Frozen,
                prices: Some(&book),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Deadline);
    let record = log.records()[0].clone();
    assert_eq!(record.dispatch, Some(Dispatch::Unknown));
    let cost = record
        .cost
        .expect("a batch that may have been billed is priced");
    assert!(!cost.complete);
}

/// Hangs every resolution.
struct Stuck;

impl SecretResolver for Stuck {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(std::future::pending())
    }
}

#[tokio::test]
async fn a_credential_outlasting_the_timeout_ends_deadline_not_sent_and_unpriced() {
    let (address, seen) = serve(vec![Answer::Json(answer(
        0,
        1,
        "text-embedding-3-small",
        Some(1),
    ))])
    .await;
    let catalog = Catalog::parse(&catalog_toml(address, "", BEARER)).unwrap();
    let book = prices(&catalog);
    let embedder = OpenAiEmbedder::for_route(
        &catalog,
        "embed",
        client(Duration::from_secs(20)),
        Arc::new(Stuck),
    )
    .unwrap();
    let log = CallLog::new();
    let mut slow = request(1);
    slow.timeout = Duration::from_millis(300);
    let error = embedder
        .embed_recorded(
            &slow,
            &Cancel::new(),
            CallIdentity::primary(id("req-1")),
            Recording {
                sink: &log,
                clock: &Frozen,
                prices: Some(&book),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Deadline);
    assert_eq!(error.dispatch, Dispatch::NotSent);
    assert_eq!(error.message, "the embed request passed its timeout");
    let records = log.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].dispatch, Some(Dispatch::NotSent));
    assert_eq!(records[0].cost, None);
    assert_eq!(seen.connections.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn the_transport_deadline_is_told_apart_from_the_callers_timeout() {
    let (address, _) = serve(vec![Answer::Hang]).await;
    let catalog = catalog(address, "");
    let quick_transport = OpenAiEmbedder::for_route(
        &catalog,
        "embed",
        client(Duration::from_millis(200)),
        Arc::new(AnonymousResolver),
    )
    .unwrap();
    let transport = quick_transport
        .embed(&request(1), &Cancel::new())
        .await
        .unwrap_err();
    assert_eq!(transport.code, ErrorCode::Deadline);
    assert_ne!(transport.message, "the embed request passed its timeout");

    let mut slow = request(1);
    slow.timeout = Duration::from_millis(200);
    let caller = embedder(&catalog)
        .embed(&slow, &Cancel::new())
        .await
        .unwrap_err();
    assert_eq!(caller.code, ErrorCode::Deadline);
    assert_eq!(caller.message, "the embed request passed its timeout");
}

#[tokio::test]
async fn the_timeout_spans_batches_and_records_each_one_sent() {
    let (address, seen) = serve(vec![
        Answer::Json(answer(0, 1, "text-embedding-3-small", Some(1))),
        Answer::Hang,
    ])
    .await;
    let catalog = catalog(address, "embed_batch_limit = 1");
    let log = CallLog::new();
    let mut slow = request(3);
    slow.timeout = Duration::from_millis(400);
    let error = embedder(&catalog)
        .embed_recorded(
            &slow,
            &Cancel::new(),
            CallIdentity::primary(id("req-1")),
            Recording {
                sink: &log,
                clock: &Frozen,
                prices: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Deadline);
    assert_eq!(seen.inputs(), [1, 1]);
    let outcomes: Vec<(Outcome, Option<ErrorCode>)> = log
        .records()
        .iter()
        .map(|record| (record.outcome, record.error_code))
        .collect();
    assert_eq!(
        outcomes,
        [
            (Outcome::Succeeded, None),
            (Outcome::Failed, Some(ErrorCode::Deadline))
        ]
    );
}

#[tokio::test]
async fn the_batch_limit_splits_exactly() {
    for (limit, count, expected) in [
        (3_u32, 7_usize, vec![3_usize, 3, 1]),
        (3, 6, vec![3, 3]),
        (4, 4, vec![4]),
        (9, 2, vec![2]),
        (1, 3, vec![1, 1, 1]),
    ] {
        let answers = expected
            .iter()
            .scan(0, |first, &size| {
                let body = answer(*first, size, "text-embedding-3-small", Some(1));
                *first += size;
                Some(Answer::Json(body))
            })
            .collect();
        let (address, seen) = serve(answers).await;
        let catalog = catalog(address, &format!("embed_batch_limit = {limit}"));
        let embeddings = embedder(&catalog)
            .embed(&request(count), &Cancel::new())
            .await
            .unwrap();
        assert_eq!(seen.inputs(), expected, "limit {limit}, {count} texts");
        let leads: Vec<f32> = embeddings.vectors.iter().map(|vector| vector[0]).collect();
        let wanted: Vec<f32> = (0..count)
            .map(|i| f32::from(u16::try_from(i).unwrap()))
            .collect();
        assert_eq!(leads, wanted);
        assert_eq!(
            embeddings.usage.input_tokens,
            Some(u64::try_from(expected.len()).unwrap())
        );
    }
}

#[tokio::test]
async fn batches_answered_by_two_models_are_refused() {
    let (address, seen) = serve(vec![
        Answer::Json(answer(0, 1, "text-embedding-3-small", Some(1))),
        Answer::Json(answer(1, 1, "text-embedding-3-large", Some(1))),
    ])
    .await;
    let catalog = catalog(address, "embed_batch_limit = 1");
    let log = CallLog::new();
    let error = embedder(&catalog)
        .embed_recorded(
            &request(3),
            &Cancel::new(),
            CallIdentity::primary(id("req-1")),
            Recording {
                sink: &log,
                clock: &Frozen,
                prices: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Protocol);
    assert_eq!(error.dispatch, Dispatch::Accepted);
    assert_eq!(seen.inputs(), [1, 1]);
    let outcomes: Vec<Outcome> = log.records().iter().map(|record| record.outcome).collect();
    assert_eq!(outcomes, [Outcome::Succeeded, Outcome::Failed]);
}

#[tokio::test]
async fn a_component_past_f32_is_refused_not_carried_as_infinity() {
    let body = json!({"object": "list", "model": "text-embedding-3-small",
        "data": [{"object": "embedding", "embedding": [1.0e39, 0.5, 0.5], "index": 0}]});
    let (address, _) = serve(vec![Answer::Json(body)]).await;
    let catalog = catalog(address, "");
    let error = embedder(&catalog)
        .embed(&request(1), &Cancel::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Protocol);
}

#[tokio::test]
async fn no_text_reaches_an_error_message_or_a_record_when_the_far_side_echoes_it() {
    let echo = json!({"error": {"message": format!("bad input: {TEXT_MARKER} 0")}});
    for status in [400_u16, 429, 500] {
        let (address, _) = serve(vec![Answer::Status(status, echo.clone())]).await;
        let catalog = catalog(address, "");
        let log = CallLog::new();
        let error = embedder(&catalog)
            .embed_recorded(
                &request(1),
                &Cancel::new(),
                CallIdentity::primary(id("req-1")),
                Recording {
                    sink: &log,
                    clock: &Frozen,
                    prices: None,
                },
            )
            .await
            .unwrap_err();
        assert!(!error.message.contains(TEXT_MARKER), "{status}");
        assert!(!format!("{error:?}").contains(TEXT_MARKER), "{status}");
        let records = serde_json::to_string(&log.records()).unwrap();
        assert!(!records.contains(TEXT_MARKER), "{status}");
        assert_eq!(log.records().len(), 1);
    }
}
