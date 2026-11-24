//! The `OpenAI` decision endpoint client against a loopback fixture server: no live call.
//!
//! The request and answer bodies are the example and the schemas of `POST /decisions` in
//! `openai/openai-openapi` at commit `b4cad10d7cff7fc286f2dbf1992569989522c6f0` (operation
//! `createDecision`, schemas `DecisionRequest` and `DecisionResponse`). The model name is the one
//! that reference names.

use llm_calls::{CallIdentity, CallLog, CallRecord, Clock, Kind, Outcome, Recording};
use llm_core::{BoxFuture, Cancel, Dispatch, Error, ErrorCode, Id, Usage};
use llm_cost::{Amount, Currency, ModelPrice, PriceBook, PriceDocument, Rate};
use llm_credentials::{
    AnonymousResolver, ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver,
    SecretVersion,
};
use llm_decide::{
    AbstentionCause, Chain, ChainOutcome, Classification, EndpointClassifier, LabelConfidence,
    LabelSet, Ports,
};
use llm_http::{HttpClient, Limits};
use llm_openai_decisions::{
    AnswerKind, ChoiceOption, ChoiceProbability, ChoiceValue, DecisionAnswer, DecisionRequest,
    DecisionResponse, MAX_DEPTH, OpenAiDecisions, PromptCacheMode, PromptCacheOptions,
    PromptCacheTtl, Question, QuestionKind, ScoreLevel, ScoreProbability,
};
use llm_routing::{Catalog, Models, Selection};
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

/// The request example of the reference, with the route alias in place of the model.
const EXAMPLE_REQUEST: &str = r#"{
  "model": "decide",
  "input": "The package arrived with a broken screen.",
  "questions": [
    {
      "type": "predicate",
      "name": "damaged",
      "instructions": "Does the customer report a damaged item?"
    }
  ]
}"#;

/// The response example of the reference, as it is written there.
const EXAMPLE_RESPONSE: &str = r#"{
  "model": "gpt-6-luna",
  "answers": [
    {"type": "predicate", "name": "damaged", "probability": 0.95}
  ],
  "usage": {
    "input_tokens": 42,
    "input_tokens_details": {"cached_tokens": 0, "cache_write_tokens": 0},
    "output_tokens": 0,
    "output_tokens_details": {"reasoning_tokens": 0},
    "total_tokens": 42
  }
}"#;

/// One answer of the fixture server, in request order; the last one repeats.
#[derive(Clone)]
enum Answer {
    /// These exact bytes as a JSON body.
    Body(String),
    /// The first half of these bytes under a `content-length` of all of them, then the
    /// connection's end: a body the transport reports as cut.
    Cut(String),
    Status(u16),
    Hang,
}

fn body(value: &Value) -> Answer {
    Answer::Body(value.to_string())
}

#[derive(Default)]
struct Seen {
    connections: AtomicUsize,
    requests: Mutex<Vec<Request>>,
}

#[derive(Clone)]
struct Request {
    method: String,
    path: String,
    authorization: Option<String>,
    content_type: Option<String>,
    body: Vec<u8>,
}

impl Seen {
    fn requests(&self) -> Vec<Request> {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    fn bodies(&self) -> Vec<Value> {
        self.requests()
            .iter()
            .map(|request| serde_json::from_slice(&request.body).unwrap())
            .collect()
    }
}

async fn read_request(socket: &mut TcpStream) -> Option<Request> {
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
    let mut words = head.split_whitespace();
    let method = words.next()?.to_owned();
    let path = words.next()?.to_owned();
    let length: usize = header("content-length")?.parse().ok()?;
    let mut body = bytes[end + 4..].to_vec();
    while body.len() < length {
        let read = socket.read(&mut buffer).await.ok()?;
        if read == 0 {
            return None;
        }
        body.extend_from_slice(&buffer[..read]);
    }
    Some(Request {
        method,
        path,
        authorization: header("authorization"),
        content_type: header("content-type"),
        body,
    })
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
            let Some(request) = read_request(&mut socket).await else {
                continue;
            };
            shared
                .requests
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(request);
            let answer = answers[served.min(answers.len() - 1)].clone();
            served += 1;
            let (status, body) = match answer {
                Answer::Body(text) => (200, text.into_bytes()),
                Answer::Cut(text) => {
                    let head = format!(
                        "HTTP/1.1 200 Fixture\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        text.len()
                    );
                    let _ = socket.write_all(head.as_bytes()).await;
                    let _ = socket.write_all(&text.as_bytes()[..text.len() / 2]).await;
                    let _ = socket.shutdown().await;
                    continue;
                }
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

const ANONYMOUS: &str = "auth_kind = \"anonymous\"\nbilling_kind = \"self-hosted\"";

/// A turn route `decide` (target `decide-primary`, serving model `decider`, upstream
/// `gpt-6-luna`) served over `protocol` by the fixture at `address`, and an embed route `embed`.
fn catalog_toml(address: SocketAddr, protocol: &str, auth: &str) -> String {
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
id = "luna"
upstream_name = "gpt-6-luna"

[[serving_models]]
id = "decider"
endpoint_id = "fixture"
model_id = "luna"
protocol = "{protocol}"
[serving_models.capabilities]
tools = false
tool_choice = false
temperature = false
top_p = false
reasoning_efforts = []
context_window = 4096
max_output_tokens = 1024

[[serving_models]]
id = "embedder"
endpoint_id = "fixture"
model_id = "luna"
protocol = "chat-completions"
[serving_models.capabilities]
tools = false
tool_choice = false
temperature = false
top_p = false
reasoning_efforts = []
context_window = 4096
max_output_tokens = 1

[[routes]]
id = "deciding"
alias = "decide"

[[routes]]
id = "embedding"
alias = "embed"
kind = "embed"

[[targets]]
id = "decide-primary"
route_id = "deciding"
serving_model_id = "decider"
position = 0

[[targets]]
id = "embed-primary"
route_id = "embedding"
serving_model_id = "embedder"
position = 0
"#
    )
}

fn catalog(address: SocketAddr) -> Catalog {
    Catalog::parse(&catalog_toml(address, "responses", ANONYMOUS)).unwrap()
}

fn http() -> HttpClient {
    HttpClient::new(Limits {
        response_headers: Duration::from_secs(20),
        idle: Duration::from_secs(20),
        total: Duration::from_secs(20),
    })
    .unwrap()
}

fn client() -> OpenAiDecisions {
    OpenAiDecisions::new(http(), Arc::new(AnonymousResolver))
}

fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}

fn example() -> DecisionRequest {
    DecisionRequest::from_json(EXAMPLE_REQUEST).unwrap()
}

fn predicate(name: &str) -> Question {
    Question::Predicate {
        name: Some(name.to_owned()),
        instructions: "Is it so?".to_owned(),
    }
}

/// A choice question named `tone` over the texts `choices`.
fn choice(choices: &[&str]) -> Question {
    Question::Choice {
        name: Some("tone".to_owned()),
        instructions: "Which tone?".to_owned(),
        choices: choices.iter().copied().map(ChoiceOption::text).collect(),
    }
}

fn score() -> Question {
    Question::Score {
        name: Some("quality".to_owned()),
        instructions: "How good?".to_owned(),
        levels: vec![
            ScoreLevel {
                label: "poor".to_owned(),
                description: None,
            },
            ScoreLevel {
                label: "good".to_owned(),
                description: Some("no defect".to_owned()),
            },
        ],
    }
}

fn asking(questions: Vec<Question>) -> DecisionRequest {
    DecisionRequest::new("decide", "The package arrived.", questions)
}

/// The usage member of the reference's example.
fn usage() -> Value {
    json!({
        "input_tokens": 42,
        "input_tokens_details": {"cached_tokens": 0, "cache_write_tokens": 0},
        "output_tokens": 0,
        "output_tokens_details": {"reasoning_tokens": 0},
        "total_tokens": 42
    })
}

fn answering(answers: &Value) -> Value {
    json!({"model": "gpt-6-luna", "answers": answers, "usage": usage()})
}

/// One call of `request` against a fixture that gives `answers`.
async fn decide(
    answers: Vec<Answer>,
    request: &DecisionRequest,
) -> Result<DecisionResponse, Error> {
    let (address, _) = serve(answers).await;
    let catalog = catalog(address);
    let selection = OpenAiDecisions::select(&catalog, request, Some(100)).unwrap();
    client().decide(&selection, request, &Cancel::new()).await
}

/// The message of the refusal of one answer body to `request`, which is `protocol`, dispatch
/// `accepted` and final.
async fn protocol_refusal(text: &str, request: &DecisionRequest) -> String {
    let error = decide(vec![Answer::Body(text.to_owned())], request)
        .await
        .expect_err(text);
    assert_eq!(
        (error.code, error.dispatch, error.retriable),
        (ErrorCode::Protocol, Dispatch::Accepted, false),
        "{text}"
    );
    error.message
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

/// One recorded call of `request` against a fixture that gives `answers`.
async fn recorded(
    answers: Vec<Answer>,
    request: &DecisionRequest,
    prices: impl FnOnce(&Catalog) -> Option<PriceBook>,
) -> (Result<DecisionResponse, Error>, Vec<CallRecord>, Arc<Seen>) {
    let (address, seen) = serve(answers).await;
    let catalog = catalog(address);
    let prices = prices(&catalog);
    let selection = OpenAiDecisions::select(&catalog, request, Some(100)).unwrap();
    let log = CallLog::new();
    let clock = Frozen(1_700_000_000_000);
    let result = client()
        .decide_recorded(
            &selection,
            request,
            &Cancel::new(),
            CallIdentity::primary(id("req-1")),
            Recording {
                sink: &log,
                clock: &clock,
                prices: prices.as_ref(),
            },
        )
        .await;
    (result, log.records(), seen)
}

fn no_prices(_: &Catalog) -> Option<PriceBook> {
    None
}

/// Input 1 and output 2 per 1000 tokens, cached input 0.5, for the fixture's serving model.
#[expect(clippy::unnecessary_wraps, reason = "the signature `no_prices` has")]
fn prices(catalog: &Catalog) -> Option<PriceBook> {
    let binding = catalog.binding(&id("decider")).unwrap();
    let rate = |amount: &str| {
        Some(Rate {
            amount: Amount::parse(amount).unwrap(),
            per_units: 1000,
        })
    };
    Some(
        PriceBook::from_document(PriceDocument::new(
            id("prices-r1"),
            "fixture price list",
            1,
            Currency::new("USD").unwrap(),
            vec![ModelPrice {
                id: id("price-luna"),
                serving_model_id: id("decider"),
                binding_revision: binding.provenance().binding_revision.clone(),
                upstream_model: id("gpt-6-luna"),
                input: rate("1"),
                cached_input: rate("0.5"),
                cache_creation_input: rate("1"),
                output: rate("2"),
            }],
        ))
        .unwrap(),
    )
}

#[tokio::test]
async fn the_reference_example_is_sent_as_post_v1_decisions_and_answered() {
    let (address, seen) = serve(vec![Answer::Body(EXAMPLE_RESPONSE.to_owned())]).await;
    let catalog = catalog(address);
    let request = example();
    let selection = OpenAiDecisions::select(&catalog, &request, Some(100)).unwrap();
    assert_eq!(selection.target.id, id("decide-primary"));
    let response = client()
        .decide(&selection, &request, &Cancel::new())
        .await
        .unwrap();

    let requests = seen.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/v1/decisions");
    assert_eq!(
        requests[0].content_type.as_deref(),
        Some("application/json")
    );
    assert_eq!(
        requests[0].authorization, None,
        "an anonymous account sends none"
    );
    // The body is the reference's example: the route alias became the target's upstream model.
    assert_eq!(
        seen.bodies(),
        [json!({
            "model": "gpt-6-luna",
            "input": "The package arrived with a broken screen.",
            "questions": [{
                "type": "predicate",
                "name": "damaged",
                "instructions": "Does the customer report a damaged item?"
            }]
        })]
    );
    assert_eq!(
        response,
        DecisionResponse {
            model: "gpt-6-luna".to_owned(),
            answers: vec![DecisionAnswer::Predicate {
                name: Some("damaged".to_owned()),
                probability: 0.95,
            }],
            usage: Usage {
                input_tokens: Some(42),
                output_tokens: Some(0),
                cached_input_tokens: Some(0),
                cache_creation_input_tokens: Some(0),
                reasoning_output_tokens: Some(0),
            },
        }
    );
    assert_eq!(response.answers[0].kind(), AnswerKind::Predicate);
    assert_eq!(response.answers[0].name(), Some("damaged"));
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one request and one answer of every kind"
)]
async fn every_question_kind_is_sent_and_every_answer_kind_is_read() {
    let mut request = asking(vec![
        predicate("damaged"),
        Question::Choice {
            name: None,
            instructions: "Which tone?".to_owned(),
            choices: vec![
                ChoiceOption {
                    value: ChoiceValue::Text("true".to_owned()),
                    description: Some("the word".to_owned()),
                },
                ChoiceOption {
                    value: ChoiceValue::Flag(true),
                    description: None,
                },
            ],
        },
        score(),
        predicate("refused"),
    ]);
    request.safety_identifier = Some("user-1".to_owned());
    request.prompt_cache_key = Some("key-1".to_owned());
    request.prompt_cache_options = Some(PromptCacheOptions {
        ttl: Some(PromptCacheTtl::ThirtyMinutes),
        mode: Some(PromptCacheMode::Explicit),
    });
    assert_eq!(request.questions[1].kind(), QuestionKind::Choice);
    let answers = json!([
        {"type": "predicate", "name": "damaged", "probability": 1},
        {
            "type": "choice", "name": null, "choice": true, "confidence": 0.7,
            "probabilities": [
                {"value": "true", "probability": 0.3},
                {"value": true, "probability": 0.7}
            ]
        },
        {
            "type": "score", "name": "quality", "score": 0.75, "confidence": 0.5,
            "probabilities": [
                {"value": 0, "label": "poor", "probability": 0.25},
                {"value": 1, "label": "good", "probability": 0.75}
            ]
        },
        {"type": "refusal", "name": "refused"}
    ]);
    let (address, seen) = serve(vec![body(&answering(&answers))]).await;
    let catalog = catalog(address);
    let selection = OpenAiDecisions::select(&catalog, &request, Some(100)).unwrap();
    let response = client()
        .decide(&selection, &request, &Cancel::new())
        .await
        .unwrap();
    assert_eq!(
        seen.bodies(),
        [json!({
            "model": "gpt-6-luna",
            "input": "The package arrived.",
            "questions": [
                {"type": "predicate", "name": "damaged", "instructions": "Is it so?"},
                {
                    "type": "choice", "instructions": "Which tone?",
                    "choices": [{"value": "true", "description": "the word"}, {"value": true}]
                },
                {
                    "type": "score", "name": "quality", "instructions": "How good?",
                    "levels": [{"label": "poor"}, {"label": "good", "description": "no defect"}]
                },
                {"type": "predicate", "name": "refused", "instructions": "Is it so?"}
            ],
            "safety_identifier": "user-1",
            "prompt_cache_key": "key-1",
            "prompt_cache_options": {"ttl": "30m", "mode": "explicit"}
        })],
        "only the reference's members, an absent option left out"
    );
    assert_eq!(
        response.answers,
        [
            DecisionAnswer::Predicate {
                name: Some("damaged".to_owned()),
                probability: 1.0,
            },
            DecisionAnswer::Choice {
                name: None,
                choice: ChoiceValue::Flag(true),
                probabilities: vec![
                    ChoiceProbability {
                        value: ChoiceValue::Text("true".to_owned()),
                        probability: 0.3,
                    },
                    ChoiceProbability {
                        value: ChoiceValue::Flag(true),
                        probability: 0.7,
                    },
                ],
                confidence: 0.7,
            },
            DecisionAnswer::Score {
                name: Some("quality".to_owned()),
                score: 0.75,
                probabilities: vec![
                    ScoreProbability {
                        value: 0,
                        label: "poor".to_owned(),
                        probability: 0.25,
                    },
                    ScoreProbability {
                        value: 1,
                        label: "good".to_owned(),
                        probability: 0.75,
                    },
                ],
                confidence: 0.5,
            },
            DecisionAnswer::Refusal {
                name: Some("refused".to_owned()),
            },
        ]
    );
    // An answer serializes in the reference's shape.
    assert_eq!(
        serde_json::to_value(&response.answers[3]).unwrap(),
        json!({"type": "refusal", "name": "refused"})
    );
    assert_eq!(
        serde_json::to_value(&response.answers[0]).unwrap(),
        json!({"type": "predicate", "name": "damaged", "probability": 1.0})
    );
}

#[tokio::test]
async fn an_answer_body_is_taken_only_whole() {
    let request = example();
    let whole = "decision answer is not one whole JSON document with every member once, nested at most 16 deep";
    // The fixture declares the length of what it sends, so `cut` and `half` arrive whole by the
    // transport's account: the document ends early, the body does not.
    let cut = &EXAMPLE_RESPONSE[..EXAMPLE_RESPONSE.len() - 1];
    let half = &EXAMPLE_RESPONSE[..EXAMPLE_RESPONSE.len() / 2];
    let trailing = format!("{EXAMPLE_RESPONSE}x");
    let second = format!("{EXAMPLE_RESPONSE}{EXAMPLE_RESPONSE}");
    let second_spaced = format!("{EXAMPLE_RESPONSE}\n{{}}");
    let repeated_root = EXAMPLE_RESPONSE.replacen('{', r#"{"model": "gpt-6-luna","#, 1);
    let repeated_answers = EXAMPLE_RESPONSE.replacen("\"usage\"", "\"answers\": [], \"usage\"", 1);
    let repeated_inside = EXAMPLE_RESPONSE.replace(
        r#""probability": 0.95"#,
        r#""probability": 0.05, "probability": 0.95"#,
    );
    let repeated_usage = EXAMPLE_RESPONSE.replace(
        r#""cached_tokens": 0,"#,
        r#""cached_tokens": 0, "cached_tokens": 0,"#,
    );
    // The second `model` is written with one letter escaped: the same name, other bytes.
    let repeated_escaped = EXAMPLE_RESPONSE.replacen('{', "{\"mod\\u0065l\": \"gpt-6-luna\",", 1);
    assert!(repeated_escaped.contains(r#""model""#));
    assert!(!repeated_root.contains('\\'));
    for text in [
        "",
        " ",
        cut,
        half,
        &trailing,
        &second,
        &second_spaced,
        &repeated_root,
        &repeated_answers,
        &repeated_inside,
        &repeated_usage,
        &repeated_escaped,
        "\u{feff}{}",
        "not json",
    ] {
        assert_eq!(protocol_refusal(text, &request).await, whole, "{text}");
    }
    // JSON whitespace around the one document is part of no value.
    let spaced = format!(" \n\t{EXAMPLE_RESPONSE}\r\n ");
    assert!(decide(vec![Answer::Body(spaced)], &request).await.is_ok());

    // A document that is whole and is no object, or lacks a member.
    for (text, message) in [
        (
            "[]",
            "decision answer lacks a member or holds one of the wrong type: (document)",
        ),
        (
            "null",
            "decision answer lacks a member or holds one of the wrong type: (document)",
        ),
        (
            "{}",
            "decision answer lacks a member or holds one of the wrong type: model",
        ),
        (
            r#"{"model": 7, "answers": []}"#,
            "decision answer lacks a member or holds one of the wrong type: model",
        ),
        (
            r#"{"model": "gpt-6-luna"}"#,
            "decision answer lacks a member or holds one of the wrong type: answers",
        ),
        (
            r#"{"model": "gpt-6-luna", "answers": {}}"#,
            "decision answer lacks a member or holds one of the wrong type: answers",
        ),
    ] {
        assert_eq!(protocol_refusal(text, &request).await, message, "{text}");
    }
}

#[tokio::test]
async fn an_answer_nested_past_the_bound_is_refused_and_one_at_the_bound_is_read() {
    assert_eq!(MAX_DEPTH, 16);
    let request = example();
    // A member the reference does not name is left alone, so it can carry the nesting: the
    // document's object is level 1 and each array one more.
    let nested = |arrays: usize| {
        EXAMPLE_RESPONSE.replacen(
            '{',
            &format!(
                r#"{{"extra": {}{},"#,
                "[".repeat(arrays),
                "]".repeat(arrays)
            ),
            1,
        )
    };
    assert!(
        decide(vec![Answer::Body(nested(15))], &request)
            .await
            .is_ok(),
        "16 levels are read"
    );
    assert_eq!(
        protocol_refusal(&nested(16), &request).await,
        "decision answer is not one whole JSON document with every member once, nested at most 16 deep"
    );
}

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "one table of answers")]
async fn an_answer_is_held_to_its_question() {
    let one = |answer: Value| answering(&json!([answer])).to_string();
    let predicate_request = asking(vec![predicate("damaged")]);
    let mistyped = |path: &str| {
        format!("decision answer lacks a member or holds one of the wrong type: {path}")
    };

    // An answer type the reference does not name is refused by its path, never guessed.
    for unknown in ["label", "Predicate", "", "predicate "] {
        let text = one(json!({"type": unknown, "name": "damaged", "probability": 0.5}));
        assert_eq!(
            protocol_refusal(&text, &predicate_request).await,
            "decision answer has a type outside predicate, choice, score and refusal: answers[0].type"
        );
    }
    // The path names the answer's position.
    let two = asking(vec![predicate("damaged"), predicate("late")]);
    let text = answering(&json!([
        {"type": "predicate", "name": "damaged", "probability": 0.5},
        {"type": "ranking", "name": "late"}
    ]))
    .to_string();
    assert_eq!(
        protocol_refusal(&text, &two).await,
        "decision answer has a type outside predicate, choice, score and refusal: answers[1].type"
    );

    // A known type that is not the question's.
    let text = one(json!({
        "type": "choice", "name": "damaged", "choice": "a", "probabilities": [], "confidence": 1
    }));
    assert_eq!(
        protocol_refusal(&text, &predicate_request).await,
        "decision answer is of another type than its question: answers[0].type"
    );

    // Not one answer per question: none, and one too many.
    for answers in [
        json!([]),
        json!([
            {"type": "predicate", "name": "damaged", "probability": 0.5},
            {"type": "predicate", "name": "damaged", "probability": 0.5}
        ]),
    ] {
        assert_eq!(
            protocol_refusal(&answering(&answers).to_string(), &predicate_request).await,
            "decision answer does not hold one answer per question: answers"
        );
    }

    // The question's name: required, null exactly for an unnamed question.
    for (answer, message) in [
        (
            json!({"type": "predicate", "probability": 0.5}),
            mistyped("answers[0].name"),
        ),
        (
            json!({"type": "predicate", "name": 7, "probability": 0.5}),
            mistyped("answers[0].name"),
        ),
        (
            json!({"type": "predicate", "name": "other", "probability": 0.5}),
            "decision answer does not carry its question's name: answers[0].name".to_owned(),
        ),
        (
            json!({"type": "predicate", "name": null, "probability": 0.5}),
            "decision answer does not carry its question's name: answers[0].name".to_owned(),
        ),
        (
            json!({"type": "refusal", "name": "other"}),
            "decision answer does not carry its question's name: answers[0].name".to_owned(),
        ),
        (
            json!({"name": "damaged", "probability": 0.5}),
            mistyped("answers[0].type"),
        ),
        (
            json!({"type": 1, "name": "damaged"}),
            mistyped("answers[0].type"),
        ),
        (json!("predicate"), mistyped("answers[0]")),
        (
            json!({"type": "predicate", "name": "damaged"}),
            mistyped("answers[0].probability"),
        ),
        (
            json!({"type": "predicate", "name": "damaged", "probability": "0.5"}),
            mistyped("answers[0].probability"),
        ),
        (
            json!({"type": "predicate", "name": "damaged", "probability": null}),
            mistyped("answers[0].probability"),
        ),
    ] {
        assert_eq!(
            protocol_refusal(&one(answer.clone()), &predicate_request).await,
            message,
            "{answer}"
        );
    }
    let unnamed = asking(vec![Question::Predicate {
        name: None,
        instructions: "Is it so?".to_owned(),
    }]);
    let text = one(json!({"type": "predicate", "name": null, "probability": 0.5}));
    assert!(decide(vec![Answer::Body(text)], &unnamed).await.is_ok());
    let text = one(json!({"type": "predicate", "name": "", "probability": 0.5}));
    assert_eq!(
        protocol_refusal(&text, &unnamed).await,
        "decision answer does not carry its question's name: answers[0].name",
        "an empty name is a name"
    );
}

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "one table of answers")]
async fn a_choice_or_score_answer_missing_a_member_is_refused_by_its_path() {
    let one = |answer: Value| answering(&json!([answer])).to_string();
    let mistyped = |path: &str| {
        format!("decision answer lacks a member or holds one of the wrong type: {path}")
    };
    let choice_request = asking(vec![choice(&["warm", "cold"])]);
    let full = json!({
        "type": "choice", "name": "tone", "choice": "warm", "confidence": 0.9,
        "probabilities": [{"value": "warm", "probability": 0.9}]
    });
    assert!(
        decide(vec![Answer::Body(one(full.clone()))], &choice_request)
            .await
            .is_ok()
    );
    let without = |member: &str| {
        let mut answer = full.clone();
        answer.as_object_mut().unwrap().remove(member);
        answer
    };
    let with = |member: &str, value: Value| {
        let mut answer = full.clone();
        answer[member] = value;
        answer
    };
    for (answer, path) in [
        (without("choice"), "answers[0].choice"),
        (with("choice", json!(1)), "answers[0].choice"),
        (with("choice", json!(null)), "answers[0].choice"),
        (without("confidence"), "answers[0].confidence"),
        (without("probabilities"), "answers[0].probabilities"),
        (with("probabilities", json!({})), "answers[0].probabilities"),
        (
            with("probabilities", json!([7])),
            "answers[0].probabilities[0]",
        ),
        (
            with(
                "probabilities",
                json!([{"value": "warm", "probability": 0.9}, {"value": "cold"}]),
            ),
            "answers[0].probabilities[1].probability",
        ),
        (
            with("probabilities", json!([{"value": 3, "probability": 0.9}])),
            "answers[0].probabilities[0].value",
        ),
    ] {
        assert_eq!(
            protocol_refusal(&one(answer.clone()), &choice_request).await,
            mistyped(path),
            "{answer}"
        );
    }

    let score_request = asking(vec![score()]);
    let full = json!({
        "type": "score", "name": "quality", "score": 1, "confidence": 0.9,
        "probabilities": [{"value": 1, "label": "good", "probability": 0.9}]
    });
    assert!(
        decide(vec![Answer::Body(one(full.clone()))], &score_request)
            .await
            .is_ok()
    );
    let with = |member: &str, value: Value| {
        let mut answer = full.clone();
        answer[member] = value;
        answer
    };
    for (answer, path) in [
        (with("score", json!("high")), "answers[0].score"),
        (
            with(
                "probabilities",
                json!([{"value": 1.5, "label": "good", "probability": 0.9}]),
            ),
            "answers[0].probabilities[0].value",
        ),
        (
            // One past the largest 64-bit signed integer.
            with(
                "probabilities",
                json!([{"value": 9_223_372_036_854_775_808_u64, "label": "good", "probability": 0.9}]),
            ),
            "answers[0].probabilities[0].value",
        ),
        (
            with("probabilities", json!([{"value": 1, "probability": 0.9}])),
            "answers[0].probabilities[0].label",
        ),
    ] {
        assert_eq!(
            protocol_refusal(&one(answer.clone()), &score_request).await,
            mistyped(path),
            "{answer}"
        );
    }
    let largest = with(
        "probabilities",
        json!([{"value": i64::MAX, "label": "good", "probability": 0.9}]),
    );
    assert!(
        decide(vec![Answer::Body(one(largest))], &score_request)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn a_refusal_names_a_path_and_never_a_value_of_the_body() {
    let marker = "marker-quokka-7731";
    let request = asking(vec![predicate("damaged")]);
    for text in [
        answering(&json!([{"type": marker, "name": "damaged", "probability": 0.5}])).to_string(),
        answering(&json!([{"type": "predicate", "name": marker, "probability": 0.5}])).to_string(),
        answering(&json!([{"type": "predicate", "name": "damaged", "probability": marker}]))
            .to_string(),
        json!({"model": "gpt-6-luna", "answers": marker}).to_string(),
        format!("{{\"model\": \"{marker}\""),
    ] {
        let message = protocol_refusal(&text, &request).await;
        assert!(!message.contains(marker), "{message}");
    }
}

#[tokio::test]
async fn an_unreported_usage_counter_stays_unknown() {
    let request = example();
    let with_usage = |usage: Value| {
        let mut answer: Value = serde_json::from_str(EXAMPLE_RESPONSE).unwrap();
        match usage {
            Value::Null => {
                answer.as_object_mut().unwrap().remove("usage");
            }
            usage => answer["usage"] = usage,
        }
        Answer::Body(answer.to_string())
    };
    let known = |input, output, cached, written, reasoning| Usage {
        input_tokens: input,
        output_tokens: output,
        cached_input_tokens: cached,
        cache_creation_input_tokens: written,
        reasoning_output_tokens: reasoning,
    };
    for (usage, expected) in [
        // No usage member at all, and one of another type.
        (Value::Null, Usage::default()),
        (json!("none"), Usage::default()),
        (json!({}), Usage::default()),
        // Only what is reported is known; total_tokens is no counter of its own.
        (
            json!({"input_tokens": 42, "total_tokens": 42}),
            known(Some(42), None, None, None, None),
        ),
        (
            json!({"output_tokens": 3, "output_tokens_details": {}}),
            known(None, Some(3), None, None, None),
        ),
        (
            json!({"input_tokens": 42, "input_tokens_details": {"cached_tokens": 40}}),
            known(Some(42), None, Some(40), None, None),
        ),
        // A reported zero is a known zero.
        (
            json!({
                "input_tokens": 0, "output_tokens": 0,
                "input_tokens_details": {"cached_tokens": 0, "cache_write_tokens": 0},
                "output_tokens_details": {"reasoning_tokens": 0}
            }),
            known(Some(0), Some(0), Some(0), Some(0), Some(0)),
        ),
        // null, a negative count, a fraction, an exponent and a text are not counts.
        (
            json!({
                "input_tokens": null, "output_tokens": -1,
                "input_tokens_details": {"cached_tokens": 1.5, "cache_write_tokens": "7"},
                "output_tokens_details": {"reasoning_tokens": 4.0}
            }),
            Usage::default(),
        ),
        (
            json!({"input_tokens": 18_446_744_073_709_551_615_u64}),
            known(Some(u64::MAX), None, None, None, None),
        ),
        // The reference bounds each counter to -2147483648..=2147483647. That bound is not
        // enforced: a count on either side of 2147483647 is taken as written, and a negative
        // counter is unknown beside a known one, at -1 and at the reference's lower bound.
        (
            json!({"input_tokens": 2_147_483_647_u64, "output_tokens": 2_147_483_647_u64}),
            known(Some(2_147_483_647), Some(2_147_483_647), None, None, None),
        ),
        (
            json!({"input_tokens": 2_147_483_648_u64, "output_tokens": 2_147_483_648_u64}),
            known(Some(2_147_483_648), Some(2_147_483_648), None, None, None),
        ),
        (
            json!({"input_tokens": -1, "output_tokens": 3}),
            known(None, Some(3), None, None, None),
        ),
        (
            json!({"input_tokens": 7, "output_tokens": -2_147_483_648_i64}),
            known(Some(7), None, None, None, None),
        ),
        // Counters that contradict each other are all unknown.
        (
            json!({"input_tokens": 10, "input_tokens_details": {"cached_tokens": 11}}),
            Usage::default(),
        ),
        (
            json!({
                "input_tokens": 10, "output_tokens": 1,
                "output_tokens_details": {"reasoning_tokens": 2}
            }),
            Usage::default(),
        ),
    ] {
        let response = decide(vec![with_usage(usage.clone())], &request)
            .await
            .unwrap();
        assert_eq!(response.usage, expected, "{usage}");
    }
    // A count past 64 bits is read as a double, so it is no count.
    let past = EXAMPLE_RESPONSE.replace(
        r#""input_tokens": 42,"#,
        r#""input_tokens": 18446744073709551616,"#,
    );
    let response = decide(vec![Answer::Body(past)], &request).await.unwrap();
    assert_eq!(response.usage.input_tokens, None);
    assert_eq!(response.usage.output_tokens, Some(0));
    // And one written with an exponent.
    let exponent = EXAMPLE_RESPONSE.replace(r#""input_tokens": 42,"#, r#""input_tokens": 4e1,"#);
    let response = decide(vec![Answer::Body(exponent)], &request)
        .await
        .unwrap();
    assert_eq!(response.usage.input_tokens, None);
}

#[tokio::test]
async fn a_request_field_outside_the_reference_is_refused_by_name() {
    let with = |pointer: &[&str], member: &str, value: Value| {
        let mut request: Value = serde_json::from_str(EXAMPLE_REQUEST).unwrap();
        let mut at = &mut request;
        for step in pointer {
            at = match step.parse::<usize>() {
                Ok(index) => &mut at[index],
                Err(_) => &mut at[*step],
            };
        }
        at[member] = value;
        request.to_string()
    };
    let refusal = |text: String| {
        let error = DecisionRequest::from_json(&text).expect_err(&text);
        (error.code, error.message)
    };
    let unnamed = |path: &str| {
        (
            ErrorCode::InvalidRequest,
            format!("decision request carries a member the reference does not name: {path}"),
        )
    };
    assert_eq!(
        refusal(with(&[], "temperature", json!(0))),
        unnamed("temperature")
    );
    assert_eq!(
        refusal(with(&[], "stream", json!(false))),
        unnamed("stream")
    );
    assert_eq!(
        refusal(with(&["questions", "0"], "threshold", json!(0.5))),
        unnamed("questions[0].threshold")
    );
    // `choices` belongs to a choice question, not to a predicate.
    assert_eq!(
        refusal(with(&["questions", "0"], "choices", json!([]))),
        unnamed("questions[0].choices")
    );
    assert_eq!(
        refusal(with(
            &[],
            "prompt_cache_options",
            json!({"ttl": "30m", "scope": "org"})
        )),
        unnamed("prompt_cache_options.scope")
    );
    let asking_one = |question: Value| {
        json!({"model": "decide", "input": "x", "questions": [question]}).to_string()
    };
    let choice_question = json!({
        "type": "choice", "instructions": "Which?",
        "choices": [{"value": "a"}, {"value": "b", "weight": 2}]
    });
    assert_eq!(
        refusal(asking_one(choice_question)),
        unnamed("questions[0].choices[1].weight")
    );
    let score_question = json!({
        "type": "score", "instructions": "How?",
        "levels": [{"label": "a", "value": 0}, {"label": "b"}]
    });
    assert_eq!(
        refusal(asking_one(score_question)),
        unnamed("questions[0].levels[0].value")
    );
    // A member name is written only when it is short and plain; a value never is.
    let plain = "x".repeat(64);
    assert_eq!(refusal(with(&[], &plain, json!(1))), unnamed(&plain));
    // Plain is every ASCII letter of either case, every digit, `_` and `-`.
    for name in ["Top_P-2", "0", "_", "-", "Z9", "aZ09_-"] {
        assert_eq!(refusal(with(&[], name, json!(1))), unnamed(name));
    }
    for name in [
        "x".repeat(65),
        "a b".to_owned(),
        "a\nb".to_owned(),
        String::new(),
        // A letter or a digit outside ASCII is not plain, however short the name.
        "é".to_owned(),
        "é".repeat(32),
        "٣".to_owned(),
        // Nor is punctuation a path is written with.
        "a.b".to_owned(),
        "a[0]".to_owned(),
        "a\"b".to_owned(),
    ] {
        assert_eq!(
            refusal(with(&[], &name, json!(1))),
            unnamed("(a name not written)"),
            "{name:?}"
        );
    }
    let (_, message) = refusal(with(&[], "extra", json!("marker-quokka-7731")));
    assert!(!message.contains("marker-quokka-7731"));

    // Of several members the reference does not name, the first in name order is the one
    // named, wherever the document wrote it; a known member between them changes nothing.
    let several = r#"{"zeta": 1, "model": "decide", "beta": 2, "input": "x", "alpha": 3,
        "questions": [{"type": "predicate", "instructions": "Is it so?", "y": 1, "b": 2}]}"#;
    assert_eq!(refusal(several.to_owned()), unnamed("alpha"));
    let inner = several
        .replace(r#""zeta": 1, "#, "")
        .replace(r#""beta": 2, "#, "")
        .replace(r#""alpha": 3,"#, "");
    assert_eq!(refusal(inner), unnamed("questions[0].b"));
}

#[test]
#[expect(clippy::too_many_lines, reason = "one table of documents")]
fn a_request_is_read_from_one_whole_document_of_the_reference_shape() {
    let refusal = |text: &str| {
        let error = DecisionRequest::from_json(text).expect_err(text);
        (error.code, error.message)
    };
    let invalid = |message: &str| (ErrorCode::InvalidRequest, message.to_owned());
    let whole = invalid(
        "decision request is not one whole JSON document with every member once, nested at most 16 deep",
    );
    let question = r#"{"type":"predicate","instructions":"Is it so?"}"#;
    let request =
        |members: &str| format!(r#"{{"model":"decide","questions":[{question}]{members}}}"#);

    assert_eq!(
        DecisionRequest::from_json(&request(r#","input":"x""#)).unwrap(),
        DecisionRequest::new(
            "decide",
            "x",
            vec![Question::Predicate {
                name: None,
                instructions: "Is it so?".to_owned()
            }]
        )
    );
    // Cut short, followed by anything, a member twice.
    let good = request(r#","input":"x""#);
    assert_eq!(refusal(&good[..good.len() - 1]), whole);
    assert_eq!(refusal(&format!("{good}{good}")), whole);
    assert_eq!(refusal(&format!("{good} x")), whole);
    assert_eq!(refusal(&request(r#","input":"x","input":"x""#)), whole);
    assert_eq!(refusal(""), whole);
    assert_eq!(
        refusal("[]"),
        invalid("decision request holds a member of the wrong type: (document)")
    );
    // Required members and their types.
    assert_eq!(
        refusal(&request("")),
        invalid("decision request lacks a member the reference requires: input")
    );
    assert_eq!(
        refusal(&request(r#","input":7"#)),
        invalid("decision request holds a member of the wrong type: input")
    );
    assert_eq!(
        refusal(r#"{"input":"x","questions":[]}"#),
        invalid("decision request lacks a member the reference requires: model")
    );
    assert_eq!(
        refusal(r#"{"model":"decide","input":"x"}"#),
        invalid("decision request lacks a member the reference requires: questions")
    );
    assert_eq!(
        refusal(r#"{"model":"decide","input":"x","questions":[{"instructions":"?"}]}"#),
        invalid("decision request lacks a member the reference requires: questions[0].type")
    );
    assert_eq!(
        refusal(r#"{"model":"decide","input":"x","questions":[{"type":"predicate"}]}"#),
        invalid(
            "decision request lacks a member the reference requires: questions[0].instructions"
        )
    );
    assert_eq!(
        refusal(
            r#"{"model":"decide","input":"x","questions":[{"type":"rank","instructions":"?"}]}"#
        ),
        invalid("decision request names a value outside the reference: questions[0].type")
    );
    assert_eq!(
        refusal(
            r#"{"model":"decide","input":"x","questions":[{"type":"predicate","name":null,"instructions":"?"}]}"#
        ),
        invalid("decision request holds a member of the wrong type: questions[0].name")
    );
    assert_eq!(
        refusal(
            r#"{"model":"decide","input":"x","questions":[{"type":"choice","instructions":"?","choices":[{"value":"a"},{"value":1}]}]}"#
        ),
        invalid("decision request holds a member of the wrong type: questions[0].choices[1].value")
    );
    assert_eq!(
        refusal(
            r#"{"model":"decide","input":"x","questions":[{"type":"score","instructions":"?","levels":[{"description":"a"}]}]}"#
        ),
        invalid(
            "decision request lacks a member the reference requires: questions[0].levels[0].label"
        )
    );
    // A form the reference names and this client does not send.
    let unsent = |path: &str| {
        (
            ErrorCode::Unsupported,
            format!("decision request uses a form this client does not send: {path}"),
        )
    };
    assert_eq!(
        refusal(&request(
            r#","input":[{"type":"message","role":"user","content":"x"}]"#
        )),
        unsent("input")
    );
    assert_eq!(
        refusal(
            r#"{"model":"decide","input":"x","questions":[{"type":"predicate","instructions":[{"type":"input_text","text":"?"}]}]}"#
        ),
        unsent("questions[0].instructions")
    );
    // A document of the reference's shape is held to the reference's bounds as well.
    assert_eq!(
        refusal(r#"{"model":"decide","input":"x","questions":[]}"#),
        invalid("decision request is outside the reference's bounds: questions")
    );
    assert_eq!(
        refusal(
            r#"{"model":"decide","input":"x","questions":[{"type":"choice","instructions":"?","choices":[{"value":"a"},{"value":"a"}]}]}"#
        ),
        invalid(
            "decision request is outside the reference's bounds: questions[0].choices[1].value"
        )
    );
    // Nesting: the document is level 1, `input` given as items opens level 2.
    let nested = |arrays: usize| {
        request(&format!(
            r#","input":{}{}"#,
            "[".repeat(arrays),
            "]".repeat(arrays)
        ))
    };
    assert_eq!(refusal(&nested(15)), unsent("input"));
    assert_eq!(refusal(&nested(16)), whole);

    // The nullable members: null is the member left out.
    let nullable = DecisionRequest::from_json(&request(
        r#","input":"x","safety_identifier":null,"prompt_cache_key":null,"prompt_cache_options":null"#,
    ))
    .unwrap();
    assert_eq!(nullable.safety_identifier, None);
    assert_eq!(nullable.prompt_cache_options, None);
    let options = DecisionRequest::from_json(&request(
        r#","input":"x","safety_identifier":"u","prompt_cache_key":"k","prompt_cache_options":{"ttl":"30m","mode":null}"#,
    ))
    .unwrap();
    assert_eq!(options.safety_identifier.as_deref(), Some("u"));
    assert_eq!(options.prompt_cache_key.as_deref(), Some("k"));
    assert_eq!(
        options.prompt_cache_options,
        Some(PromptCacheOptions {
            ttl: Some(PromptCacheTtl::ThirtyMinutes),
            mode: None
        })
    );
    assert_eq!(
        refusal(&request(
            r#","input":"x","prompt_cache_options":{"ttl":"1h"}"#
        )),
        invalid("decision request names a value outside the reference: prompt_cache_options.ttl")
    );
    assert_eq!(
        refusal(&request(
            r#","input":"x","prompt_cache_options":{"mode":"auto"}"#
        )),
        invalid("decision request names a value outside the reference: prompt_cache_options.mode")
    );
    assert_eq!(
        refusal(&request(
            r#","input":"x","prompt_cache_options":{"ttl":null}"#
        )),
        invalid("decision request holds a member of the wrong type: prompt_cache_options.ttl")
    );
    assert_eq!(
        refusal(&request(r#","input":"x","safety_identifier":7"#)),
        invalid("decision request holds a member of the wrong type: safety_identifier")
    );
    // A document read is held to the bounds too.
    assert_eq!(
        refusal(r#"{"model":"decide","input":"x","questions":[]}"#),
        invalid("decision request is outside the reference's bounds: questions")
    );
}

#[test]
#[expect(clippy::too_many_lines, reason = "one table of bounds")]
fn a_request_is_held_to_the_bounds_of_the_reference() {
    let outside = |request: &DecisionRequest| {
        let error = request.validate().expect_err("outside its bounds");
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(error.dispatch, Dispatch::NotSent);
        error
            .message
            .strip_prefix("decision request is outside the reference's bounds: ")
            .expect("the fixed message")
            .to_owned()
    };
    let questions = |count: usize| asking(vec![predicate("p"); count]);
    // 1 to 400 questions.
    assert_eq!(outside(&questions(0)), "questions");
    assert!(questions(1).validate().is_ok());
    assert!(questions(400).validate().is_ok());
    assert_eq!(outside(&questions(401)), "questions");

    // 2 to 255 choices, each typed value once.
    let choices = |count: usize| {
        let texts: Vec<String> = (0..count).map(|index| format!("c{index}")).collect();
        let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
        asking(vec![predicate("p"), choice(&texts)])
    };
    assert_eq!(outside(&choices(0)), "questions[1].choices");
    assert_eq!(outside(&choices(1)), "questions[1].choices");
    assert!(choices(2).validate().is_ok());
    assert!(choices(255).validate().is_ok());
    assert_eq!(outside(&choices(256)), "questions[1].choices");
    assert_eq!(
        outside(&asking(vec![choice(&["a", "b", "a"])])),
        "questions[0].choices[2].value"
    );
    // Values are distinct within one question, not across questions: two questions may offer
    // the same choices, and a repeat in the third is named in the third.
    assert!(
        asking(vec![choice(&["a", "b"]), choice(&["a", "b"])])
            .validate()
            .is_ok()
    );
    assert_eq!(
        outside(&asking(vec![
            choice(&["a", "b"]),
            choice(&["b", "c"]),
            choice(&["c", "d", "c"])
        ])),
        "questions[2].choices[2].value"
    );
    let typed = |values: Vec<ChoiceValue>| {
        asking(vec![Question::Choice {
            name: None,
            instructions: String::new(),
            choices: values
                .into_iter()
                .map(|value| ChoiceOption {
                    value,
                    description: None,
                })
                .collect(),
        }])
    };
    let text = |value: &str| ChoiceValue::Text(value.to_owned());
    assert!(
        typed(vec![
            text("true"),
            ChoiceValue::Flag(true),
            ChoiceValue::Flag(false)
        ])
        .validate()
        .is_ok(),
        "the text and the boolean are distinct, and so are the two booleans"
    );
    assert_eq!(
        outside(&typed(vec![
            ChoiceValue::Flag(true),
            ChoiceValue::Flag(true)
        ])),
        "questions[0].choices[1].value"
    );
    assert!(typed(vec![text(""), text(" ")]).validate().is_ok());

    // 2 to 10 levels.
    let levels = |count: usize| {
        asking(vec![Question::Score {
            name: None,
            instructions: String::new(),
            levels: (0..count)
                .map(|index| ScoreLevel {
                    label: format!("l{index}"),
                    description: None,
                })
                .collect(),
        }])
    };
    assert_eq!(outside(&levels(1)), "questions[0].levels");
    assert!(levels(2).validate().is_ok());
    assert!(levels(10).validate().is_ok());
    assert_eq!(outside(&levels(11)), "questions[0].levels");

    // Text lengths count characters, not bytes: "é" is two bytes and one character.
    let long = |characters: usize| "é".repeat(characters);
    let named = |name: String| {
        asking(vec![Question::Predicate {
            name: Some(name),
            instructions: String::new(),
        }])
    };
    assert!(named(long(1_048_576)).validate().is_ok());
    assert_eq!(outside(&named(long(1_048_577))), "questions[0].name");
    let instructed = |instructions: String| {
        asking(vec![Question::Predicate {
            name: None,
            instructions,
        }])
    };
    assert!(instructed(long(1_048_576)).validate().is_ok());
    assert_eq!(
        outside(&instructed(long(1_048_577))),
        "questions[0].instructions"
    );
    let valued = |value: String| typed(vec![text("a"), ChoiceValue::Text(value)]);
    assert!(valued(long(1_048_576)).validate().is_ok());
    assert_eq!(
        outside(&valued(long(1_048_577))),
        "questions[0].choices[1].value"
    );
    let described = |description: String| {
        asking(vec![Question::Choice {
            name: None,
            instructions: String::new(),
            choices: vec![
                ChoiceOption {
                    value: text("a"),
                    description: Some(description),
                },
                ChoiceOption::text("b"),
            ],
        }])
    };
    assert!(described(long(1_048_576)).validate().is_ok());
    assert_eq!(
        outside(&described(long(1_048_577))),
        "questions[0].choices[0].description"
    );
    let leveled = |label: String, description: String| {
        asking(vec![Question::Score {
            name: None,
            instructions: String::new(),
            levels: vec![
                ScoreLevel {
                    label: "a".to_owned(),
                    description: None,
                },
                ScoreLevel {
                    label,
                    description: Some(description),
                },
            ],
        }])
    };
    assert!(leveled(long(1_048_576), long(1_048_576)).validate().is_ok());
    assert_eq!(
        outside(&leveled(long(1_048_577), String::new())),
        "questions[0].levels[1].label"
    );
    assert_eq!(
        outside(&leveled(String::new(), long(1_048_577))),
        "questions[0].levels[1].description"
    );

    let mut request = questions(1);
    request.input = long(10_485_760);
    assert!(request.validate().is_ok());
    request.input = long(10_485_761);
    assert_eq!(outside(&request), "input");

    let mut request = questions(1);
    request.safety_identifier = Some(long(128));
    request.prompt_cache_key = Some(long(64));
    assert!(request.validate().is_ok());
    request.safety_identifier = Some(long(129));
    assert_eq!(outside(&request), "safety_identifier");
    request.safety_identifier = None;
    request.prompt_cache_key = Some(long(65));
    assert_eq!(outside(&request), "prompt_cache_key");
}

#[tokio::test]
async fn a_request_outside_its_bounds_or_its_wire_is_refused_before_io_and_writes_no_record() {
    let (address, seen) = serve(vec![Answer::Body(EXAMPLE_RESPONSE.to_owned())]).await;
    let catalog = catalog(address);
    let log = CallLog::new();
    let clock = Frozen(0);
    let recording = Recording {
        sink: &log,
        clock: &clock,
        prices: None,
    };
    let identity = || CallIdentity::primary(id("req-1"));
    let client = client();

    // No question.
    let empty = asking(Vec::new());
    let selection = OpenAiDecisions::select(&catalog, &empty, Some(100)).unwrap();
    let error = client
        .decide_recorded(&selection, &empty, &Cancel::new(), identity(), recording)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert_eq!(error.dispatch, Dispatch::NotSent);

    // Already cancelled.
    let request = example();
    let cancel = Cancel::new();
    cancel.cancel();
    let error = client
        .decide_recorded(&selection, &request, &cancel, identity(), recording)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);

    // A target served over the messages protocol has no decision endpoint.
    let messages = Catalog::parse(&catalog_toml(address, "messages", ANONYMOUS)).unwrap();
    let selection = OpenAiDecisions::select(&messages, &request, Some(100)).unwrap();
    let error = client
        .decide_recorded(&selection, &request, &Cancel::new(), identity(), recording)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unsupported);

    // An unknown alias and an embed route select nothing.
    let unknown = DecisionRequest::new("no-such-route", "x", vec![predicate("p")]);
    assert_eq!(
        OpenAiDecisions::select(&catalog, &unknown, Some(100))
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    let embed = DecisionRequest::new("embed", "x", vec![predicate("p")]);
    assert_eq!(
        OpenAiDecisions::select(&catalog, &embed, Some(100))
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        seen.connections.load(Ordering::SeqCst),
        0,
        "nothing was sent"
    );
    assert!(log.records().is_empty(), "{:?}", log.records());
}

#[tokio::test]
async fn an_encoded_body_at_the_transport_bound_is_sent_and_one_byte_more_is_refused() {
    const BOUND: usize = 16 * 1024 * 1024;
    const QUESTIONS: usize = 6;
    const INPUT: usize = 10_485_760;
    const INSTRUCTIONS: usize = 1_048_576;
    let answers = vec![json!({"type": "predicate", "name": "p", "probability": 0.5}); QUESTIONS];
    let (address, seen) = serve(vec![body(&answering(&json!(answers)))]).await;
    let catalog = catalog(address);
    // The input at its bound and six instructions carry the bulk: five at their bound of
    // 1048576 characters, the last as long as `last`.
    let sized = |input: usize, full: usize, last: usize| {
        let mut request =
            DecisionRequest::new("decide", "i".repeat(input), vec![predicate("p"); QUESTIONS]);
        for (index, question) in request.questions.iter_mut().enumerate() {
            if let Question::Predicate { instructions, .. } = question {
                *instructions = "q".repeat(if index + 1 == QUESTIONS { last } else { full });
            }
        }
        request
    };
    let selection = OpenAiDecisions::select(&catalog, &sized(0, 0, 0), Some(100)).unwrap();
    client()
        .decide(&selection, &sized(0, 0, 0), &Cancel::new())
        .await
        .unwrap();
    let overhead = seen.requests()[0].body.len();
    let last = BOUND - overhead - INPUT - (QUESTIONS - 1) * INSTRUCTIONS;
    assert!(
        last < INSTRUCTIONS,
        "the last instructions stay inside their own bound"
    );

    let at = sized(INPUT, INSTRUCTIONS, last);
    client()
        .decide(&selection, &at, &Cancel::new())
        .await
        .unwrap();
    assert_eq!(seen.requests()[1].body.len(), BOUND);

    let over = sized(INPUT, INSTRUCTIONS, last + 1);
    let error = client()
        .decide(&selection, &over, &Cancel::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::TooLarge);
    assert_eq!(error.dispatch, Dispatch::NotSent);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        seen.requests().len(),
        2,
        "the body over the bound was not sent"
    );
}

#[tokio::test]
async fn a_decision_endpoint_call_yields_a_call_record_priced_from_the_cost_table() {
    // The reference's example usage: 42 input tokens, none cached, no output.
    let (result, records, _) = recorded(
        vec![Answer::Body(EXAMPLE_RESPONSE.to_owned())],
        &example(),
        prices,
    )
    .await;
    result.unwrap();
    assert_eq!(records.len(), 1, "{records:?}");
    let record = &records[0];
    assert_eq!(record.id, id("req-1"));
    assert_eq!(record.kind, Kind::Classification);
    assert_eq!(record.route_id, id("deciding"));
    assert_eq!(record.route_target_id, Some(id("decide-primary")));
    assert_eq!(record.upstream_model, Some(id("gpt-6-luna")));
    assert_eq!(record.outcome, Outcome::Succeeded);
    assert_eq!(record.error_code, None);
    assert_eq!(record.dispatch, None);
    assert_eq!(record.attempts, 1);
    assert_eq!(record.started_at_unix_ms, 1_700_000_000_000);
    assert_eq!(record.latency_ms, Some(0));
    assert_eq!(record.usage.input_tokens, Some(42));
    let cost = record.cost.as_ref().expect("a priced call has a cost");
    assert!(cost.complete, "{cost:?}");
    assert_eq!(cost.known_subtotal, Amount::parse("0.042").unwrap());
    assert_eq!(cost.price_revision, id("prices-r1"));

    // Cached and written input and output are priced at their own rates:
    // (1000 - 400 - 100) * 1 + 400 * 0.5 + 100 * 1 + 50 * 2, per 1000 tokens.
    let mut answer: Value = serde_json::from_str(EXAMPLE_RESPONSE).unwrap();
    answer["usage"] = json!({
        "input_tokens": 1000,
        "input_tokens_details": {"cached_tokens": 400, "cache_write_tokens": 100},
        "output_tokens": 50,
        "output_tokens_details": {"reasoning_tokens": 0},
        "total_tokens": 1050
    });
    let (_, records, _) = recorded(vec![body(&answer)], &example(), prices).await;
    let cost = records[0].cost.as_ref().unwrap();
    assert!(cost.complete, "{cost:?}");
    assert_eq!(cost.known_subtotal, Amount::parse("0.9").unwrap());

    // An unreported counter leaves its line unknown: the cost is incomplete, a lower bound.
    answer["usage"] = json!({"input_tokens": 1000});
    let (_, records, _) = recorded(vec![body(&answer)], &example(), prices).await;
    assert_eq!(records[0].usage.output_tokens, None);
    let cost = records[0].cost.as_ref().unwrap();
    assert!(!cost.complete, "{cost:?}");
    // No line is known: the input is priced only with its cache split, which is unreported.
    assert_eq!(cost.known_subtotal, Amount::parse("0").unwrap());
    // With the split and no output count, the input lines are known and the output line is not.
    answer["usage"] = json!({
        "input_tokens": 1000,
        "input_tokens_details": {"cached_tokens": 400, "cache_write_tokens": 100}
    });
    let (_, records, _) = recorded(vec![body(&answer)], &example(), prices).await;
    let cost = records[0].cost.as_ref().unwrap();
    assert!(!cost.complete, "{cost:?}");
    assert_eq!(cost.known_subtotal, Amount::parse("0.8").unwrap());

    // Without a price book the record carries usage and no cost.
    let (_, records, _) = recorded(
        vec![Answer::Body(EXAMPLE_RESPONSE.to_owned())],
        &example(),
        no_prices,
    )
    .await;
    assert_eq!(records[0].cost, None);
    assert_eq!(records[0].usage.input_tokens, Some(42));
}

#[tokio::test]
async fn a_refused_or_failed_call_is_recorded_with_what_the_body_reported() {
    // An answer refused for its type still records the model and the tokens the body named.
    let unknown = EXAMPLE_RESPONSE.replace(r#""type": "predicate""#, r#""type": "ranking""#);
    let (result, records, _) = recorded(vec![Answer::Body(unknown)], &example(), prices).await;
    let error = result.unwrap_err();
    assert_eq!(error.code, ErrorCode::Protocol);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].outcome, Outcome::Failed);
    assert_eq!(records[0].error_code, Some(ErrorCode::Protocol));
    assert_eq!(records[0].dispatch, Some(Dispatch::Accepted));
    assert_eq!(records[0].upstream_model, Some(id("gpt-6-luna")));
    assert_eq!(records[0].usage.input_tokens, Some(42));
    let cost = records[0].cost.as_ref().expect("the tokens were spent");
    assert_eq!(cost.known_subtotal, Amount::parse("0.042").unwrap());

    // A body that is not whole reports nothing.
    let cut = EXAMPLE_RESPONSE[..EXAMPLE_RESPONSE.len() - 1].to_owned();
    let (result, records, _) = recorded(vec![Answer::Body(cut)], &example(), no_prices).await;
    assert_eq!(result.unwrap_err().code, ErrorCode::Protocol);
    assert_eq!(records[0].usage, Usage::default());
    assert_eq!(records[0].upstream_model, None);

    // A refusal by status: one record, one request, nothing retried.
    let (result, records, seen) = recorded(vec![Answer::Status(429)], &example(), no_prices).await;
    let error = result.unwrap_err();
    assert_eq!(error.code, ErrorCode::RateLimited);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].outcome, Outcome::Failed);
    assert_eq!(records[0].error_code, Some(ErrorCode::RateLimited));
    assert_eq!(records[0].dispatch, Some(error.dispatch));
    assert_eq!(records[0].attempts, 1);
    assert_eq!(seen.requests().len(), 1);
    let (result, _, _) = recorded(vec![Answer::Status(401)], &example(), no_prices).await;
    assert_eq!(result.unwrap_err().code, ErrorCode::Unauthorized);
}

#[tokio::test]
async fn a_cancelled_call_is_recorded_cancelled() {
    let (address, _) = serve(vec![Answer::Hang]).await;
    let catalog = catalog(address);
    let request = example();
    let selection = OpenAiDecisions::select(&catalog, &request, Some(100)).unwrap();
    let log = CallLog::new();
    let clock = Frozen(0);
    let cancel = Cancel::new();
    let stopper = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        stopper.cancel();
    });
    let error = client()
        .decide_recorded(
            &selection,
            &request,
            &cancel,
            CallIdentity::primary(id("req-1")),
            Recording {
                sink: &log,
                clock: &clock,
                prices: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);
    let records = log.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].outcome, Outcome::Cancelled);
    // The record carries the call's error code and the dispatch the error reports.
    assert_eq!(records[0].error_code, Some(ErrorCode::Cancelled));
    assert_eq!(records[0].dispatch, Some(error.dispatch));
    assert_ne!(error.dispatch, Dispatch::NotSent, "the request had left");
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
    let (address, seen) = serve(vec![Answer::Body(EXAMPLE_RESPONSE.to_owned())]).await;
    let catalog = Catalog::parse(&catalog_toml(
        address,
        "chat-completions",
        "auth_kind = \"bearer\"\nbilling_kind = \"metered\"\nsecret_reference_id = \"decide-token\"",
    ))
    .unwrap();
    let request = example();
    let selection = OpenAiDecisions::select(&catalog, &request, Some(100)).unwrap();
    OpenAiDecisions::new(http(), Arc::new(Fixed))
        .decide(&selection, &request, &Cancel::new())
        .await
        .unwrap();
    assert_eq!(
        seen.requests()[0].authorization.as_deref(),
        Some("Bearer fixture-token")
    );
    // The token is no part of the body.
    assert!(!String::from_utf8_lossy(&seen.requests()[0].body).contains("fixture-token"));
}

/// A clock that stands where a test put it: the held milliseconds after its start.
struct Moved(AtomicU64);

impl Clock for Moved {
    fn unix_ms(&self) -> u64 {
        1_700_000_000_000 + self.0.load(Ordering::SeqCst)
    }
    fn monotonic(&self) -> Duration {
        Duration::from_millis(self.0.load(Ordering::SeqCst))
    }
}

/// Hands out the fixed token and takes 1500 ms of the clock to do it.
struct Slow(Arc<Moved>);

impl SecretResolver for Slow {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async {
            self.0.0.fetch_add(1500, Ordering::SeqCst);
            Ok(ResolvedSecret {
                secret: Secret::new(b"fixture-token".to_vec())?,
                version: SecretVersion::new("v1".to_owned())?,
            })
        })
    }
}

/// Every other fixture's clock is frozen, so its latency is 0 wherever the stopwatch sits.
#[tokio::test]
async fn the_record_times_the_whole_call_from_before_its_credential_is_resolved() {
    let (address, _) = serve(vec![Answer::Body(EXAMPLE_RESPONSE.to_owned())]).await;
    let catalog = Catalog::parse(&catalog_toml(
        address,
        "responses",
        "auth_kind = \"bearer\"\nbilling_kind = \"metered\"\nsecret_reference_id = \"decide-token\"",
    ))
    .unwrap();
    let request = example();
    let selection = OpenAiDecisions::select(&catalog, &request, Some(100)).unwrap();
    let clock = Arc::new(Moved(AtomicU64::new(250)));
    let log = CallLog::new();
    OpenAiDecisions::new(http(), Arc::new(Slow(clock.clone())))
        .decide_recorded(
            &selection,
            &request,
            &Cancel::new(),
            CallIdentity::primary(id("req-1")),
            Recording {
                sink: &log,
                clock: &*clock,
                prices: None,
            },
        )
        .await
        .unwrap();
    let records = log.records();
    assert_eq!(records.len(), 1);
    // Started before the credential was asked for, stopped after it: the 1500 ms are the call's.
    assert_eq!(records[0].started_at_unix_ms, 1_700_000_000_250);
    assert_eq!(records[0].latency_ms, Some(1500));
}

/// Every other fixture's base URL ends in `/v1`.
#[tokio::test]
async fn the_request_goes_to_the_endpoint_base_url_followed_by_decisions() {
    for (base, path) in [
        ("/v1/", "/v1/decisions"),
        ("/gateway/openai/v1", "/gateway/openai/v1/decisions"),
        ("/gateway/openai/v1/", "/gateway/openai/v1/decisions"),
        ("", "/decisions"),
        ("/", "/decisions"),
        ("/v2", "/v2/decisions"),
    ] {
        let (address, seen) = serve(vec![Answer::Body(EXAMPLE_RESPONSE.to_owned())]).await;
        let declared = format!("base_url = \"http://{address}{base}\"");
        let toml = catalog_toml(address, "responses", ANONYMOUS)
            .replace(&format!("base_url = \"http://{address}/v1\""), &declared);
        assert!(toml.contains(&declared), "{base:?}");
        let catalog = Catalog::parse(&toml).unwrap();
        let request = example();
        let selection = OpenAiDecisions::select(&catalog, &request, Some(100)).unwrap();
        client()
            .decide(&selection, &request, &Cancel::new())
            .await
            .unwrap();
        let requests = seen.requests();
        assert_eq!(requests.len(), 1, "{base:?}");
        assert_eq!(requests[0].path, path, "{base:?}");
    }
}

/// Every other fixture estimates 100 input tokens. The fixture's serving model has a context
/// window of 4096 tokens and 1024 of them for output.
#[test]
fn a_target_is_selected_against_the_callers_token_estimate() {
    let catalog = catalog("127.0.0.1:9".parse().unwrap());
    let request = example();
    assert!(OpenAiDecisions::select(&catalog, &request, Some(3072)).is_ok());
    for estimate in [Some(3073), Some(u64::MAX), None] {
        let error = OpenAiDecisions::select(&catalog, &request, estimate).unwrap_err();
        assert_eq!(error.code, ErrorCode::Unsupported, "{estimate:?}");
    }
    // A model that is no alias at all selects nothing either.
    let invalid = DecisionRequest::new("not an alias", "x", vec![predicate("p")]);
    assert_eq!(
        OpenAiDecisions::select(&catalog, &invalid, Some(100))
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
}

/// A body short of the length it declared is the transport's failure, not the reader's: the
/// client hands it on unchanged, asks once and records what the error reports.
#[tokio::test]
async fn a_body_the_transport_reports_as_cut_is_its_retriable_failure_and_is_recorded_so() {
    let (result, records, seen) = recorded(
        vec![Answer::Cut(EXAMPLE_RESPONSE.to_owned())],
        &example(),
        no_prices,
    )
    .await;
    let error = result.unwrap_err();
    assert_eq!(
        (error.code, error.dispatch, error.retriable),
        (ErrorCode::Transport, Dispatch::Accepted, true)
    );
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0].outcome, Outcome::Failed);
    assert_eq!(records[0].error_code, Some(ErrorCode::Transport));
    assert_eq!(records[0].dispatch, Some(Dispatch::Accepted));
    assert_eq!(records[0].attempts, 1);
    // Nothing is read from a body that did not arrive whole.
    assert_eq!(records[0].usage, Usage::default());
    assert_eq!(records[0].upstream_model, None);
    assert_eq!(
        seen.requests().len(),
        1,
        "retrying is the caller's: one request"
    );

    // The classifier abstains with the transport's code.
    let classified = classify(
        vec![Answer::Cut(EXAMPLE_RESPONSE.to_owned())],
        labels(&["angry", "calm"]),
        LONG,
        None,
    )
    .await;
    assert_eq!(
        cause(&classified),
        Some(AbstentionCause::ProviderError(ErrorCode::Transport))
    );
    assert_eq!(classified.records.len(), 1);
    assert_eq!(classified.seen.requests().len(), 1);
}

// The classifier's decision-endpoint path through the real client.

struct NoModels;

impl Models for NoModels {
    fn model(&self, _: &Id) -> Option<&dyn llm_core::Model> {
        None
    }
}

fn labels(labels: &[&str]) -> LabelSet {
    LabelSet::new(
        id("tone"),
        "v1",
        labels.iter().map(|label| (*label).to_owned()).collect(),
    )
    .unwrap()
}

struct Classified {
    outcome: ChainOutcome,
    records: Vec<CallRecord>,
    seen: Arc<Seen>,
}

/// One chain of one decision-endpoint classifier `endpoint` over `set`, against a fixture that
/// gives `answers`, within `deadline`.
async fn classify(
    answers: Vec<Answer>,
    set: LabelSet,
    deadline: Duration,
    floor: Option<u16>,
) -> Classified {
    let (address, seen) = serve(answers).await;
    let catalog = catalog(address);
    let mut classifier = EndpointClassifier::new(id("endpoint"), "decide", client())
        .with_instructions("Which tone does the text have?");
    if let Some(bps) = floor {
        classifier = classifier.with_min_confidence_bps(bps).unwrap();
    }
    let chain = Chain::new(id("chain"), set, deadline, vec![Box::new(classifier)]).unwrap();
    let log = CallLog::new();
    let clock = Frozen(1_700_000_000_000);
    let cancel = Cancel::new();
    let admit = |_: &Selection<'_>| Ok(());
    let outcome = chain
        .classify(
            &id("req-1"),
            "The package arrived with a broken screen.",
            Some(100),
            Ports {
                catalog: &catalog,
                models: &NoModels,
                admit: &admit,
                cancel: &cancel,
                pause: &|wait| Box::pin(tokio::time::sleep(wait)),
                recording: Recording {
                    sink: &log,
                    clock: &clock,
                    prices: None,
                },
            },
        )
        .await;
    Classified {
        outcome,
        records: log.records(),
        seen,
    }
}

fn choice_answer(choice: &Value, probabilities: &Value) -> Answer {
    body(&answering(&json!([{
        "type": "choice", "name": null, "choice": choice,
        "probabilities": probabilities, "confidence": 0.5
    }])))
}

fn cause(classified: &Classified) -> Option<AbstentionCause> {
    match classified.outcome.result()? {
        Classification::Decided(_) => None,
        Classification::Abstained(abstention) => Some(abstention.cause),
    }
}

const LONG: Duration = Duration::from_secs(20);

#[tokio::test]
async fn a_choice_answer_becomes_a_decision_with_a_confidence_per_label() {
    let answer = choice_answer(
        &json!("angry"),
        &json!([
            {"value": "angry", "probability": 0.62},
            {"value": "calm", "probability": 0.03125}
        ]),
    );
    let classified = classify(vec![answer], labels(&["angry", "calm", "glad"]), LONG, None).await;

    // One choice question, unnamed, the labels as text values in the set's order.
    assert_eq!(
        classified.seen.bodies(),
        [json!({
            "model": "gpt-6-luna",
            "input": "The package arrived with a broken screen.",
            "questions": [{
                "type": "choice",
                "instructions": "Which tone does the text have?",
                "choices": [{"value": "angry"}, {"value": "calm"}, {"value": "glad"}]
            }]
        })]
    );
    assert_eq!(classified.seen.requests()[0].path, "/v1/decisions");
    let decision = classified
        .outcome
        .decision()
        .expect("a label of the set decides");
    assert_eq!(decision.label, "angry");
    let bps = |label: &str, confidence_bps| LabelConfidence {
        label: label.to_owned(),
        confidence_bps,
    };
    assert_eq!(
        decision.confidences,
        [
            bps("angry", Some(6200)),
            bps("calm", Some(313)),
            bps("glad", None)
        ]
    );
    assert_eq!(decision.call_record_id, Some(id("req-1/endpoint")));
    assert_eq!(classified.records.len(), 1);
    assert_eq!(classified.records[0].id, id("req-1/endpoint"));
    assert_eq!(classified.records[0].kind, Kind::Classification);
    assert_eq!(classified.records[0].usage.input_tokens, Some(42));
}

#[tokio::test]
async fn a_refusal_answer_abstains_off_label() {
    let refusal = body(&answering(&json!([{"type": "refusal", "name": null}])));
    let classified = classify(vec![refusal], labels(&["angry", "calm"]), LONG, None).await;
    assert_eq!(cause(&classified), Some(AbstentionCause::OffLabel));
    let Some(Classification::Abstained(abstention)) = classified.outcome.result() else {
        panic!("a refusal decides nothing");
    };
    assert!(abstention.is_final);
    assert_eq!(abstention.call_record_id, Some(id("req-1/endpoint")));
    // The call itself succeeded: the endpoint answered.
    assert_eq!(classified.records[0].outcome, Outcome::Succeeded);
}

#[tokio::test]
async fn a_choice_answer_outside_the_label_set_or_the_probability_rule_abstains_off_label() {
    let set = || labels(&["angry", "calm"]);
    let weighed =
        |value: Value, probability: Value| json!([{"value": value, "probability": probability}]);
    for (choice, probabilities) in [
        // A choice that is no label, a boolean one, a boolean among the probabilities.
        (json!("furious"), json!([])),
        (json!(true), json!([])),
        (json!("angry"), weighed(json!(true), json!(0.5))),
        // A probability for a value outside the set; one label twice.
        (json!("angry"), weighed(json!("furious"), json!(0.5))),
        (
            json!("angry"),
            json!([{"value": "calm", "probability": 0.5}, {"value": "calm", "probability": 0.5}]),
        ),
        // A probability outside 0 to 1.
        (json!("angry"), weighed(json!("angry"), json!(1.0001))),
        (json!("angry"), weighed(json!("angry"), json!(-0.0001))),
        (json!("angry"), weighed(json!("angry"), json!(2))),
        (json!("angry"), weighed(json!("angry"), json!(1e300))),
    ] {
        let classified = classify(
            vec![choice_answer(&choice, &probabilities)],
            set(),
            LONG,
            None,
        )
        .await;
        assert_eq!(
            cause(&classified),
            Some(AbstentionCause::OffLabel),
            "{choice} {probabilities}"
        );
        assert_eq!(classified.records.len(), 1);
    }
    // The edges of the range decide: 0 is 0 and 1 is 10000 basis points; the sum is not checked.
    let edges =
        json!([{"value": "angry", "probability": 1}, {"value": "calm", "probability": 1.0}]);
    let classified = classify(
        vec![choice_answer(&json!("calm"), &edges)],
        set(),
        LONG,
        None,
    )
    .await;
    let decision = classified.outcome.decision().expect("the edges decide");
    assert_eq!(decision.confidences[0].confidence_bps, Some(10_000));
    assert_eq!(decision.confidences[1].confidence_bps, Some(10_000));
    let zero = weighed(json!("calm"), json!(0));
    let classified = classify(
        vec![choice_answer(&json!("calm"), &zero)],
        set(),
        LONG,
        None,
    )
    .await;
    let decision = classified
        .outcome
        .decision()
        .expect("a zero probability decides");
    assert_eq!(decision.confidences[1].confidence_bps, Some(0));
    assert_eq!(decision.confidences[0].confidence_bps, None);

    // A floor is met by the basis points, and an unknown confidence meets none.
    let at = weighed(json!("angry"), json!(0.62));
    let classified = classify(
        vec![choice_answer(&json!("angry"), &at)],
        set(),
        LONG,
        Some(6200),
    )
    .await;
    assert!(classified.outcome.decision().is_some());
    let classified = classify(
        vec![choice_answer(&json!("angry"), &at)],
        set(),
        LONG,
        Some(6201),
    )
    .await;
    assert_eq!(cause(&classified), Some(AbstentionCause::OffLabel));
    let classified = classify(
        vec![choice_answer(&json!("angry"), &json!([]))],
        set(),
        LONG,
        Some(1),
    )
    .await;
    assert_eq!(cause(&classified), Some(AbstentionCause::OffLabel));
}

#[tokio::test]
async fn an_unknown_answer_type_is_refused_by_its_path_and_abstains_with_its_code() {
    let unknown = body(&answering(&json!([{"type": "ranking", "name": null}])));
    let classified = classify(vec![unknown], labels(&["angry", "calm"]), LONG, None).await;
    assert_eq!(
        cause(&classified),
        Some(AbstentionCause::ProviderError(ErrorCode::Protocol))
    );
    assert_eq!(classified.records.len(), 1);
    assert_eq!(classified.records[0].error_code, Some(ErrorCode::Protocol));
    assert_eq!(classified.seen.requests().len(), 1, "one call, no retry");

    // A failed call abstains with its code as well.
    let classified = classify(
        vec![Answer::Status(429)],
        labels(&["angry", "calm"]),
        LONG,
        None,
    )
    .await;
    assert_eq!(
        cause(&classified),
        Some(AbstentionCause::ProviderError(ErrorCode::RateLimited))
    );
    assert_eq!(classified.seen.requests().len(), 1);
}

#[tokio::test]
async fn a_decision_endpoint_call_open_at_the_chain_deadline_abstains_timeout() {
    let classified = classify(
        vec![Answer::Hang],
        labels(&["angry", "calm"]),
        Duration::from_millis(100),
        None,
    )
    .await;
    assert_eq!(cause(&classified), Some(AbstentionCause::Timeout));
    assert_eq!(
        classified.records.len(),
        1,
        "the cancelled call is recorded"
    );
    assert_eq!(classified.records[0].outcome, Outcome::Cancelled);
}

#[tokio::test]
async fn a_label_set_larger_than_the_choice_bound_abstains_without_a_call() {
    let names: Vec<String> = (0..256).map(|index| format!("label-{index}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let answer = choice_answer(&json!("label-0"), &json!([]));

    // 256 labels are one more than the endpoint's 255 choices.
    let classified = classify(vec![answer.clone()], labels(&names), LONG, None).await;
    assert_eq!(
        cause(&classified),
        Some(AbstentionCause::ProviderError(ErrorCode::InvalidRequest))
    );
    let Some(Classification::Abstained(abstention)) = classified.outcome.result() else {
        panic!("a request refused before I/O decides nothing");
    };
    assert_eq!(abstention.call_record_id, None);
    assert!(classified.records.is_empty());
    assert!(classified.seen.requests().is_empty());

    // 255 labels are asked.
    let classified = classify(vec![answer], labels(&names[..255]), LONG, None).await;
    assert_eq!(
        classified
            .outcome
            .decision()
            .map(|decision| decision.label.as_str()),
        Some("label-0")
    );
    assert_eq!(
        classified.seen.bodies()[0]["questions"][0]["choices"]
            .as_array()
            .unwrap()
            .len(),
        255
    );
}

#[path = "client/adversary_w28_wire.rs"]
mod adversary_w28_wire;
