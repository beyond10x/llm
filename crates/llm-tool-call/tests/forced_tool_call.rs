//! Acceptance for story call-tool-helper: one forced tool call through the llm crates, ported
//! from Loom's `intake-model` (Loom story `model-access`).
//!
//! Every model here is either the Codex Responses model pointed at a local fixture server on
//! `127.0.0.1`, or a recorded fake. Every credential is a fixture `auth.json` written under
//! `CARGO_TARGET_TMPDIR`; no test reads the operator's Codex login, the process environment or the
//! network.

use b10x_llm_tool_call::{CODEX_BASE_URL, ModelError, call_tool, codex_auth_path, codex_model_at};
use llm_core::{
    BoxFuture, CallId, Cancel, Capabilities, Error, Id, Item, Model, Protocol, Provenance,
    StopReason, StreamSink, ToolCall, ToolChoice, ToolName, ToolSpec, TurnObservation, TurnOutcome,
    TurnRequest,
};
use serde_json::{Value, json};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};

/// The model the fixture server answers as.
const MODEL: &str = "gpt-5.1-codex";

/// The tool every case forces.
const TOOL: &str = "propose_protocol";

/// A JWT whose `exp` is 2100-01-01: `{"alg":"none","typ":"JWT"}` . `{"exp":4102444800}` . `fixture`.
const VALID_JWT: &str = "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.eyJleHAiOjQxMDI0NDQ4MDB9.fixture";

/// A JWT whose `exp` is one second after the Unix epoch.
const EXPIRED_JWT: &str = "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.eyJleHAiOjF9.fixture";

/// One Responses answer carrying a single `function_call` to [`TOOL`].
const CALL_STREAM: &str = "event: response.created\n\
data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"status\":\"in_progress\"}}\n\n\
event: response.output_item.added\n\
data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"propose_protocol\",\"arguments\":\"\"}}\n\n\
event: response.function_call_arguments.delta\n\
data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"output_index\":0,\"delta\":\"{\\\"protocol\\\":\"}\n\n\
event: response.function_call_arguments.delta\n\
data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"output_index\":0,\"delta\":\"\\\"software.change/1\\\"}\"}\n\n\
event: response.function_call_arguments.done\n\
data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc_1\",\"output_index\":0,\"arguments\":\"{\\\"protocol\\\":\\\"software.change/1\\\"}\"}\n\n\
event: response.output_item.done\n\
data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"propose_protocol\",\"arguments\":\"{\\\"protocol\\\":\\\"software.change/1\\\"}\",\"status\":\"completed\"}}\n\n\
event: response.completed\n\
data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5.1-codex\",\"status\":\"completed\",\"output\":[{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"propose_protocol\",\"arguments\":\"{\\\"protocol\\\":\\\"software.change/1\\\"}\",\"status\":\"completed\"}],\"usage\":{\"input_tokens\":20,\"output_tokens\":9}}}\n\n";

/// One Responses answer that is text only: no tool call at all.
const TEXT_STREAM: &str = "event: response.created\n\
data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_2\",\"status\":\"in_progress\"}}\n\n\
event: response.output_text.delta\n\
data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"delta\":\"I would pick\"}\n\n\
event: response.output_text.delta\n\
data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"delta\":\" software.change/1\"}\n\n\
event: response.completed\n\
data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_2\",\"model\":\"gpt-5.1-codex\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"I would pick software.change/1\"}]}],\"usage\":{\"input_tokens\":20,\"output_tokens\":6}}}\n\n";

/// One Responses answer carrying a single `function_call` to a tool nobody offered.
const OTHER_TOOL_STREAM: &str = "event: response.created\n\
data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_3\",\"status\":\"in_progress\"}}\n\n\
event: response.output_item.added\n\
data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"open_case\",\"arguments\":\"\"}}\n\n\
event: response.function_call_arguments.done\n\
data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc_1\",\"output_index\":0,\"arguments\":\"{\\\"case\\\":\\\"now\\\"}\"}\n\n\
event: response.output_item.done\n\
data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"open_case\",\"arguments\":\"{\\\"case\\\":\\\"now\\\"}\",\"status\":\"completed\"}}\n\n\
event: response.completed\n\
data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_3\",\"model\":\"gpt-5.1-codex\",\"status\":\"completed\",\"output\":[{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"open_case\",\"arguments\":\"{\\\"case\\\":\\\"now\\\"}\",\"status\":\"completed\"}],\"usage\":{\"input_tokens\":20,\"output_tokens\":9}}}\n\n";

const SSE_HEAD: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";

const INSTRUCTIONS: &str = "Propose the protocol this intent runs under.";

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

/// A fresh directory for one case's fixture credential, under the build's own scratch area.
fn fixture_dir(case: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("llm-tool-call-forced")
        .join(format!("{case}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a fixture directory");
    dir
}

/// Writes a Codex-shaped `auth.json` holding `token` and returns its absolute path.
fn auth_file(case: &str, token: &str) -> PathBuf {
    let path = fixture_dir(case).join("auth.json");
    let document = json!({
        "OPENAI_API_KEY": null,
        "tokens": {
            "id_token": token,
            "access_token": token,
            "refresh_token": "fixture-refresh",
            "account_id": "fixture-account"
        },
        "last_refresh": "2026-10-04T00:00:00Z"
    });
    fs::write(&path, document.to_string()).expect("a fixture credential");
    path
}

/// A bound local port and the base URL that addresses it, shaped like the Codex backend's.
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

/// Bounded so that a client which never connects fails the test instead of hanging it.
async fn accept(listener: &TcpListener) -> TcpStream {
    tokio::time::timeout(Duration::from_secs(10), listener.accept())
        .await
        .expect("the client never opened a connection")
        .expect("an accepted connection")
        .0
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

/// Asserts that no connection arrives within a short window.
///
/// A connection the client opened earlier waits in the listen backlog, so this also sees one made
/// before it was called.
async fn assert_no_request(listener: &TcpListener, why: &str) {
    assert!(
        tokio::time::timeout(Duration::from_millis(150), listener.accept())
            .await
            .is_err(),
        "{why}"
    );
}

/// What the fixture server does with the one request it accepts.
enum Reply {
    /// Answers with this Responses stream.
    Stream(&'static str),
    /// Reads the request, then closes the connection without answering.
    Close,
}

/// Serves exactly one request, refuses to see a second, and returns the request it read.
fn serve(listener: TcpListener, reply: Reply) -> JoinHandle<String> {
    tokio::spawn(async move {
        let mut socket = accept(&listener).await;
        let captured = read_request(&mut socket).await;
        match reply {
            Reply::Stream(stream) => {
                socket.write_all(SSE_HEAD).await.expect("head written");
                socket
                    .write_all(stream.as_bytes())
                    .await
                    .expect("stream written");
                socket.shutdown().await.expect("closed");
            }
            Reply::Close => drop(socket),
        }
        assert_no_request(&listener, "the request was attempted a second time").await;
        captured
    })
}

/// Splits a captured request into its lower-cased head and its JSON body.
fn parts(captured: &str) -> (String, Value) {
    let (head, body) = captured.split_once("\r\n\r\n").expect("a head and a body");
    (
        head.to_ascii_lowercase(),
        serde_json::from_str(body).expect("a JSON body"),
    )
}

#[tokio::test]
async fn a_forced_tool_call_returns_its_arguments() {
    assert_eq!(CODEX_BASE_URL, "https://chatgpt.com/backend-api/codex");
    let auth = auth_file("call", VALID_JWT);

    // One `function_call` to the forced tool: its arguments come back as JSON.
    let (socket, url) = listener().await;
    let server = serve(socket, Reply::Stream(CALL_STREAM));
    let model = codex_model_at(MODEL, &url, &auth).expect("a Codex model");
    let arguments = call_tool(&model, INSTRUCTIONS, items(), tool())
        .await
        .expect("the forced call's arguments");
    assert_eq!(arguments, json!({"protocol": "software.change/1"}));
    let (head, body) = parts(&server.await.expect("the fixture server"));
    assert!(
        head.starts_with("post /backend-api/codex/responses "),
        "{head}"
    );
    assert!(
        head.lines()
            .any(|line| line == format!("authorization: bearer {}", VALID_JWT.to_ascii_lowercase())),
        "the access token of the fixture login is the bearer: {head}"
    );
    assert_eq!(body["model"], json!(MODEL));
    assert_eq!(body["tools"][0]["name"], json!(TOOL));
    assert_eq!(
        body["tool_choice"],
        json!({"type": "function", "name": TOOL})
    );
    assert!(
        body.get("max_output_tokens").is_none(),
        "the Codex backend may refuse an output limit: {body}"
    );
    assert!(body.to_string().contains(INSTRUCTIONS), "{body}");

    // A stream with no tool call at all.
    let (socket, url) = listener().await;
    let server = serve(socket, Reply::Stream(TEXT_STREAM));
    let model = codex_model_at(MODEL, &url, &auth).expect("a Codex model");
    let error = call_tool(&model, INSTRUCTIONS, items(), tool())
        .await
        .expect_err("text is not a forced call");
    assert!(matches!(error, ModelError::NoToolCall), "{error:?}");
    server.await.expect("the fixture server");

    // A call to a tool other than the forced one.
    let (socket, url) = listener().await;
    let server = serve(socket, Reply::Stream(OTHER_TOOL_STREAM));
    let model = codex_model_at(MODEL, &url, &auth).expect("a Codex model");
    let error = call_tool(&model, INSTRUCTIONS, items(), tool())
        .await
        .expect_err("another tool is not the forced call");
    assert!(matches!(error, ModelError::WrongTool), "{error:?}");
    server.await.expect("the fixture server");

    // The fixture reads the request and closes the connection without answering.
    let (socket, url) = listener().await;
    let server = serve(socket, Reply::Close);
    let model = codex_model_at(MODEL, &url, &auth).expect("a Codex model");
    let error = call_tool(&model, INSTRUCTIONS, items(), tool())
        .await
        .expect_err("a closed connection is no answer");
    assert!(matches!(error, ModelError::Transport(_)), "{error:?}");
    server.await.expect("the fixture server");

    // No credential file: refused before any request. Building the model reads nothing, as llm's
    // credential reads the file on every request and never at construction.
    let (socket, url) = listener().await;
    let absent = fixture_dir("absent").join("auth.json");
    let model = codex_model_at(MODEL, &url, &absent).expect("construction reads no file");
    let error = call_tool(&model, INSTRUCTIONS, items(), tool())
        .await
        .expect_err("no login, no call");
    assert!(
        matches!(error, ModelError::MissingCredential(_)),
        "{error:?}"
    );
    assert_no_request(&socket, "a missing credential still reached the server").await;

    // An expired token: refused before any request, and the message says how to renew it.
    let (socket, url) = listener().await;
    let expired = auth_file("expired", EXPIRED_JWT);
    let model = codex_model_at(MODEL, &url, &expired).expect("construction reads no file");
    let error = call_tool(&model, INSTRUCTIONS, items(), tool())
        .await
        .expect_err("an expired login, no call");
    assert!(
        matches!(error, ModelError::ExpiredCredential(_)),
        "{error:?}"
    );
    let message = error.to_string();
    assert!(message.contains("run `codex`"), "{message}");
    assert!(
        !message.contains(EXPIRED_JWT),
        "the token leaked: {message}"
    );
    assert_no_request(&socket, "an expired credential still reached the server").await;
}

/// The Codex login is `$CODEX_HOME/auth.json`, else `$HOME/.codex/auth.json`, else refused.
#[test]
fn the_codex_login_is_found_under_codex_home_then_home() {
    let codex_home = Some(OsStr::new("/srv/codex-home"));
    let home = Some(OsStr::new("/srv/operator"));
    assert_eq!(
        codex_auth_path(codex_home, home).expect("a path"),
        PathBuf::from("/srv/codex-home/auth.json")
    );
    assert_eq!(
        codex_auth_path(None, home).expect("a path"),
        PathBuf::from("/srv/operator/.codex/auth.json")
    );
    let error = codex_auth_path(None, None).expect_err("nowhere to look");
    assert!(
        matches!(error, ModelError::MissingCredential(_)),
        "{error:?}"
    );
}

/// A recorded model: answers every turn with one fixed outcome and keeps the requests it saw.
struct Recorded {
    provenance: Provenance,
    capabilities: Capabilities,
    answer: Vec<Item>,
    stop_reason: StopReason,
    seen: Mutex<Vec<TurnRequest>>,
}

impl Recorded {
    fn new(answer: Vec<Item>, stop_reason: StopReason) -> Self {
        let id = |value: &str| Id::new(value).expect("fixture identifier");
        Self {
            provenance: Provenance {
                protocol: Protocol::Responses,
                provider: id("recorded"),
                account: id("recorded"),
                endpoint: id("recorded"),
                model: id("recorded-model"),
                binding_revision: id("rev-1"),
            },
            capabilities: Capabilities {
                tools: true,
                tool_choice: true,
                temperature: false,
                top_p: false,
                reasoning_efforts: Vec::new(),
                context_window: 32_768,
                max_output_tokens: 2_048,
            },
            answer,
            stop_reason,
            seen: Mutex::new(Vec::new()),
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
        request: &'a TurnRequest,
        _sink: &'a mut dyn StreamSink,
        _cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        self.seen.lock().expect("unpoisoned").push(request.clone());
        let mut observation = TurnObservation::new(self.provenance.clone());
        observation.final_usage = true;
        let outcome = TurnOutcome {
            stop_reason: self.stop_reason.clone(),
            items: self.answer.clone(),
            observation,
        };
        Box::pin(async move { Ok(outcome) })
    }
}

fn recorded_call(name: &str, arguments: Value) -> Item {
    Item::ToolCall(ToolCall {
        call_id: CallId::new("call_1").expect("call id"),
        name: ToolName::new(name).expect("tool name"),
        arguments,
    })
}

/// Other crates test against a recorded model, which nothing in llm checks: `call_tool` holds
/// its answer to the forced tool itself.
#[tokio::test]
async fn a_recorded_model_is_held_to_the_forced_tool() {
    let model = Recorded::new(
        vec![recorded_call(
            TOOL,
            json!({"protocol": "software.change/1"}),
        )],
        StopReason::ToolCalls,
    );
    let arguments = call_tool(&model, INSTRUCTIONS, items(), tool())
        .await
        .expect("the forced call's arguments");
    assert_eq!(arguments, json!({"protocol": "software.change/1"}));
    let seen = model.seen.lock().expect("unpoisoned").clone();
    assert_eq!(seen.len(), 1, "one turn, no retry");
    let request = &seen[0];
    assert_eq!(request.model, "recorded-model");
    assert_eq!(request.instructions, INSTRUCTIONS);
    assert_eq!(request.items, items());
    assert_eq!(request.tools, vec![tool()]);
    assert_eq!(
        request.tool_choice,
        ToolChoice::Named(ToolName::new(TOOL).expect("tool name"))
    );
    assert_eq!(request.max_output_tokens, None);

    let model = Recorded::new(vec![Item::assistant("no call")], StopReason::EndTurn);
    let error = call_tool(&model, INSTRUCTIONS, items(), tool())
        .await
        .expect_err("text is not a forced call");
    assert!(matches!(error, ModelError::NoToolCall), "{error:?}");

    let model = Recorded::new(
        vec![recorded_call("open_case", json!({"case": "now"}))],
        StopReason::ToolCalls,
    );
    let error = call_tool(&model, INSTRUCTIONS, items(), tool())
        .await
        .expect_err("another tool is not the forced call");
    assert!(matches!(error, ModelError::WrongTool), "{error:?}");
}
