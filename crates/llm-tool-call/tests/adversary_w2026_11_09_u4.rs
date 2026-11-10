//! Adversary cases for story `call-tool-spawned-refusal-keeps-its-class` (wave 2026-11-09, unit 4).
//!
//! The fix made every Codex model's account identifier unique (`codex-subscription-N`) and added a
//! refusal slot shared by every call on one model. These cases attack both halves: the binding
//! coordinate two models of one login used to share, and the shared slot under concurrent calls.
//! Every credential is a fixture `auth.json` under `CARGO_TARGET_TMPDIR`; every endpoint is a
//! fixture server on `127.0.0.1`.

use llm_core::{
    BoxFuture, Cancel, Capabilities, Error, Item, Model, Provenance, StreamSink, ToolName,
    ToolSpec, TurnOutcome, TurnRequest,
};
use llm_tool_call::{ModelError, call_tool, codex_model_at};
use serde_json::{Value, json};
use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Notify, oneshot},
};

const MODEL: &str = "gpt-5.1-codex";
const TOOL: &str = "propose_protocol";
const INSTRUCTIONS: &str = "Propose the protocol this intent runs under.";

/// A JWT whose `exp` is 2100-01-01.
const VALID_JWT: &str = "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.eyJleHAiOjQxMDI0NDQ4MDB9.fixture";

const SSE_HEAD: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";

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
        .join("llm-tool-call-adversary-w2026-11-09-u4")
        .join(format!("{case}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a fixture directory");
    dir
}

fn valid_login(case: &str) -> PathBuf {
    let path = fixture_dir(case).join("auth.json");
    let document = json!({
        "OPENAI_API_KEY": null,
        "tokens": {"id_token": VALID_JWT, "access_token": VALID_JWT, "refresh_token": "r", "account_id": "a"}
    });
    fs::write(&path, document.to_string()).expect("a fixture credential");
    path
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

/// Reads one request head and its `content-length` body.
async fn read_request(socket: &mut TcpStream) {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        let length = socket.read(&mut buffer).await.expect("a readable socket");
        assert!(length > 0, "the client closed before sending a request");
        bytes.extend_from_slice(&buffer[..length]);
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
                return;
            }
        }
    }
}

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

/// The opening of an answer that starts one call to the forced tool and is never finished.
fn unfinished_answer() -> Vec<u8> {
    let body = sse(&[
        json!({"type": "response.created", "response": {"id": "resp_9", "status": "in_progress"}}),
        json!({"type": "response.output_item.added", "output_index": 0,
               "item": {"type": "function_call", "id": "fc_1", "call_id": "call_1", "name": TOOL, "arguments": ""}}),
    ]);
    let mut reply = SSE_HEAD.to_vec();
    reply.extend_from_slice(body.as_bytes());
    reply
}

/// Two Codex models of one login, one model id and one endpoint are one serving binding: opaque
/// continuation state the first produced goes back to the second. Before this unit both carried
/// account `codex-subscription` and the state crossed; a caller that builds its model per request
/// (control-plane's `CodexAgentModel::respond_in` calls `codex_model` on every request and hashes
/// `provenance()` into its continuation wire) depends on that.
#[tokio::test]
async fn opaque_state_crosses_two_models_of_one_login() {
    let (_unused, url) = listener().await;
    let absent = fixture_dir("opaque-crossing").join("auth.json");
    let first = codex_model_at(MODEL, &url, &absent).expect("a Codex model");
    let second = codex_model_at(MODEL, &url, &absent).expect("the same Codex model again");
    let mut conversation = items();
    conversation.push(Item::Opaque {
        provenance: first.provenance().clone(),
        payload: json!({"type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "e"}),
    });
    let error = call_tool(&second, INSTRUCTIONS, conversation, tool())
        .await
        .expect_err("no login, no call");
    assert!(
        matches!(error, ModelError::MissingCredential(_)),
        "the state of one binding came back refused as another binding's: {error:?}"
    );
}

/// Two calls at once on one plain (not spawning) Codex model: the first is sent and its stream is
/// cut, a Transport failure; meanwhile the login is removed and the second is refused as a missing
/// login. Each call keeps its own class, as the task-local slot alone guaranteed.
#[tokio::test]
async fn a_concurrent_calls_refusal_is_not_another_calls_failure() {
    let auth = valid_login("concurrent-cross");
    let (socket, url) = listener().await;
    let (arrived, request_arrived) = oneshot::channel::<()>();
    let (release, released) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let mut stream = accept(&socket).await;
        read_request(&mut stream).await;
        arrived.send(()).expect("the driver waits");
        released.await.expect("the driver releases");
        stream.write_all(&unfinished_answer()).await.expect("reply written");
        stream.shutdown().await.expect("closed");
    });
    let model = codex_model_at(MODEL, &url, &auth).expect("a Codex model");
    let sent = call_tool(&model, INSTRUCTIONS, items(), tool());
    let refused = async {
        request_arrived.await.expect("the first request arrived");
        fs::remove_file(&auth).expect("the login removed");
        let refused = call_tool(&model, INSTRUCTIONS, items(), tool()).await;
        release.send(()).expect("the server waits");
        refused
    };
    let (sent, refused) = tokio::join!(sent, refused);
    server.await.expect("the fixture server");
    assert!(
        matches!(refused, Err(ModelError::MissingCredential(_))),
        "{refused:?}"
    );
    assert!(
        matches!(sent, Err(ModelError::Transport(_))),
        "a call whose login was read and whose stream was cut came back as {sent:?}"
    );
}

/// A model that runs its inner turn on a spawned task, ordering two calls: the first is refused
/// on its spawned task, then the second call starts, then the first returns.
struct Ordered {
    inner: Arc<dyn Model>,
    calls: AtomicUsize,
    first_refused: Notify,
    second_started: Notify,
    first_returned: Notify,
}

impl Model for Ordered {
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
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            if call > 0 {
                self.second_started.notify_one();
                self.first_returned.notified().await;
            }
            let result = tokio::spawn(async move {
                let mut sink = Discard;
                inner.turn(&request, &mut sink, &Cancel::new()).await
            })
            .await
            .expect("the spawned turn");
            if call == 0 {
                self.first_refused.notify_one();
                self.second_started.notified().await;
            }
            result
        })
    }
}

struct Discard;

impl StreamSink for Discard {
    fn emit(&mut self, _event: llm_core::StreamEvent) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async { Ok(()) })
    }
}

/// Two calls at once on one spawning model, both refused for the same missing login: both are a
/// missing login, whichever starts while the other is in flight.
#[tokio::test]
async fn two_concurrent_spawned_refusals_are_both_credential_refusals() {
    let (_unused, url) = listener().await;
    let absent = fixture_dir("spawned-concurrent").join("auth.json");
    let model = Ordered {
        inner: Arc::new(codex_model_at(MODEL, &url, &absent).expect("a Codex model")),
        calls: AtomicUsize::new(0),
        first_refused: Notify::new(),
        second_started: Notify::new(),
        first_returned: Notify::new(),
    };
    let first = async {
        let result = call_tool(&model, INSTRUCTIONS, items(), tool()).await;
        model.first_returned.notify_one();
        result
    };
    let second = async {
        model.first_refused.notified().await;
        call_tool(&model, INSTRUCTIONS, items(), tool()).await
    };
    let (first, second) = tokio::join!(first, second);
    assert!(
        matches!(second, Err(ModelError::MissingCredential(_))),
        "{second:?}"
    );
    assert!(
        matches!(first, Err(ModelError::MissingCredential(_))),
        "the first call's refusal was cleared by the second call's start: {first:?}"
    );
}
