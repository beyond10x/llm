//! Decision endpoint conformance observations against a scripted loopback server.
//!
//! This module parses the authored catalog, puts the fixture server's address in place of
//! `http://fixture.invalid`, and runs the real client, `llm_openai_decisions::OpenAiDecisions`:
//! for one decision request read by `DecisionRequest::from_json`, or as the endpoint of a real
//! `llm_decide::Chain` of `EndpointClassifier`s. A price naming the binding revision
//! `fixture-binding-revision` is priced under the revision its serving model's binding has at
//! that address. The server only replays the authored answers in order and keeps the path and
//! body of each request it received. Records go to an in-memory sink under a clock that never
//! moves. Every answer, count, decision, refusal and record comes back out of the library; it
//! reads no suite and branches on no scenario name. Nothing leaves the host.

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use ess_conformance::target::TargetError;
use llm_calls::{CallIdentity, CallLog, Clock, Recording};
use llm_core::{Cancel, Error, Id, Model};
use llm_cost::PriceBook;
use llm_credentials::AnonymousResolver;
use llm_decide::{Chain, Classifier, EndpointClassifier, LabelSet, Path, Ports};
use llm_http::{HttpClient, Limits};
use llm_openai_decisions::{DecisionRequest, OpenAiDecisions};
use llm_routing::{Catalog, Models, Selection};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use super::{ChainInput, LabelSetInput, Refusal, identifier, invalid, report, unavailable};
use crate::target::Observed;

/// The view this command answers through `query_view`.
pub const VIEW: &str = "llm.decisions.LastDecide";

const MAX_PROGRAM_BYTES: usize = 64 * 1024;
const MAX_ANSWERS: usize = 64;
/// The largest request the scripted server reads.
const MAX_REQUEST_BYTES: usize = 1024 * 1024;
/// The placeholder origin the authored catalog names for the fixture server.
const FIXTURE_ORIGIN: &str = "http://fixture.invalid";
/// The placeholder binding revision an authored price names.
const FIXTURE_REVISION: &str = "fixture-binding-revision";
/// A backstop so a fixture can never hang the gate. Far above every authored bound.
const BACKSTOP: Duration = Duration::from_secs(30);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DecideInput {
    catalog_toml: String,
    #[serde(default)]
    prices_toml: Option<String>,
    #[serde(default)]
    request_json: Option<String>,
    #[serde(default)]
    label_set_json: Option<String>,
    #[serde(default)]
    chain_json: Option<String>,
    #[serde(default)]
    input_text: Option<String>,
    program_json: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Program {
    request_id: String,
    /// The caller's input-token estimate, which routing admits a target against.
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    start_unix_ms: u64,
    answers: Vec<Answer>,
}

/// One answer of the fixture server, in request order; the last one repeats. `body_text` is
/// written byte for byte, else `body` as JSON.
#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct Answer {
    #[serde(default = "ok")]
    status: u16,
    #[serde(default)]
    body: Option<Value>,
    #[serde(default)]
    body_text: Option<String>,
}

const fn ok() -> u16 {
    200
}

/// What the fixture server received: each request's path and body, in order.
#[derive(Default)]
struct Received {
    paths: Vec<String>,
    bodies: Vec<String>,
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

/// The decision endpoint path binds no turn model.
struct NoModels;

impl Models for NoModels {
    fn model(&self, _: &Id) -> Option<&dyn Model> {
        None
    }
}

pub fn decide(input: &Value) -> Result<Observed, TargetError> {
    let input: DecideInput = serde_json::from_value(input.clone()).map_err(unavailable)?;
    Ok(Observed {
        facts: run(&input),
        view: VIEW,
        event: "llm.decisions.Decided",
        field: "valid_program",
    })
}

fn blank() -> Value {
    json!({
        "valid_program": false, "error_code": null, "error_message": null, "answers": [],
        "upstream_model": null, "input_tokens": null, "output_tokens": null,
        "cached_input_tokens": null, "cache_creation_input_tokens": null,
        "reasoning_output_tokens": null, "request_paths": [], "request_bodies": [],
        "tried": [], "results": [], "label": null, "confidences": [], "final_cause": null,
        "call_record_ids": [], "records": []
    })
}

fn run(input: &DecideInput) -> Value {
    let mut facts = blank();
    let authored = [
        &input.request_json,
        &input.label_set_json,
        &input.chain_json,
        &input.input_text,
    ];
    if input.program_json.len() > MAX_PROGRAM_BYTES
        || authored.iter().any(|text| {
            text.as_ref()
                .is_some_and(|text| text.len() > MAX_PROGRAM_BYTES)
        })
    {
        return facts;
    }
    let Ok(program) = serde_json::from_str::<Program>(&input.program_json) else {
        return facts;
    };
    // One decision request, or a chain with its label set and input: never both, never neither.
    let chain =
        input.label_set_json.is_some() && input.chain_json.is_some() && input.input_text.is_some();
    let no_chain =
        input.label_set_json.is_none() && input.chain_json.is_none() && input.input_text.is_none();
    let authored_one =
        (input.request_json.is_some() && no_chain) || (input.request_json.is_none() && chain);
    if !authored_one || program.answers.is_empty() || program.answers.len() > MAX_ANSWERS {
        return facts;
    }
    let Ok(record_id) = Id::new(&program.request_id) else {
        return facts;
    };
    facts["valid_program"] = json!(true);
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        facts["error_code"] = json!("fixture:no-runtime");
        return facts;
    };
    let received = Arc::new(Mutex::new(Received::default()));
    let log = CallLog::new();
    let clock = Frozen(program.start_unix_ms);
    // The timer is made inside the runtime, which owns its clock.
    let outcome = runtime.block_on(async {
        let run = exercise(
            input,
            program,
            record_id,
            &received,
            (&log, &clock),
            &mut facts,
        );
        tokio::time::timeout(BACKSTOP, run).await
    });
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(refusal)) => {
            facts["error_code"] = refusal.code();
            if let Refusal::Llm(error) = &refusal {
                facts["error_message"] = json!(error.message);
            }
        }
        Err(_) => facts["error_code"] = json!("fixture:backstop"),
    }
    drop(runtime);
    let received = received.lock().unwrap_or_else(PoisonError::into_inner);
    facts["request_paths"] = json!(received.paths);
    facts["request_bodies"] = json!(received.bodies);
    facts["records"] = json!(
        log.records()
            .iter()
            .filter_map(|record| serde_json::to_string(record).ok())
            .collect::<Vec<_>>()
    );
    facts
}

fn fixture(code: &'static str) -> Refusal {
    Refusal::Llm(Error::new(llm_core::ErrorCode::Unavailable, code))
}

/// The authored price book, each `fixture-binding-revision` replaced by the revision the
/// price's serving model has in the fixture catalog.
fn prices(source: &str, catalog: &Catalog) -> Result<PriceBook, Refusal> {
    let unreadable = |_| Refusal::Llm(Error::invalid("invalid price book"));
    let mut document = PriceBook::parse_toml(source)
        .map_err(unreadable)?
        .document()
        .clone();
    for price in &mut document.models {
        if price.binding_revision.as_str() == FIXTURE_REVISION
            && let Some(binding) = catalog.binding(&price.serving_model_id)
        {
            price.binding_revision = binding.provenance().binding_revision.clone();
        }
    }
    PriceBook::from_document(document).map_err(unreadable)
}

/// The authored chain over the authored label set, each classifier asking `client`.
fn chain(input: &DecideInput, client: &OpenAiDecisions) -> Result<Chain, Refusal> {
    let set: LabelSetInput =
        serde_json::from_str(input.label_set_json.as_deref().unwrap_or_default())
            .map_err(invalid("invalid label set"))?;
    let label_set = LabelSet::new(identifier(&set.id)?, set.version, set.labels)?;
    let chain: ChainInput = serde_json::from_str(input.chain_json.as_deref().unwrap_or_default())
        .map_err(invalid("invalid chain"))?;
    let mut classifiers: Vec<Box<dyn Classifier>> = Vec::new();
    for each in chain.classifiers {
        if each.path != Path::DecisionEndpoint {
            return Err(
                Error::invalid("this command runs decision-endpoint classifiers only").into(),
            );
        }
        let route = each
            .route_alias
            .ok_or_else(|| Error::invalid("a decision-endpoint classifier names a route"))?;
        let classifier = EndpointClassifier::new(identifier(&each.id)?, route, client.clone())
            .with_instructions(each.instructions.unwrap_or_default());
        classifiers.push(Box::new(match each.min_confidence_bps {
            Some(bps) => classifier.with_min_confidence_bps(bps)?,
            None => classifier,
        }));
    }
    Ok(Chain::new(
        identifier(&chain.id)?,
        label_set,
        Duration::from_millis(chain.deadline_ms),
        classifiers,
    )?)
}

/// Runs the request or the chain against the fixture and writes what the library returned into
/// `facts`.
async fn exercise(
    input: &DecideInput,
    program: Program,
    record_id: Id,
    received: &Arc<Mutex<Received>>,
    (log, clock): (&CallLog, &Frozen),
    facts: &mut Value,
) -> Result<(), Refusal> {
    let address = serve(program.answers, received.clone())
        .await
        .map_err(fixture)?;
    let source = input
        .catalog_toml
        .replace(FIXTURE_ORIGIN, &format!("http://{address}"));
    let catalog = Catalog::parse(&source)?;
    let prices = input
        .prices_toml
        .as_deref()
        .map(|source| prices(source, &catalog))
        .transpose()?;
    let http = HttpClient::new(Limits {
        response_headers: Duration::from_secs(20),
        idle: Duration::from_secs(20),
        total: Duration::from_secs(20),
    })
    .map_err(|_| fixture("fixture:client"))?;
    let client = OpenAiDecisions::new(http, Arc::new(AnonymousResolver));
    let recording = Recording {
        sink: log,
        clock,
        prices: prices.as_ref(),
    };
    let cancel = Cancel::new();
    if let Some(request_json) = &input.request_json {
        let request = DecisionRequest::from_json(request_json)?;
        let selection = OpenAiDecisions::select(&catalog, &request, program.input_tokens)?;
        let response = client
            .decide_recorded(
                &selection,
                &request,
                &cancel,
                CallIdentity::primary(record_id),
                recording,
            )
            .await?;
        facts["answers"] = json!(
            response
                .answers
                .iter()
                .filter_map(|answer| serde_json::to_value(answer).ok())
                .map(|answer| answer.to_string())
                .collect::<Vec<_>>()
        );
        facts["upstream_model"] = json!(response.model);
        facts["input_tokens"] = json!(response.usage.input_tokens);
        facts["output_tokens"] = json!(response.usage.output_tokens);
        facts["cached_input_tokens"] = json!(response.usage.cached_input_tokens);
        facts["cache_creation_input_tokens"] = json!(response.usage.cache_creation_input_tokens);
        facts["reasoning_output_tokens"] = json!(response.usage.reasoning_output_tokens);
        return Ok(());
    }
    let chain = chain(input, &client)?;
    let admit = |_: &Selection<'_>| Ok(());
    let outcome = chain
        .classify(
            &record_id,
            input.input_text.as_deref().unwrap_or_default(),
            program.input_tokens,
            Ports {
                catalog: &catalog,
                models: &NoModels,
                admit: &admit,
                cancel: &cancel,
                pause: &|wait| Box::pin(tokio::time::sleep(wait)),
                recording,
            },
        )
        .await;
    report(&outcome, facts);
    Ok(())
}

/// Reads one request head and its declared body; returns the request path and the body as text.
async fn read_request(socket: &mut TcpStream) -> Option<(String, String)> {
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
    let path = head.split_whitespace().nth(1)?.to_owned();
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
    Some((path, String::from_utf8(body).ok()?))
}

/// Starts the scripted server: each request is kept, then answered with the next answer.
async fn serve(
    answers: Vec<Answer>,
    received: Arc<Mutex<Received>>,
) -> Result<SocketAddr, &'static str> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| "fixture:bind")?;
    let address = listener.local_addr().map_err(|_| "fixture:bind")?;
    tokio::spawn(async move {
        let mut served = 0_usize;
        while let Ok((mut socket, _)) = listener.accept().await {
            let Some((path, body)) = read_request(&mut socket).await else {
                continue;
            };
            {
                let mut received = received.lock().unwrap_or_else(PoisonError::into_inner);
                received.paths.push(path);
                received.bodies.push(body);
            }
            let Some(answer) = answers.get(served).or_else(|| answers.last()).cloned() else {
                continue;
            };
            served += 1;
            let body = match (answer.body_text, &answer.body, answer.status) {
                (Some(text), _, _) => text.into_bytes(),
                (None, Some(body), _) => serde_json::to_vec(body).unwrap_or_default(),
                (None, None, 200..=299) => b"{}".to_vec(),
                (None, None, _) => br#"{"error":{"message":"fixture refusal"}}"#.to_vec(),
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
