//! Embedding conformance observations against a scripted loopback server.
//!
//! This module parses the authored catalog, puts the fixture server's address in place of
//! `http://fixture.invalid`, and runs the real path: `llm_embed::OpenAiEmbedder` for an embed
//! request, or `llm_calls::record_turn` over `Catalog::run_turn` with every port
//! `llm_models::CatalogModels` builds for a turn. The server only replays the authored answers in
//! order and keeps the request bodies it received. Records go to an in-memory sink under a clock
//! that never moves. Every vector, count, refusal and record comes back out of the library; it
//! reads no suite and branches on no scenario name. Nothing leaves the host.

use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use ess_conformance::target::TargetError;
use llm_calls::{CallIdentity, CallLog, Clock, Recording, record_turn};
use llm_core::{Cancel, EmbedRequest, Error, Id, StreamSink, TurnDocument, VecSink};
use llm_credentials::{AnonymousResolver, SecretResolver};
use llm_embed::OpenAiEmbedder;
use llm_http::{HttpClient, Limits};
use llm_models::CatalogModels;
use llm_routing::{Catalog, FallbackPolicy, Ports};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use crate::target::Observed;

/// Every view name this domain answers through `query_view`.
pub const VIEWS: &[&str] = &["llm.embeddings.LastEmbed"];

const MAX_PROGRAM_BYTES: usize = 64 * 1024;
const MAX_ANSWERS: usize = 64;
/// The largest request the scripted server reads.
const MAX_REQUEST_BYTES: usize = 1024 * 1024;
/// The placeholder origin the authored catalog names for the fixture server.
const FIXTURE_ORIGIN: &str = "http://fixture.invalid";
/// How long the adapter waits for a request that would arrive after the call returned.
const LATE_GRACE: Duration = Duration::from_millis(50);
/// A call that returns within its timeout and this much more ended within it.
const TIMEOUT_SLACK: Duration = Duration::from_secs(2);
/// A backstop so a fixture can never hang the gate. Far above every authored bound.
const BACKSTOP: Duration = Duration::from_secs(30);

/// Observe one command of this domain, or `None` when the command belongs to another.
pub fn observe(command: &str, input: &Value) -> Option<Result<Observed, TargetError>> {
    match command {
        "llm.embeddings.Embed" => Some(embed(input)),
        _ => None,
    }
}

fn unavailable(error: impl std::fmt::Display) -> TargetError {
    TargetError::unavailable("embedding observation", error.to_string())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmbedInput {
    catalog_toml: String,
    request_json: String,
    program_json: String,
}

/// An embed request (`alias`, `texts`, `timeout_ms`, `dimensions`) or a routed turn (`turn`,
/// whose model is the alias, with the caller's `input_tokens` estimate).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestInput {
    request_id: String,
    #[serde(default)]
    alias: Option<String>,
    #[serde(default)]
    texts: Vec<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    dimensions: Option<u32>,
    #[serde(default)]
    turn: Option<TurnDocument>,
    #[serde(default)]
    input_tokens: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Program {
    #[serde(default)]
    start_unix_ms: u64,
    answers: Vec<Answer>,
}

/// One answer of the fixture server, in request order; the last one repeats.
#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct Answer {
    #[serde(default = "ok")]
    status: u16,
    #[serde(default)]
    body: Option<Value>,
    /// Takes the request and never answers.
    #[serde(default)]
    hang: bool,
}

const fn ok() -> u16 {
    200
}

fn embed(input: &Value) -> Result<Observed, TargetError> {
    let input: EmbedInput = serde_json::from_value(input.clone()).map_err(unavailable)?;
    Ok(Observed {
        facts: run(&input),
        view: "llm.embeddings.LastEmbed",
        event: "llm.embeddings.Embedded",
        field: "valid_program",
    })
}

fn blank() -> Value {
    json!({
        "valid_program": false, "error_code": null, "config_digest": null, "vectors": 0,
        "dimensions": [], "leading": [], "batch_texts": [], "input_tokens": null,
        "upstream_model": null, "request_bodies": [], "ended_within": null, "records": []
    })
}

fn label(value: impl serde::Serialize) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// A clock that never moves: every record starts at the program's instant and takes no time.
struct Frozen(u64);

impl Clock for Frozen {
    fn unix_ms(&self) -> u64 {
        self.0
    }
    fn monotonic(&self) -> Duration {
        Duration::ZERO
    }
}

fn run(input: &EmbedInput) -> Value {
    let mut facts = blank();
    if input.program_json.len() > MAX_PROGRAM_BYTES || input.request_json.len() > MAX_PROGRAM_BYTES
    {
        return facts;
    }
    let (Ok(request), Ok(program)) = (
        serde_json::from_str::<RequestInput>(&input.request_json),
        serde_json::from_str::<Program>(&input.program_json),
    ) else {
        return facts;
    };
    if program.answers.is_empty() || program.answers.len() > MAX_ANSWERS {
        return facts;
    }
    let Ok(record_id) = Id::new(&request.request_id) else {
        return facts;
    };
    facts["valid_program"] = json!(true);
    match Catalog::parse(&input.catalog_toml) {
        Ok(catalog) => facts["config_digest"] = json!(catalog.digest()),
        Err(error) => {
            facts["error_code"] = label(error.code);
            return facts;
        }
    }
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        facts["error_code"] = json!("fixture:no-runtime");
        return facts;
    };
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let log = CallLog::new();
    let clock = Frozen(program.start_unix_ms);
    let outcome = runtime.block_on(async {
        let outcome = tokio::time::timeout(
            BACKSTOP,
            exercise(
                input,
                request,
                program.answers,
                record_id,
                &bodies,
                (&log, &clock),
                &mut facts,
            ),
        )
        .await;
        tokio::time::sleep(LATE_GRACE).await;
        outcome
    });
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(code)) => facts["error_code"] = json!(code),
        Err(_) => facts["error_code"] = json!("fixture:backstop"),
    }
    drop(runtime);
    let bodies = bodies
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    facts["batch_texts"] = json!(
        bodies
            .iter()
            .map(|body| serde_json::from_str::<Value>(body)
                .ok()
                .and_then(|value| value["input"].as_array().map(Vec::len))
                .unwrap_or(0))
            .collect::<Vec<_>>()
    );
    facts["request_bodies"] = json!(bodies);
    facts["records"] = json!(
        log.records()
            .iter()
            .filter_map(|record| serde_json::to_string(record).ok())
            .collect::<Vec<_>>()
    );
    facts
}

/// Runs the request against the fixture and writes what the library returned into `facts`.
async fn exercise(
    input: &EmbedInput,
    request: RequestInput,
    answers: Vec<Answer>,
    record_id: Id,
    bodies: &Arc<Mutex<Vec<String>>>,
    (log, clock): (&CallLog, &Frozen),
    facts: &mut Value,
) -> Result<(), &'static str> {
    let address = serve(answers, bodies.clone()).await?;
    let source = input
        .catalog_toml
        .replace(FIXTURE_ORIGIN, &format!("http://{address}"));
    let catalog = match Catalog::parse(&source) {
        Ok(catalog) => catalog,
        Err(error) => {
            facts["error_code"] = label(error.code);
            return Ok(());
        }
    };
    let http = HttpClient::new(Limits {
        response_headers: Duration::from_secs(20),
        idle: Duration::from_secs(20),
        total: Duration::from_secs(20),
    })
    .map_err(|_| "fixture:client")?;
    let recording = Recording {
        sink: log,
        clock,
        prices: None,
    };
    let identity = CallIdentity::primary(record_id);
    if let Some(turn) = request.turn {
        if let Err(error) = turn_through(
            &catalog,
            &http,
            &turn,
            request.input_tokens,
            identity,
            recording,
        )
        .await
        {
            facts["error_code"] = label(error.code);
        }
        return Ok(());
    }
    let alias = request.alias.as_deref().unwrap_or_default();
    let embedder =
        match OpenAiEmbedder::for_route(&catalog, alias, http, Arc::new(AnonymousResolver)) {
            Ok(embedder) => embedder,
            Err(error) => {
                facts["error_code"] = label(error.code);
                return Ok(());
            }
        };
    let timeout = Duration::from_millis(request.timeout_ms.unwrap_or(10_000));
    let mut embed = EmbedRequest::new(request.texts, timeout);
    embed.dimensions = request.dimensions;
    let started = Instant::now();
    let result = embedder
        .embed_recorded(&embed, &Cancel::new(), identity, recording)
        .await;
    facts["ended_within"] = json!(started.elapsed() < timeout + TIMEOUT_SLACK);
    match result {
        Ok(embeddings) => {
            facts["vectors"] = json!(embeddings.vectors.len());
            facts["dimensions"] =
                json!(embeddings.vectors.iter().map(Vec::len).collect::<Vec<_>>());
            facts["leading"] = json!(
                embeddings
                    .vectors
                    .iter()
                    .map(|vector| vector.first().map(ToString::to_string))
                    .collect::<Vec<_>>()
            );
            facts["input_tokens"] = json!(embeddings.usage.input_tokens);
            facts["upstream_model"] = json!(embeddings.upstream_model);
        }
        Err(error) => facts["error_code"] = label(error.code),
    }
    Ok(())
}

/// A routed, recorded turn over the ports the catalog declares, as a caller would run one.
async fn turn_through(
    catalog: &Catalog,
    http: &HttpClient,
    turn: &TurnDocument,
    input_tokens: Option<u64>,
    identity: CallIdentity,
    recording: Recording<'_>,
) -> Result<(), Error> {
    let resolvers: BTreeMap<Id, Arc<dyn SecretResolver>> = BTreeMap::new();
    let models = CatalogModels::build(catalog, http, &resolvers)?;
    let mut sink = VecSink::new(1024, 1024 * 1024);
    let mut admit = |_: &llm_routing::Selection<'_>| Ok(());
    let cancel = Cancel::new();
    record_turn(
        catalog,
        &turn.request,
        input_tokens,
        FallbackPolicy::default(),
        Ports {
            models: &models,
            admit: &mut admit,
            sink: &mut sink as &mut dyn StreamSink,
            cancel: &cancel,
            pause: &|wait| Box::pin(tokio::time::sleep(wait)),
        },
        identity,
        recording,
    )
    .await
    .map(|_| ())
}

/// Reads one request head and its declared body; returns the body as text.
async fn read_request(socket: &mut TcpStream) -> Option<String> {
    let mut bytes = Vec::new();
    let mut buffer = vec![0_u8; 16 * 1024];
    let end = loop {
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break end;
        }
        if bytes.len() > MAX_REQUEST_BYTES {
            return None;
        }
        let read = socket.read(&mut buffer).await.ok()?;
        if read == 0 {
            return None;
        }
        bytes.extend_from_slice(buffer.get(..read)?);
    };
    let head = String::from_utf8_lossy(bytes.get(..end)?).into_owned();
    let length: usize = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().ok())?
        })
        .unwrap_or(0);
    if length > MAX_REQUEST_BYTES {
        return None;
    }
    let mut body = bytes.get(end + 4..)?.to_vec();
    while body.len() < length {
        let read = socket.read(&mut buffer).await.ok()?;
        if read == 0 {
            return None;
        }
        body.extend_from_slice(buffer.get(..read)?);
    }
    String::from_utf8(body).ok()
}

/// Starts the scripted server: each request is kept, then answered with the next answer.
async fn serve(
    answers: Vec<Answer>,
    bodies: Arc<Mutex<Vec<String>>>,
) -> Result<SocketAddr, &'static str> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| "fixture:bind")?;
    let address = listener.local_addr().map_err(|_| "fixture:bind")?;
    tokio::spawn(async move {
        let mut served = 0_usize;
        while let Ok((mut socket, _)) = listener.accept().await {
            let Some(body) = read_request(&mut socket).await else {
                continue;
            };
            bodies
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(body);
            let Some(answer) = answers.get(served).or_else(|| answers.last()).cloned() else {
                continue;
            };
            served += 1;
            if answer.hang {
                // Held open past every authored timeout; the runtime drops it at the end.
                tokio::spawn(async move {
                    tokio::time::sleep(BACKSTOP).await;
                    drop(socket);
                });
                continue;
            }
            let body = match (&answer.body, answer.status) {
                (Some(body), _) => serde_json::to_vec(body).unwrap_or_default(),
                (None, 200..=299) => b"{}".to_vec(),
                (None, _) => br#"{"error":{"message":"fixture refusal"}}"#.to_vec(),
            };
            let head = format!(
                "HTTP/1.1 {} Fixture\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                answer.status,
                body.len()
            );
            let _ = socket.write_all(head.as_bytes()).await;
            let _ = socket.write_all(&body).await;
            let _ = socket.shutdown().await;
        }
    });
    Ok(address)
}
