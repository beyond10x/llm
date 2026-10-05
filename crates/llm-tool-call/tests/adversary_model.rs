//! Adversary cases for `call_tool` and the Codex preset (story call-tool-helper), ported from
//! Loom's `intake-model`, where they were written for Loom story `model-access` (wave
//! 2026-10-04-w17, first pass).
//!
//! Every model is the Codex Responses model pointed at a fixture server on `127.0.0.1`, a recorded
//! fake, or a wrapper around one of those. Every credential is a fixture `auth.json` under
//! `CARGO_TARGET_TMPDIR`; nothing reads the operator's Codex login, the process environment or
//! the network.

use b10x_llm_tool_call::{ModelError, call_tool, codex_auth_path, codex_model_at};
use llm_core::{
    BoxFuture, CallId, Cancel, Capabilities, Error, Id, Item, Model, Protocol, Provenance,
    StopReason, StreamEvent, StreamSink, ToolCall, ToolChoice, ToolName, ToolSpec, TurnObservation,
    TurnOutcome, TurnRequest,
};
use serde_json::{Value, json};
use std::{
    ffi::OsStr,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};

const MODEL: &str = "gpt-5.1-codex";
const TOOL: &str = "propose_protocol";
const INSTRUCTIONS: &str = "Propose the protocol this intent runs under.";

/// A JWT whose `exp` is 2100-01-01.
const VALID_JWT: &str = "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.eyJleHAiOjQxMDI0NDQ4MDB9.fixture";

/// Material that must never appear in a refusal's message.
const SECRET: &str = "SECRETMATERIAL7f3a";

const SSE_HEAD: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";

const CHUNKED_SSE_HEAD: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n";

fn tool() -> ToolSpec {
    ToolSpec {
        name: ToolName::new(TOOL).expect("tool name"),
        description: "Propose one protocol from the registry".to_owned(),
        input_schema: json!({
            "type": "object",
            "properties": {"protocol": {"type": "string"}},
            "required": ["protocol"]
        }),
    }
}

fn items() -> Vec<Item> {
    vec![Item::user("Fix the flaky retry test in the importer")]
}

fn fixture_dir(case: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("llm-tool-call-adversary")
        .join(format!("{case}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a fixture directory");
    dir
}

/// Writes `document` as `auth.json` in a fresh fixture directory and returns its path.
fn auth_document(case: &str, document: &str) -> PathBuf {
    let path = fixture_dir(case).join("auth.json");
    fs::write(&path, document).expect("a fixture credential");
    path
}

fn auth_with_token(case: &str, token: &str) -> PathBuf {
    auth_document(
        case,
        &json!({
            "OPENAI_API_KEY": null,
            "tokens": {"id_token": token, "access_token": token, "refresh_token": "r", "account_id": "a"}
        })
        .to_string(),
    )
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

async fn accept(listener: &TcpListener) -> TcpStream {
    tokio::time::timeout(Duration::from_secs(10), listener.accept())
        .await
        .expect("the client never opened a connection")
        .expect("an accepted connection")
        .0
}

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

async fn assert_no_request(listener: &TcpListener, why: &str) {
    assert!(
        tokio::time::timeout(Duration::from_millis(150), listener.accept())
            .await
            .is_err(),
        "{why}"
    );
}

/// Serves one request with `reply` written verbatim, then closes; returns the request it read.
fn serve(listener: TcpListener, reply: Vec<u8>) -> JoinHandle<String> {
    tokio::spawn(async move {
        let mut socket = accept(&listener).await;
        let captured = read_request(&mut socket).await;
        socket.write_all(&reply).await.expect("reply written");
        socket.shutdown().await.expect("closed");
        drop(socket);
        assert_no_request(&listener, "the request was attempted a second time").await;
        captured
    })
}

/// Server-sent events, one per JSON object, each named by its `type`.
fn sse(events: &[Value]) -> String {
    events.iter().fold(String::new(), |mut stream, event| {
        let _ = write!(
            stream,
            "event: {}\ndata: {}\n\n",
            event["type"].as_str().expect("a typed event"),
            event
        );
        stream
    })
}

fn function_call(id: &str, call_id: &str, arguments: &str) -> Value {
    json!({
        "type": "function_call", "id": id, "call_id": call_id, "name": TOOL,
        "arguments": arguments, "status": "completed"
    })
}

/// The opening of an answer that starts one call to the forced tool and is never finished.
fn unfinished_answer() -> String {
    sse(&[
        json!({"type": "response.created", "response": {"id": "resp_9", "status": "in_progress"}}),
        json!({"type": "response.output_item.added", "output_index": 0,
               "item": {"type": "function_call", "id": "fc_1", "call_id": "call_1", "name": TOOL, "arguments": ""}}),
        json!({"type": "response.function_call_arguments.delta", "item_id": "fc_1", "output_index": 0,
               "delta": "{\"protocol\":"}),
    ])
}

fn with_head(head: &[u8], body: &str) -> Vec<u8> {
    let mut reply = head.to_vec();
    reply.extend_from_slice(body.as_bytes());
    reply
}

/// A sink that drops every event.
struct Discard;

impl StreamSink for Discard {
    fn emit(&mut self, _event: StreamEvent) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async { Ok(()) })
    }
}

/// The request `call_tool` itself would send to a model named `model`.
fn forced_request(model: &str) -> TurnRequest {
    let mut request = TurnRequest::new(model, items());
    INSTRUCTIONS.clone_into(&mut request.instructions);
    request.tools = vec![tool()];
    request.tool_choice = ToolChoice::Named(ToolName::new(TOOL).expect("tool name"));
    request
}

fn recorded_provenance() -> Provenance {
    let id = |value: &str| Id::new(value).expect("fixture identifier");
    Provenance {
        protocol: Protocol::Responses,
        provider: id("recorded"),
        account: id("recorded"),
        endpoint: id("recorded"),
        model: id("recorded-model"),
        binding_revision: id("rev-1"),
    }
}

/// A recorded model answering every turn with one forced-tool call carrying `arguments`.
struct Recorded {
    provenance: Provenance,
    capabilities: Capabilities,
    arguments: Value,
}

impl Recorded {
    fn new(arguments: Value) -> Self {
        Self {
            provenance: recorded_provenance(),
            capabilities: Capabilities {
                tools: true,
                tool_choice: true,
                temperature: false,
                top_p: false,
                reasoning_efforts: Vec::new(),
                context_window: 32_768,
                max_output_tokens: 2_048,
            },
            arguments,
        }
    }
}

impl Model for Recorded {
    fn provenance(&self) -> &Provenance {
        &self.provenance
    }
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }
    fn turn<'a>(
        &'a self,
        _request: &'a TurnRequest,
        _sink: &'a mut dyn StreamSink,
        _cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        let mut observation = TurnObservation::new(self.provenance.clone());
        observation.final_usage = true;
        let outcome = TurnOutcome {
            stop_reason: StopReason::ToolCalls,
            items: vec![Item::ToolCall(ToolCall {
                call_id: CallId::new("call_1").expect("call id"),
                name: ToolName::new(TOOL).expect("tool name"),
                arguments: self.arguments.clone(),
            })],
            observation,
        };
        Box::pin(async move { Ok(outcome) })
    }
}

/// The Transport variant promises "the connection failed before the answer completed". A
/// fixture that sends the head and the start of a call, then closes, is exactly that.
#[tokio::test]
async fn a_stream_closed_before_the_answer_completed_is_a_transport_failure() {
    let auth = auth_with_token("cut-off", VALID_JWT);
    let (socket, url) = listener().await;
    let server = serve(socket, with_head(SSE_HEAD, &unfinished_answer()));
    let model = codex_model_at(MODEL, &url, &auth).expect("a Codex model");
    let error = call_tool(&model, INSTRUCTIONS, items(), tool())
        .await
        .expect_err("half an answer is no answer");
    server.await.expect("the fixture server");
    assert!(
        matches!(error, ModelError::Transport(_)),
        "the connection closed before the answer completed, and came back as {error:?}"
    );
}

/// The same cut, under chunked framing: the body itself fails.
#[tokio::test]
async fn a_chunked_stream_cut_mid_chunk_is_a_transport_failure() {
    let auth = auth_with_token("chunk-cut", VALID_JWT);
    let (socket, url) = listener().await;
    let body = unfinished_answer();
    let mut chunked = format!("{:x}\r\n{body}\r\n", body.len() + 64);
    chunked.truncate(chunked.len() - 2);
    let server = serve(socket, with_head(CHUNKED_SSE_HEAD, &chunked));
    let model = codex_model_at(MODEL, &url, &auth).expect("a Codex model");
    let error = call_tool(&model, INSTRUCTIONS, items(), tool())
        .await
        .expect_err("half an answer is no answer");
    server.await.expect("the fixture server");
    assert!(matches!(error, ModelError::Transport(_)), "{error:?}");
}

/// Every refusal kind of the Codex login is a credential variant, decided before any request,
/// and no message carries what the file held.
#[tokio::test]
async fn every_unusable_login_is_refused_before_any_request_without_its_material() {
    let (socket, url) = listener().await;

    let directory = fixture_dir("directory").join("auth.json");
    fs::create_dir_all(&directory).expect("a directory where the file belongs");
    let parent_is_a_file = {
        let file = fixture_dir("parent-file").join("codex-home");
        fs::write(&file, "a file, not a directory").expect("a file");
        file.join("auth.json")
    };
    let mut oversized = format!("{{\"pad\":\"{SECRET}");
    oversized.push_str(&"x".repeat(1024 * 1024));
    let _ = write!(
        oversized,
        "\",\"tokens\":{{\"access_token\":\"{VALID_JWT}\"}}}}"
    );
    let unusable: Vec<(&str, PathBuf)> = vec![
        ("a directory at the path", directory),
        ("CODEX_HOME naming a file", parent_is_a_file),
        ("a relative path", PathBuf::from("relative/auth.json")),
        (
            "a document that is not JSON",
            auth_document("not-json", &format!("not json {SECRET}")),
        ),
        (
            "a token that is not a JWT",
            auth_with_token("not-jwt", &format!("{SECRET}.%%%.sig")),
        ),
        ("a file over 1 MiB", auth_document("oversized", &oversized)),
    ];
    for (case, path) in unusable {
        let model = codex_model_at(MODEL, &url, &path).expect("construction reads no file");
        let error = call_tool(&model, INSTRUCTIONS, items(), tool())
            .await
            .expect_err(case);
        assert!(
            matches!(error, ModelError::UnusableCredential(_)),
            "{case}: {error:?}"
        );
        let message = error.to_string();
        assert!(!message.contains(SECRET), "{case} leaked: {message}");
    }

    // An API-key login has no access token: missing, and the key stays out of the message.
    let api_key = auth_document(
        "api-key",
        &json!({"OPENAI_API_KEY": format!("sk-{SECRET}"), "tokens": null}).to_string(),
    );
    let model = codex_model_at(MODEL, &url, &api_key).expect("construction reads no file");
    let error = call_tool(&model, INSTRUCTIONS, items(), tool())
        .await
        .expect_err("no access token, no call");
    assert!(
        matches!(error, ModelError::MissingCredential(_)),
        "{error:?}"
    );
    assert!(!error.to_string().contains(SECRET), "{error}");

    assert_no_request(&socket, "an unusable login still reached the server").await;
}

/// Two calls to the forced tool, under distinct call ids, are no single answer.
#[tokio::test]
async fn two_calls_to_the_forced_tool_are_multiple_calls() {
    let auth = auth_with_token("two-calls", VALID_JWT);
    let (socket, url) = listener().await;
    let first = function_call("fc_1", "call_1", "{\"protocol\":\"software.change/1\"}");
    let second = function_call("fc_2", "call_2", "{\"protocol\":\"software.fix/1\"}");
    let mut added_first = first.clone();
    added_first["arguments"] = json!("");
    let mut added_second = second.clone();
    added_second["arguments"] = json!("");
    let stream = sse(&[
        json!({"type": "response.created", "response": {"id": "resp_4", "status": "in_progress"}}),
        json!({"type": "response.output_item.added", "output_index": 0, "item": added_first}),
        json!({"type": "response.output_item.done", "output_index": 0, "item": first}),
        json!({"type": "response.output_item.added", "output_index": 1, "item": added_second}),
        json!({"type": "response.output_item.done", "output_index": 1, "item": second}),
        json!({"type": "response.completed", "response": {
            "id": "resp_4", "model": MODEL, "status": "completed", "output": [first, second],
            "usage": {"input_tokens": 20, "output_tokens": 18}}}),
    ]);
    let server = serve(socket, with_head(SSE_HEAD, &stream));
    let model = codex_model_at(MODEL, &url, &auth).expect("a Codex model");
    let error = call_tool(&model, INSTRUCTIONS, items(), tool())
        .await
        .expect_err("two calls are not one answer");
    server.await.expect("the fixture server");
    assert!(
        matches!(error, ModelError::MultipleToolCalls(2)),
        "{error:?}"
    );
}

/// A refusal raised by a turn outside `call_tool` is not kept for a later call, and two calls
/// running at once each see only their own login.
#[tokio::test]
async fn a_login_refusal_stays_with_the_call_that_raised_it() {
    let (unused, unused_url) = listener().await;
    let absent = fixture_dir("leak-absent").join("auth.json");
    let missing = codex_model_at(MODEL, &unused_url, &absent).expect("a Codex model");

    // Outside call_tool: llm refuses untyped, and nothing is kept.
    let request = forced_request(MODEL);
    let error = missing
        .turn(&request, &mut Discard, &Cancel::new())
        .await
        .expect_err("no login, no turn");
    assert_eq!(error.code, llm_core::ErrorCode::Unauthorized, "{error:?}");

    let call = function_call("fc_1", "call_1", "{\"protocol\":\"software.change/1\"}");
    let answer = || {
        let mut added = call.clone();
        added["arguments"] = json!("");
        sse(&[
            json!({"type": "response.created", "response": {"id": "resp_5", "status": "in_progress"}}),
            json!({"type": "response.output_item.added", "output_index": 0, "item": added}),
            json!({"type": "response.output_item.done", "output_index": 0, "item": call}),
            json!({"type": "response.completed", "response": {
                "id": "resp_5", "model": MODEL, "status": "completed", "output": [call],
                "usage": {"input_tokens": 20, "output_tokens": 9}}}),
        ])
    };
    let auth = auth_with_token("leak-valid", VALID_JWT);
    let (socket, url) = listener().await;
    let server = serve(socket, with_head(SSE_HEAD, &answer()));
    let valid = codex_model_at(MODEL, &url, &auth).expect("a Codex model");
    let arguments = call_tool(&valid, INSTRUCTIONS, items(), tool())
        .await
        .expect("an earlier refusal elsewhere does not refuse this call");
    assert_eq!(arguments, json!({"protocol": "software.change/1"}));
    server.await.expect("the fixture server");

    // Concurrently, in one task.
    let (socket, url) = listener().await;
    let server = serve(socket, with_head(SSE_HEAD, &answer()));
    let valid = codex_model_at(MODEL, &url, &auth).expect("a Codex model");
    let (refused, answered) = tokio::join!(
        call_tool(&missing, INSTRUCTIONS, items(), tool()),
        call_tool(&valid, INSTRUCTIONS, items(), tool()),
    );
    assert!(
        matches!(refused, Err(ModelError::MissingCredential(_))),
        "{refused:?}"
    );
    assert_eq!(
        answered.expect("the valid login's call"),
        json!({"protocol": "software.change/1"})
    );
    server.await.expect("the fixture server");
    assert_no_request(&unused, "a missing login reached the server").await;
}

/// `CODEX_HOME=relative/dir` is handed to llm as a relative login path, which llm refuses on
/// every call with "pass the absolute path of `auth.json`", a thing the operator never passed.
#[test]
fn a_relative_codex_home_does_not_become_a_relative_login_path() {
    let found = codex_auth_path(
        Some(OsStr::new("relative/codex")),
        Some(OsStr::new("/srv/operator")),
    );
    if let Ok(path) = found {
        assert!(
            path.is_absolute(),
            "CODEX_HOME=relative/codex became the login path {path:?}; every call will then be refused as not absolute"
        );
    }
}

/// Empty counts as unset, a trailing slash is harmless, and a non-UTF-8 value is kept as is.
#[cfg(unix)]
#[test]
fn codex_home_boundaries() {
    use std::os::unix::ffi::OsStrExt;
    let home = Some(OsStr::new("/srv/operator"));
    assert_eq!(
        codex_auth_path(Some(OsStr::new("")), home).expect("HOME is used"),
        PathBuf::from("/srv/operator/.codex/auth.json")
    );
    assert_eq!(
        codex_auth_path(Some(OsStr::new("/srv/codex-home/")), home).expect("a path"),
        PathBuf::from("/srv/codex-home/auth.json")
    );
    let raw = OsStr::from_bytes(b"/srv/codex-\xff");
    assert_eq!(
        codex_auth_path(Some(raw), home)
            .expect("a path")
            .as_os_str()
            .as_bytes(),
        b"/srv/codex-\xff/auth.json"
    );
    for (codex_home, home) in [
        (Some(OsStr::new("")), Some(OsStr::new(""))),
        (None, Some(OsStr::new(""))),
        (Some(OsStr::new("")), None),
    ] {
        assert!(
            matches!(
                codex_auth_path(codex_home, home),
                Err(ModelError::MissingCredential(_))
            ),
            "{codex_home:?} {home:?}"
        );
    }
}

/// llm refuses non-object arguments from the real binding ("function call arguments are not a
/// JSON object"); the suite's own comment says `call_tool` holds a recorded model to the forced
/// tool itself. A recorded string argument comes back as an answer.
#[tokio::test]
async fn a_recorded_answer_llm_would_refuse_is_not_an_answer() {
    for arguments in [
        json!("software.change/1"),
        json!(["software.change/1"]),
        Value::Null,
    ] {
        let model = Recorded::new(arguments.clone());
        let result = call_tool(&model, INSTRUCTIONS, items(), tool()).await;
        assert!(
            result.is_err(),
            "non-object arguments {arguments} came back as an answer: {result:?}"
        );
    }
}

/// A model that tries the Codex login and, refused, answers from a second model.
struct Fallback {
    first: Box<dyn Model>,
    second: Recorded,
}

impl Model for Fallback {
    fn provenance(&self) -> &Provenance {
        self.first.provenance()
    }
    fn capabilities(&self) -> &Capabilities {
        self.first.capabilities()
    }
    fn turn<'a>(
        &'a self,
        request: &'a TurnRequest,
        sink: &'a mut dyn StreamSink,
        cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        Box::pin(async move {
            match self.first.turn(request, &mut *sink, cancel).await {
                Ok(outcome) => Ok(outcome),
                Err(_) => self.second.turn(request, sink, cancel).await,
            }
        })
    }
}

/// The kept refusal is consulted before the result: a turn that succeeded is reported as a
/// credential failure.
#[tokio::test]
async fn a_kept_refusal_does_not_override_a_turn_that_succeeded() {
    let (unused, url) = listener().await;
    let absent = fixture_dir("fallback-absent").join("auth.json");
    let model = Fallback {
        first: Box::new(codex_model_at(MODEL, &url, &absent).expect("a Codex model")),
        second: Recorded::new(json!({"protocol": "software.change/1"})),
    };
    let result = call_tool(&model, INSTRUCTIONS, items(), tool()).await;
    assert_eq!(
        result.expect("the turn succeeded"),
        json!({"protocol": "software.change/1"})
    );
    assert_no_request(&unused, "a missing login reached the server").await;
}

/// A model that runs its inner turn on a spawned task.
struct Spawning {
    inner: Arc<dyn Model>,
}

impl Model for Spawning {
    fn provenance(&self) -> &Provenance {
        self.inner.provenance()
    }
    fn capabilities(&self) -> &Capabilities {
        self.inner.capabilities()
    }
    fn turn<'a>(
        &'a self,
        request: &'a TurnRequest,
        _sink: &'a mut dyn StreamSink,
        _cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        let inner = Arc::clone(&self.inner);
        let request = request.clone();
        Box::pin(async move {
            tokio::spawn(async move { inner.turn(&request, &mut Discard, &Cancel::new()).await })
                .await
                .expect("the spawned turn")
        })
    }
}

/// A missing login refused on a spawned task is still a missing login.
#[tokio::test]
#[ignore = "declined in wave 2026-10-04-w17: a refusal raised on a spawned task is outside the task-local slot; documented on call_tool"]
async fn a_refusal_on_a_spawned_task_is_still_a_credential_refusal() {
    let (unused, url) = listener().await;
    let absent = fixture_dir("spawned-absent").join("auth.json");
    let model = Spawning {
        inner: Arc::new(codex_model_at(MODEL, &url, &absent).expect("a Codex model")),
    };
    let error = call_tool(&model, INSTRUCTIONS, items(), tool())
        .await
        .expect_err("no login, no call");
    assert!(
        matches!(error, ModelError::MissingCredential(_)),
        "{error:?}"
    );
    assert_no_request(&unused, "a missing login reached the server").await;
}

/// An identifier llm refuses is a setup error; one with path characters stays in the body and
/// never reaches the request path.
#[tokio::test]
async fn model_identifiers_are_validated_and_never_reach_the_path() {
    let auth = auth_with_token("model-id", VALID_JWT);
    for bad in ["gpt 5", "", "gpt-5\n"] {
        let refused = codex_model_at(bad, "http://127.0.0.1:9/backend-api/codex", &auth);
        assert!(
            matches!(refused, Err(ModelError::Setup(_))),
            "{bad:?} was not refused as a setup error"
        );
    }
    let odd = "../../v1/models?x=1";
    let (socket, url) = listener().await;
    let call = function_call("fc_1", "call_1", "{\"protocol\":\"software.change/1\"}");
    let mut added = call.clone();
    added["arguments"] = json!("");
    let stream = sse(&[
        json!({"type": "response.created", "response": {"id": "resp_6", "status": "in_progress"}}),
        json!({"type": "response.output_item.added", "output_index": 0, "item": added}),
        json!({"type": "response.output_item.done", "output_index": 0, "item": call}),
        json!({"type": "response.completed", "response": {
            "id": "resp_6", "model": odd, "status": "completed", "output": [call],
            "usage": {"input_tokens": 20, "output_tokens": 9}}}),
    ]);
    let server = serve(socket, with_head(SSE_HEAD, &stream));
    let model = codex_model_at(odd, &url, &auth).expect("printable ASCII is an identifier");
    let _ = call_tool(&model, INSTRUCTIONS, items(), tool()).await;
    let captured = server.await.expect("the fixture server");
    assert!(
        captured.starts_with("POST /backend-api/codex/responses "),
        "{captured}"
    );
    let (_, body) = captured.split_once("\r\n\r\n").expect("a body");
    let body: Value = serde_json::from_str(body).expect("JSON");
    assert_eq!(body["model"], json!(odd));
}
