//! One signed attempt of `BedrockClient` against a local fixture server: how the request is
//! signed, which operation it names, what a target refuses, and the class of every way the
//! attempt fails, before and after the turn answered.
//!
//! Every event-stream frame here is synthetic, built with `eventstream::encode_message`; the
//! recorded answers are replayed in `tests/converse.rs`.

use llm_bedrock::{
    Api, AwsCredentials, BedrockClient, BedrockTarget, CredentialSource,
    eventstream::{HeaderValue, Message, encode_message},
};
use llm_core::{
    BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item, Model, Protocol,
    Provenance, StreamEvent, TurnOutcome, TurnRequest, VecSink,
};
use llm_http::{HttpClient, Limits};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use url::Url;

const MODEL: &str = "fixture-model";
const NOVA: &str = "us.amazon.nova-micro-v1:0";
const KEY_ID: &str = "AKIDFIXTURE";
const SECRET: &str = "fixture-secret-access-key";
const SESSION_TOKEN: &str = "fixture-session-token";
const EVENT_STREAM: &str = "application/vnd.amazon.eventstream";
/// An endpoint nothing listens on: a test that uses it expects no request to be sent.
const NOWHERE: &str = "http://127.0.0.1:9/";

/// What the fixture server read: the request line, the header fields with lower-cased names,
/// and the body.
struct Request {
    line: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.as_str())
    }

    fn count(&self, name: &str) -> usize {
        self.headers
            .iter()
            .filter(|(field, _)| field == name)
            .count()
    }
}

async fn read_request(socket: &mut TcpStream) -> Request {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    let end = loop {
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break end;
        }
        let read = socket.read(&mut buffer).await.unwrap();
        assert!(read > 0, "the client closed before sending a request");
        bytes.extend_from_slice(&buffer[..read]);
    };
    let head = String::from_utf8(bytes[..end].to_vec()).unwrap();
    let mut lines = head.split("\r\n");
    let line = lines.next().unwrap().to_owned();
    let headers: Vec<(String, String)> = lines
        .map(|field| {
            let (name, value) = field.split_once(':').unwrap();
            (name.trim().to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect();
    let length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .map_or(0, |(_, value)| value.parse().unwrap());
    let mut body = bytes[end + 4..].to_vec();
    while body.len() < length {
        let read = socket.read(&mut buffer).await.unwrap();
        assert!(read > 0, "the client closed inside its body");
        body.extend_from_slice(&buffer[..read]);
    }
    Request {
        line,
        headers,
        body,
    }
}

/// Serves one connection on 127.0.0.1: reads the request, writes `head`, then `body` in small
/// parts, then closes. Reports the request it read.
async fn serve(head: String, body: Vec<u8>) -> (Url, JoinHandle<Request>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let request = read_request(&mut socket).await;
        socket.write_all(head.as_bytes()).await.unwrap();
        for part in body.chunks(61) {
            // The client may already have gone, after a failure it found.
            if socket.write_all(part).await.is_err() || socket.flush().await.is_err() {
                break;
            }
        }
        request
    });
    (origin, server)
}

/// A streamed answer whose body ends when the connection closes.
fn streamed() -> String {
    format!("HTTP/1.1 200 OK\r\ncontent-type: {EVENT_STREAM}\r\nconnection: close\r\n\r\n")
}

/// A successful answer of `content_type` that declares 100 bytes more than `body` holds, so the
/// connection closes inside it.
fn cut_short(content_type: &str, body: &[u8]) -> String {
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\n\
         connection: close\r\n\r\n",
        body.len() + 100
    )
}

/// A refusal before any frame: `status`, the exception type Bedrock names, and a JSON body.
fn refusal(status: &str, error_type: &str, body: &[u8]) -> String {
    format!(
        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\nx-amzn-errortype: {error_type}\r\n\
         content-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    )
}

/// Fixed keys, with or without a session token. Counts how often it was asked.
struct Keys {
    session_token: Option<&'static str>,
    asked: AtomicUsize,
}

impl Keys {
    fn new(session_token: Option<&'static str>) -> Arc<Self> {
        Arc::new(Self {
            session_token,
            asked: AtomicUsize::new(0),
        })
    }
}

impl CredentialSource for Keys {
    fn credentials<'a>(&'a self, _: &'a Cancel) -> BoxFuture<'a, Result<AwsCredentials, Error>> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        let token = self.session_token.map(str::to_owned);
        Box::pin(async move { Ok(AwsCredentials::new(KEY_ID, SECRET, token, None)) })
    }
}

/// A source that has no credentials to give.
struct NoKeys;

impl CredentialSource for NoKeys {
    fn credentials<'a>(&'a self, _: &'a Cancel) -> BoxFuture<'a, Result<AwsCredentials, Error>> {
        Box::pin(async {
            Err(Error::new(
                ErrorCode::Unauthorized,
                "the fixture has no credentials",
            ))
        })
    }
}

/// A source that never answers.
struct Silent;

impl CredentialSource for Silent {
    fn credentials<'a>(&'a self, _: &'a Cancel) -> BoxFuture<'a, Result<AwsCredentials, Error>> {
        Box::pin(std::future::pending::<Result<AwsCredentials, Error>>())
    }
}

fn provenance(protocol: Protocol) -> Provenance {
    Provenance {
        protocol,
        provider: Id::new("bedrock").unwrap(),
        account: Id::new("fixture-account").unwrap(),
        endpoint: Id::new("fixture-region").unwrap(),
        model: Id::new(MODEL).unwrap(),
        binding_revision: Id::new("fixture-1").unwrap(),
    }
}

fn capabilities(stream: bool) -> Capabilities {
    Capabilities {
        tools: true,
        tool_choice: true,
        temperature: true,
        top_p: true,
        reasoning_efforts: Vec::new(),
        context_window: 200_000,
        max_output_tokens: 4_096,
        output_streaming: stream,
        input_during_output: false,
    }
}

fn target(api: Api, stream: bool, region: &str, model_id: &str, endpoint: &str) -> BedrockTarget {
    let protocol = match api {
        Api::Converse => Protocol::BedrockConverse,
        Api::AnthropicInvoke => Protocol::Messages,
    };
    BedrockTarget::new(
        provenance(protocol),
        capabilities(stream),
        region,
        model_id,
        api,
        stream,
        Some(Url::parse(endpoint).unwrap()),
    )
    .expect("a valid fixture target")
}

/// A streamed Converse target at `endpoint`.
fn converse(endpoint: &Url) -> BedrockTarget {
    target(Api::Converse, true, "us-east-1", NOVA, endpoint.as_str())
}

fn http() -> HttpClient {
    HttpClient::new(Limits {
        response_headers: Duration::from_secs(10),
        idle: Duration::from_secs(10),
        total: Duration::from_secs(30),
    })
    .unwrap()
}

fn client(target: BedrockTarget, credentials: Arc<dyn CredentialSource>) -> BedrockClient {
    BedrockClient::new(target, http(), credentials).unwrap()
}

fn hello() -> TurnRequest {
    TurnRequest::new(MODEL, vec![Item::user("Hello")])
}

/// Runs one turn and returns its result and what the sink was shown.
async fn attempt(client: &BedrockClient) -> (Result<TurnOutcome, Error>, Vec<StreamEvent>) {
    let mut sink = VecSink::new(1_000, 1 << 20);
    let result = client.turn(&hello(), &mut sink, &Cancel::new()).await;
    (result, sink.into_events())
}

/// Serves `head` and `body` to one streamed Converse turn and returns its failure, what was
/// shown and the request.
async fn failed(head: String, body: Vec<u8>) -> (Error, Vec<StreamEvent>, Request) {
    let (origin, server) = serve(head, body).await;
    let (result, events) = attempt(&client(converse(&origin), Keys::new(None))).await;
    let error = result.expect_err("the attempt fails");
    (error, events, server.await.unwrap())
}

/// A synthetic frame of `message_type`, named by `name_header`.
fn frame(message_type: &str, name_header: &str, name: &str, payload: &Value) -> Vec<u8> {
    encode_message(&Message {
        headers: vec![
            (name_header.to_owned(), HeaderValue::String(name.to_owned())),
            (
                ":content-type".to_owned(),
                HeaderValue::String("application/json".to_owned()),
            ),
            (
                ":message-type".to_owned(),
                HeaderValue::String(message_type.to_owned()),
            ),
        ],
        payload: serde_json::to_vec(payload).unwrap(),
    })
}

fn event(kind: &str, payload: &Value) -> Vec<u8> {
    frame("event", ":event-type", kind, payload)
}

/// A synthetic exception frame of `kind`, its message text one no diagnostic may quote.
fn exception(kind: &str) -> Vec<u8> {
    frame(
        "exception",
        ":exception-type",
        kind,
        &json!({"message": "fixture exception text"}),
    )
}

/// Synthetic: the message opened and nothing answered yet.
fn opening() -> Vec<u8> {
    event("messageStart", &json!({"role": "assistant"}))
}

/// Synthetic: the message opened and one text delta was shown.
fn partial() -> Vec<u8> {
    let mut body = opening();
    body.extend(event(
        "contentBlockDelta",
        &json!({"contentBlockIndex": 0, "delta": {"text": "Partial"}}),
    ));
    body
}

/// Synthetic: a whole answer, the text block, both terminal events.
fn complete() -> Vec<u8> {
    let mut body = partial();
    body.extend(event("contentBlockStop", &json!({"contentBlockIndex": 0})));
    body.extend(event("messageStop", &json!({"stopReason": "end_turn"})));
    body.extend(event(
        "metadata",
        &json!({"usage": {"inputTokens": 3, "outputTokens": 1, "totalTokens": 4}}),
    ));
    body
}

fn shown_partial() -> Vec<StreamEvent> {
    vec![StreamEvent::TextDelta {
        text: "Partial".to_owned(),
    }]
}

#[tokio::test]
async fn a_request_is_signed_for_its_region_and_the_bedrock_service() {
    let (origin, server) = serve(streamed(), complete()).await;
    let authority = format!("127.0.0.1:{}", origin.port().unwrap());
    let signed = target(Api::Converse, true, "eu-central-1", NOVA, origin.as_str());
    let (result, _) = attempt(&client(signed, Keys::new(Some(SESSION_TOKEN)))).await;
    assert_eq!(result.unwrap().items, vec![Item::assistant("Partial")]);
    let sent = server.await.unwrap();

    let date = sent.header("x-amz-date").expect("the request is dated");
    assert_eq!(date.len(), 16);
    assert!(date.bytes().enumerate().all(|(at, byte)| match at {
        8 => byte == b'T',
        15 => byte == b'Z',
        _ => byte.is_ascii_digit(),
    }));
    let authorization = sent.header("authorization").expect("the request is signed");
    let scope = format!(
        "AWS4-HMAC-SHA256 Credential={KEY_ID}/{}/eu-central-1/bedrock/aws4_request, \
         SignedHeaders=",
        &date[..8]
    );
    let rest = authorization
        .strip_prefix(&scope)
        .expect("the scope names the request's day, its Region and the bedrock service");
    let (names, signature) = rest.split_once(", Signature=").unwrap();
    let names: Vec<&str> = names.split(';').collect();
    for name in [
        "accept",
        "content-type",
        "host",
        "x-amz-date",
        "x-amz-security-token",
    ] {
        assert!(names.contains(&name), "{name} is signed");
    }
    assert_eq!(signature.len(), 64);
    assert!(
        signature
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    assert_eq!(sent.header("x-amz-security-token"), Some(SESSION_TOKEN));
    assert_eq!(sent.count("host"), 1);
    assert_eq!(sent.header("host"), Some(authority.as_str()));
    assert!(
        sent.headers
            .iter()
            .all(|(_, value)| !value.contains(SECRET))
    );
    assert!(!String::from_utf8_lossy(&sent.body).contains(SECRET));
}

#[tokio::test]
async fn keys_without_a_session_token_send_none() {
    let (origin, server) = serve(streamed(), complete()).await;
    let keys = Keys::new(None);
    let (result, _) = attempt(&client(converse(&origin), keys.clone())).await;
    assert_eq!(result.unwrap().items, vec![Item::assistant("Partial")]);
    let sent = server.await.unwrap();
    assert!(sent.header("x-amz-security-token").is_none());
    assert!(
        sent.header("authorization")
            .is_some_and(|value| !value.contains("x-amz-security-token"))
    );
    assert_eq!(keys.asked.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn each_operation_is_named_by_its_wire_and_its_mode() {
    let arn = "arn:aws:bedrock:us-east-1:123456789012:inference-profile/\
               us.anthropic.claude-3-7-sonnet-20250219-v1:0";
    let cases = [
        (
            Api::Converse,
            true,
            NOVA,
            "/model/us.amazon.nova-micro-v1%3A0/converse-stream",
        ),
        (
            Api::Converse,
            false,
            NOVA,
            "/model/us.amazon.nova-micro-v1%3A0/converse",
        ),
        (
            Api::AnthropicInvoke,
            true,
            "anthropic.claude-3-sonnet-20240229-v1:0",
            "/model/anthropic.claude-3-sonnet-20240229-v1%3A0/invoke-with-response-stream",
        ),
        (
            Api::AnthropicInvoke,
            false,
            "anthropic.claude-3-sonnet-20240229-v1:0",
            "/model/anthropic.claude-3-sonnet-20240229-v1%3A0/invoke",
        ),
        (
            Api::Converse,
            true,
            arn,
            "/model/arn%3Aaws%3Abedrock%3Aus-east-1%3A123456789012%3Ainference-profile%2F\
             us.anthropic.claude-3-7-sonnet-20250219-v1%3A0/converse-stream",
        ),
    ];
    for (api, stream, model_id, path) in cases {
        let body = br#"{"message":"fixture validation text"}"#.to_vec();
        let (origin, server) = serve(
            refusal("400 Bad Request", "ValidationException", &body),
            body,
        )
        .await;
        let (result, _) = attempt(&client(
            target(api, stream, "us-east-1", model_id, origin.as_str()),
            Keys::new(None),
        ))
        .await;
        assert_eq!(result.unwrap_err().code, ErrorCode::Refused, "{path}");
        let sent = server.await.unwrap();
        assert_eq!(sent.line, format!("POST {path} HTTP/1.1"));
        let accept = if stream {
            EVENT_STREAM
        } else {
            "application/json"
        };
        assert_eq!(sent.header("accept"), Some(accept), "{path}");
        assert_eq!(
            sent.header("content-type"),
            Some("application/json"),
            "{path}"
        );
    }
}

#[tokio::test]
async fn throttling_inside_the_stream_is_retriable_only_before_the_turn_answered() {
    // Synthetic: the message opens, then Bedrock throttles before any content.
    let mut body = opening();
    body.extend(exception("throttlingException"));
    let (error, events, _) = failed(streamed(), body).await;
    assert_eq!(error.code, ErrorCode::RateLimited);
    assert!(error.retriable && error.may_retry());
    assert_eq!(error.dispatch, Dispatch::Accepted);
    assert!(!error.message.contains("fixture"));
    let observation = error.observation.expect("the failure keeps its binding");
    assert!(!observation.final_usage);
    assert_eq!(events, [] as [StreamEvent; 0]);

    // Synthetic: the same exception after one text delta was shown.
    let mut body = partial();
    body.extend(exception("throttlingException"));
    let (error, events, _) = failed(streamed(), body).await;
    assert_eq!(error.code, ErrorCode::RateLimited);
    assert!(!error.retriable && !error.may_retry());
    assert_eq!(error.dispatch, Dispatch::Accepted);
    assert_eq!(events, shown_partial());
}

#[tokio::test]
async fn a_validation_exception_inside_the_stream_is_final_before_output() {
    // Synthetic: a refusal Bedrock reports only once the stream is open.
    let mut body = opening();
    body.extend(exception("validationException"));
    let (error, events, _) = failed(streamed(), body).await;
    assert_eq!(error.code, ErrorCode::Refused);
    assert!(!error.retriable);
    assert_eq!(error.dispatch, Dispatch::Accepted);
    assert_eq!(events, [] as [StreamEvent; 0]);
}

#[tokio::test]
async fn a_cut_is_retriable_before_the_turn_answered_and_final_after() {
    let body = opening();
    let (error, events, _) = failed(cut_short(EVENT_STREAM, &body), body).await;
    assert_eq!(error.code, ErrorCode::Transport);
    assert!(error.retriable && error.may_retry());
    assert_eq!(error.dispatch, Dispatch::Accepted);
    assert_eq!(events, [] as [StreamEvent; 0]);

    let body = partial();
    let (error, events, _) = failed(cut_short(EVENT_STREAM, &body), body).await;
    assert_eq!(error.code, ErrorCode::Transport);
    assert!(!error.retriable);
    assert_eq!(error.dispatch, Dispatch::Accepted);
    assert_eq!(events, shown_partial());
}

#[tokio::test]
async fn a_frame_left_half_read_at_the_end_is_a_cut() {
    // Synthetic: the body ends, cleanly, seven bytes into a frame's prelude.
    let half = event("contentBlockStop", &json!({"contentBlockIndex": 0}));
    let mut body = opening();
    body.extend_from_slice(&half[..7]);
    let (error, events, _) = failed(streamed(), body).await;
    assert_eq!(error.code, ErrorCode::Protocol);
    assert!(error.retriable);
    assert_eq!(error.dispatch, Dispatch::Accepted);
    assert_eq!(events, [] as [StreamEvent; 0]);

    let mut body = partial();
    body.extend_from_slice(&half[..7]);
    let (error, events, _) = failed(streamed(), body).await;
    assert_eq!(error.code, ErrorCode::Protocol);
    assert!(!error.retriable);
    assert_eq!(events, shown_partial());
}

/// A body that ends cleanly between frames before the terminal events is no cut: it is a final
/// `protocol` failure before any event, and after the message start, as on the Messages wire.
#[tokio::test]
async fn a_body_that_ends_cleanly_before_the_terminal_events_is_a_final_protocol_failure() {
    for body in [Vec::new(), opening()] {
        let (error, events, _) = failed(streamed(), body).await;
        assert_eq!(error.code, ErrorCode::Protocol, "{error:?}");
        assert!(!error.retriable && !error.may_retry(), "{error:?}");
        assert_eq!(events, [] as [StreamEvent; 0]);
    }
}

#[tokio::test]
async fn a_whole_answer_cut_short_is_final() {
    let body = br#"{"output":{"message":{"role":"assistant","content":[{"text":"Par"#.to_vec();
    let (origin, _server) = serve(cut_short("application/json", &body), body).await;
    let whole = target(Api::Converse, false, "us-east-1", NOVA, origin.as_str());
    let (result, events) = attempt(&client(whole, Keys::new(None))).await;
    let error = result.unwrap_err();
    assert_eq!(error.code, ErrorCode::Transport);
    assert!(!error.retriable);
    assert_eq!(events, [] as [StreamEvent; 0]);
}

#[tokio::test]
async fn a_validation_exception_before_the_stream_is_final() {
    let body = br#"{"message":"fixture validation text"}"#.to_vec();
    let head = refusal(
        "400 Bad Request",
        "ValidationException:http://example.invalid/",
        &body,
    );
    let (error, events, _) = failed(head, body).await;
    assert_eq!(error.code, ErrorCode::Refused);
    assert_eq!(error.dispatch, Dispatch::Rejected);
    assert!(!error.retriable);
    assert!(!error.message.contains("fixture"));
    assert_eq!(events, [] as [StreamEvent; 0]);
}

#[tokio::test]
async fn throttling_before_the_stream_is_a_retriable_rate_limit() {
    let body = br#"{"message":"fixture throttling text"}"#.to_vec();
    let head = refusal("429 Too Many Requests", "ThrottlingException", &body);
    let (error, events, _) = failed(head, body).await;
    assert_eq!(error.code, ErrorCode::RateLimited);
    assert_eq!(error.dispatch, Dispatch::Rejected);
    assert!(error.retriable && error.may_retry());
    assert!(!error.message.contains("fixture"));
    assert_eq!(events, [] as [StreamEvent; 0]);
}

#[tokio::test]
async fn an_exhausted_quota_before_the_stream_is_final() {
    let body = br#"{"message":"fixture quota text"}"#.to_vec();
    let head = refusal(
        "429 Too Many Requests",
        "ServiceQuotaExceededException",
        &body,
    );
    let (error, _, _) = failed(head, body).await;
    assert_eq!(error.code, ErrorCode::Refused);
    assert_eq!(error.dispatch, Dispatch::Rejected);
    assert!(!error.retriable);
    assert!(!error.message.contains("fixture"));
}

#[tokio::test]
async fn a_streamed_turn_answered_in_another_media_type_is_refused() {
    let (origin, _server) = serve(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n".to_owned(),
        b"{}".to_vec(),
    )
    .await;
    let (result, events) = attempt(&client(converse(&origin), Keys::new(None))).await;
    let error = result.unwrap_err();
    assert_eq!(error.code, ErrorCode::Protocol);
    assert_eq!(error.dispatch, Dispatch::Accepted);
    assert_eq!(events, [] as [StreamEvent; 0]);
}

#[tokio::test]
async fn credentials_that_fail_end_the_turn_before_it_is_sent() {
    let nowhere = Url::parse(NOWHERE).unwrap();
    let (result, events) = attempt(&client(converse(&nowhere), Arc::new(NoKeys))).await;
    let error = result.unwrap_err();
    assert_eq!(error.code, ErrorCode::Unauthorized);
    assert_eq!(error.dispatch, Dispatch::NotSent);
    assert_eq!(events, [] as [StreamEvent; 0]);
}

#[tokio::test]
async fn credentials_that_never_answer_end_on_the_turn_deadline() {
    let nowhere = Url::parse(NOWHERE).unwrap();
    let bounded = client(converse(&nowhere), Arc::new(Silent))
        .with_turn_limit(Duration::from_millis(100))
        .unwrap();
    let (result, _) = attempt(&bounded).await;
    let error = result.unwrap_err();
    assert_eq!(error.code, ErrorCode::Deadline);
    assert_eq!(error.dispatch, Dispatch::NotSent);
}

#[test]
fn a_turn_limit_is_positive_and_at_most_a_day() {
    let nowhere = Url::parse(NOWHERE).unwrap();
    let make = || client(converse(&nowhere), Keys::new(None));
    assert!(make().with_turn_limit(Duration::ZERO).is_err());
    assert!(make().with_turn_limit(Duration::from_secs(86_401)).is_err());
    assert!(make().with_turn_limit(Duration::from_hours(24)).is_ok());
}

/// A streamed Converse target with `region`, `model_id` and `endpoint`, as
/// `BedrockTarget::new` checks it.
fn checked(region: &str, model_id: &str, endpoint: Option<&str>) -> Result<BedrockTarget, Error> {
    BedrockTarget::new(
        provenance(Protocol::BedrockConverse),
        capabilities(true),
        region,
        model_id,
        Api::Converse,
        true,
        endpoint.map(|endpoint| Url::parse(endpoint).unwrap()),
    )
}

#[test]
fn a_target_names_an_aws_region_and_refuses_anything_else_without_quoting_it() {
    for region in [
        "us-east-1",
        "eu-central-1",
        "us-gov-west-1",
        "ap-southeast-2",
    ] {
        assert!(checked(region, NOVA, None).is_ok(), "{region}");
    }
    let long = format!("us-{}-1", "a".repeat(40));
    for region in [
        "",
        "us",
        "US-EAST-1",
        "us_east_1",
        "us-east-1.",
        "us--east-1",
        "us-east-1/",
        long.as_str(),
    ] {
        let error = checked(region, NOVA, None).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{region}");
        assert!(
            region.len() < 4 || !error.message.contains(region),
            "{region}"
        );
    }
}

#[test]
fn a_target_names_a_model_id_bedrock_can_route() {
    let arn = "arn:aws:bedrock:us-east-1:123456789012:application-inference-profile/a1b2c3";
    for model_id in [NOVA, arn, "anthropic.claude-3-sonnet-20240229-v1:0"] {
        assert!(checked("us-east-1", model_id, None).is_ok(), "{model_id}");
    }
    let long = "a".repeat(2049);
    for model_id in ["", ".", "..", "model id", "mod\u{e8}le", long.as_str()] {
        let error = checked("us-east-1", model_id, None).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{model_id}");
    }
}

#[test]
fn a_target_endpoint_is_an_origin_without_credentials() {
    for endpoint in [
        "https://bedrock.example.invalid/",
        "https://10.0.0.1/",
        "http://127.0.0.1:8080/",
        "http://127.1.2.3/",
        "http://[::1]:8080/",
    ] {
        assert!(
            checked("us-east-1", NOVA, Some(endpoint)).is_ok(),
            "{endpoint}"
        );
    }
    for endpoint in [
        "http://127.0.0.1:8080/v1",
        "http://fixture-user:fixture-password@127.0.0.1:8080/",
        "http://127.0.0.1:8080/?a=1",
        "http://127.0.0.1:8080/#f",
        "ftp://127.0.0.1/",
    ] {
        let error = checked("us-east-1", NOVA, Some(endpoint)).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{endpoint}");
        assert!(!error.message.contains("fixture"), "{endpoint}");
    }
}

/// Plain `http` reaches a loopback address only: a signed request and its answer cross no
/// network in the clear. A host name is no address, `localhost` included.
#[test]
fn a_target_endpoint_off_loopback_must_use_https() {
    for endpoint in [
        "http://fixture-bedrock.example.invalid/",
        "http://localhost:8080/",
        "http://10.0.0.1/",
        "http://169.254.170.2/",
        "http://[fd00:ec2::23]/",
        "http://[::ffff:127.0.0.1]/",
    ] {
        let error = checked("us-east-1", NOVA, Some(endpoint)).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{endpoint}");
        assert!(!error.message.contains("fixture"), "{endpoint}");
        assert!(!error.message.contains("10.0.0.1"), "{endpoint}");
    }
}

/// Converse carries no reasoning effort, so a Converse target declaring one is refused by the
/// field's name; an `InvokeModel` target may declare them.
#[test]
fn a_converse_target_declares_no_reasoning_efforts() {
    let mut efforts = capabilities(true);
    efforts.reasoning_efforts = vec!["high".to_owned()];
    let error = BedrockTarget::new(
        provenance(Protocol::BedrockConverse),
        efforts.clone(),
        "us-east-1",
        NOVA,
        Api::Converse,
        true,
        None,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(
        error.message.contains("capabilities.reasoning_efforts"),
        "{}",
        error.message
    );
    BedrockTarget::new(
        provenance(Protocol::Messages),
        efforts,
        "us-east-1",
        "anthropic.claude-fixture-v1:0",
        Api::AnthropicInvoke,
        true,
        None,
    )
    .expect("an InvokeModel target with reasoning efforts");
}

#[test]
fn a_target_speaks_the_protocol_of_its_wire_in_the_mode_it_declares() {
    let mismatched = |protocol, api| {
        BedrockTarget::new(
            provenance(protocol),
            capabilities(true),
            "us-east-1",
            NOVA,
            api,
            true,
            None,
        )
        .unwrap_err()
        .code
    };
    assert_eq!(
        mismatched(Protocol::Messages, Api::Converse),
        ErrorCode::Unsupported
    );
    assert_eq!(
        mismatched(Protocol::BedrockConverse, Api::AnthropicInvoke),
        ErrorCode::Unsupported
    );

    let streaming = |declared, stream| {
        BedrockTarget::new(
            provenance(Protocol::BedrockConverse),
            capabilities(declared),
            "us-east-1",
            NOVA,
            Api::Converse,
            stream,
            None,
        )
    };
    assert!(streaming(true, true).is_ok() && streaming(false, false).is_ok());
    assert_eq!(
        streaming(false, true).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        streaming(true, false).unwrap_err().code,
        ErrorCode::InvalidRequest
    );

    let mut duplex = capabilities(true);
    duplex.input_during_output = true;
    let error = BedrockTarget::new(
        provenance(Protocol::BedrockConverse),
        duplex,
        "us-east-1",
        NOVA,
        Api::Converse,
        true,
        None,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
}

#[test]
fn a_client_checks_a_target_changed_after_it_was_made() {
    let nowhere = Url::parse(NOWHERE).unwrap();
    let mut changed = converse(&nowhere);
    changed.region = "US".to_owned();
    let error = BedrockClient::new(changed, http(), Keys::new(None)).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
}

#[test]
fn a_client_shows_no_secret_in_its_debug_output() {
    let nowhere = Url::parse(NOWHERE).unwrap();
    let shown = format!(
        "{:?}",
        client(converse(&nowhere), Keys::new(Some(SESSION_TOKEN)))
    );
    assert!(shown.contains("BedrockClient"));
    assert!(!shown.contains(SECRET) && !shown.contains(SESSION_TOKEN));
}
