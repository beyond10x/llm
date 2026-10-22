//! The `llm` binary against loopback fixture servers: every stream here is synthetic, and no
//! test reaches a provider.

use base64::{Engine, engine::general_purpose::STANDARD};
use llm_bedrock::eventstream::{HeaderValue, Message, encode_message};
use serde_json::{Value, json};
use std::{
    fmt::Write as _,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output, Stdio},
    sync::{Arc, Mutex},
    thread,
};

const SSE: &str = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n";
const EVENT_STREAM: &str = "HTTP/1.1 200 OK\r\ncontent-type: application/vnd.amazon.eventstream\r\n\
connection: close\r\n\r\n";
const ACCESS_KEY_ID: &str = "FIXTUREKEYIDENTIFIER";
const SECRET_ACCESS_KEY: &str = "fixture-secret-access-key";
const CONTAINER_TOKEN: &str = "fixture-container-token";
const API_KEY: &str = "fixture-api-key";

/// One request a fixture server received.
#[derive(Debug, Clone)]
struct Received {
    line: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Received {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap()
    }
}

/// A loopback server that answers its connections, in order, with `answers`, and keeps what
/// each one sent.
struct Fixture {
    origin: String,
    received: Arc<Mutex<Vec<Received>>>,
}

impl Fixture {
    fn new(answers: Vec<Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let received = Arc::new(Mutex::new(Vec::new()));
        let kept = received.clone();
        thread::spawn(move || {
            for answer in answers {
                let Ok((mut socket, _)) = listener.accept() else {
                    return;
                };
                let request = read_request(&mut socket);
                kept.lock().unwrap().push(request);
                let _ = socket.write_all(&answer);
                let _ = socket.flush();
            }
        });
        Self { origin, received }
    }

    fn received(&self) -> Vec<Received> {
        self.received.lock().unwrap().clone()
    }
}

fn read_request(socket: &mut std::net::TcpStream) -> Received {
    let mut reader = BufReader::new(socket);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let mut headers = Vec::new();
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).unwrap();
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        let (name, value) = header.split_once(':').unwrap();
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
    }
    let length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .map_or(0, |(_, value)| value.parse::<usize>().unwrap());
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    Received {
        line: line.trim_end().to_owned(),
        headers,
        body,
    }
}

fn answer(head: &str, body: &[u8]) -> Vec<u8> {
    let mut answer = head.as_bytes().to_vec();
    answer.extend_from_slice(body);
    answer
}

fn sse(events: &[(&str, Value)]) -> Vec<u8> {
    let mut body = String::new();
    for (name, data) in events {
        let _ = write!(body, "event: {name}\ndata: {data}\n\n");
    }
    answer(SSE, body.as_bytes())
}

fn messages_stream() -> Vec<u8> {
    sse(&[
        (
            "message_start",
            json!({"type":"message_start","message":{"id":"msg_fixture","type":"message",
                "role":"assistant","model":"claude-haiku-5-5","content":[],"stop_reason":null,
                "stop_sequence":null,"usage":{"input_tokens":12,"output_tokens":1,
                "cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}),
        ),
        (
            "content_block_start",
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        ),
        (
            "content_block_delta",
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Guten "}}),
        ),
        (
            "content_block_delta",
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Tag"}}),
        ),
        (
            "content_block_stop",
            json!({"type":"content_block_stop","index":0}),
        ),
        (
            "message_delta",
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},
                "usage":{"output_tokens":4}}),
        ),
        ("message_stop", json!({"type":"message_stop"})),
    ])
}

fn responses_stream() -> Vec<u8> {
    let done = json!({"type":"message","id":"msg_1","role":"assistant","status":"completed",
        "content":[{"type":"output_text","text":"Hello there","annotations":[]}]});
    sse(&[
        (
            "response.created",
            json!({"type":"response.created","response":{"id":"resp_1","status":"in_progress"}}),
        ),
        (
            "response.output_item.added",
            json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message",
                "id":"msg_1","role":"assistant","status":"in_progress","content":[]}}),
        ),
        (
            "response.output_text.delta",
            json!({"type":"response.output_text.delta","item_id":"msg_1","output_index":0,
                "content_index":0,"delta":"Hello"}),
        ),
        (
            "response.output_text.delta",
            json!({"type":"response.output_text.delta","item_id":"msg_1","output_index":0,
                "content_index":0,"delta":" there"}),
        ),
        (
            "response.output_item.done",
            json!({"type":"response.output_item.done","output_index":0,"item":done}),
        ),
        (
            "response.completed",
            json!({"type":"response.completed","response":{"id":"resp_1","model":"gpt-fixture",
                "status":"completed","output":[done],"usage":{"input_tokens":5,"output_tokens":2}}}),
        ),
    ])
}

/// A Chat Completions stream that reports no usage at all.
fn chat_stream() -> Vec<u8> {
    let chunk = |delta: Value, finish: Value| {
        json!({"id":"chatcmpl-fixture","object":"chat.completion.chunk","model":"small",
            "choices":[{"index":0,"delta":delta,"finish_reason":finish}]})
    };
    let mut body = String::new();
    for data in [
        chunk(json!({"role":"assistant","content":""}), Value::Null),
        chunk(json!({"content":"Moin"}), Value::Null),
        chunk(json!({"content":" moin"}), Value::Null),
        chunk(json!({}), json!("stop")),
    ] {
        let _ = write!(body, "data: {data}\n\n");
    }
    body.push_str("data: [DONE]\n\n");
    answer(SSE, body.as_bytes())
}

fn frame(headers: &[(&str, &str)], payload: Vec<u8>) -> Vec<u8> {
    encode_message(&Message {
        headers: headers
            .iter()
            .map(|(name, value)| ((*name).to_owned(), HeaderValue::String((*value).to_owned())))
            .collect(),
        payload,
    })
}

fn converse_event(kind: &str, payload: &Value) -> Vec<u8> {
    frame(
        &[
            (":event-type", kind),
            (":content-type", "application/json"),
            (":message-type", "event"),
        ],
        serde_json::to_vec(payload).unwrap(),
    )
}

fn converse_stream() -> Vec<u8> {
    let mut body = Vec::new();
    for (kind, payload) in [
        ("messageStart", json!({"role": "assistant"})),
        (
            "contentBlockDelta",
            json!({"contentBlockIndex": 0, "delta": {"text": "Opus "}}),
        ),
        (
            "contentBlockDelta",
            json!({"contentBlockIndex": 0, "delta": {"text": "speaking"}}),
        ),
        ("contentBlockStop", json!({"contentBlockIndex": 0})),
        ("messageStop", json!({"stopReason": "end_turn"})),
        (
            "metadata",
            json!({"usage": {"inputTokens": 9, "outputTokens": 2, "totalTokens": 11}}),
        ),
    ] {
        body.extend(converse_event(kind, &payload));
    }
    answer(EVENT_STREAM, &body)
}

fn invoke_stream() -> Vec<u8> {
    let mut body = Vec::new();
    for event in [
        json!({"type":"message_start","message":{"id":"msg_bedrock","type":"message",
            "role":"assistant","content":[],"model":"claude-sonnet-5-5","stop_reason":null,
            "stop_sequence":null,"usage":{"input_tokens":7,"output_tokens":1}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Sonnet "}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"here"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},
            "usage":{"output_tokens":3}}),
        json!({"type":"message_stop"}),
    ] {
        let envelope = json!({"bytes": STANDARD.encode(event.to_string())});
        body.extend(frame(
            &[
                (":event-type", "chunk"),
                (":content-type", "application/json"),
                (":message-type", "event"),
            ],
            envelope.to_string().into_bytes(),
        ));
    }
    answer(EVENT_STREAM, &body)
}

fn container_answer() -> Vec<u8> {
    let body = json!({
        "AccessKeyId": ACCESS_KEY_ID,
        "SecretAccessKey": SECRET_ACCESS_KEY,
        "Token": "fixture-session-token",
        "Expiration": "2099-01-01T00:00:00Z",
    })
    .to_string();
    answer(
        &format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\
             connection: close\r\n\r\n",
            body.len()
        ),
        body.as_bytes(),
    )
}

/// Runs `llm` with only `env` set, and `stdin` as its input.
fn llm(args: &[&str], env: &[(&str, &str)], stdin: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_llm"));
    command.env_clear().args(args).envs(env.iter().copied());
    command.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    if let Some(input) = stdin {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
    }
    child.wait_with_output().unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Three routes on the three HTTP wires, at `origin`: `haiku` aliased and priced, the other two
/// unpriced.
fn three_routes(origin: &str) -> String {
    format!(
        r#"format = "llm.routes/1"

[endpoints.anthropic]
base_url = "{origin}/v1"
protocol = "messages"
auth = {{ kind = "api-key", header = "x-api-key", secret = "env:FIXTURE_ANTHROPIC_KEY" }}

[endpoints.openai]
base_url = "{origin}/v1"
protocol = "responses"
auth = {{ kind = "bearer", secret = "env:FIXTURE_OPENAI_KEY" }}

[endpoints.lab]
base_url = "{origin}/v1"
protocol = "chat-completions"
auth = {{ kind = "anonymous" }}

[routes.haiku]
model = "anthropic/claude-haiku-5-5"
aliases = ["fast", "claude-haiku"]
price = {{ currency = "USD", input = "1", cached_input = "0.1", cache_creation_input = "1.25", output = "5" }}

[routes.gpt]
model = "openai/gpt-fixture"

[routes.small]
model = "lab/small"
"#
    )
}

fn configured(text: &str) -> (tempfile::TempDir, String) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("llm.toml");
    write(&path, text);
    let path = path.to_str().unwrap().to_owned();
    (directory, path)
}

const KEYS: [(&str, &str); 2] = [
    ("FIXTURE_ANTHROPIC_KEY", API_KEY),
    ("FIXTURE_OPENAI_KEY", API_KEY),
];

#[test]
fn model_list_prints_each_route_with_its_aliases_and_price_or_unknown() {
    let (_directory, path) = configured(&three_routes("http://127.0.0.1:9"));
    let output = llm(&["model", "list", "--config", &path], &[], None);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let text = stdout(&output);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 4, "{text}");
    assert!(lines[0].starts_with("ID"), "{text}");
    let row = |id: &str| -> Vec<&str> {
        lines
            .iter()
            .find(|line| line.split_whitespace().next() == Some(id))
            .unwrap()
            .split_whitespace()
            .collect()
    };
    assert_eq!(
        row("haiku"),
        [
            "haiku",
            "fast,claude-haiku",
            "messages",
            "anthropic",
            "127.0.0.1:9",
            "claude-haiku-5-5",
            "1/5",
            "USD"
        ]
    );
    assert_eq!(
        row("gpt"),
        [
            "gpt",
            "-",
            "responses",
            "openai",
            "127.0.0.1:9",
            "gpt-fixture",
            "unknown"
        ]
    );
    assert_eq!(
        row("small"),
        [
            "small",
            "-",
            "chat-completions",
            "lab",
            "127.0.0.1:9",
            "small",
            "unknown"
        ]
    );
}

#[test]
fn model_list_as_json_carries_the_same_rows_for_scripts() {
    let (_directory, path) = configured(&three_routes("http://127.0.0.1:9"));
    let output = llm(
        &["--config", &path, "model", "list", "--format", "json"],
        &[],
        None,
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let rows: Value = serde_json::from_str(&stdout(&output)).unwrap();
    let haiku = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "haiku")
        .unwrap();
    assert_eq!(haiku["aliases"], json!(["fast", "claude-haiku"]));
    assert_eq!(
        haiku["price_per_million"],
        json!({"currency": "USD", "input": "1", "output": "5"})
    );
    let gpt = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "gpt")
        .unwrap();
    assert_eq!(gpt["price_per_million"], Value::Null);
    assert_eq!(gpt["region"], Value::Null);
}

#[test]
fn infer_on_an_alias_streams_the_messages_text_and_prices_the_turn() {
    let fixture = Fixture::new(vec![messages_stream()]);
    let (_directory, path) = configured(&three_routes(&fixture.origin));
    let output = llm(
        &[
            "--config",
            &path,
            "infer",
            "--model=fast",
            "--system",
            "Be brief.",
            "--max-output-tokens",
            "64",
            "hi",
        ],
        &KEYS,
        None,
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "Guten Tag\n");
    assert_eq!(
        stderr(&output),
        "route=haiku protocol=messages input_tokens=12 output_tokens=4 cost=0.000032 USD\n"
    );
    let received = fixture.received();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].line, "POST /v1/messages HTTP/1.1");
    assert_eq!(received[0].header("x-api-key"), Some(API_KEY));
    let body = received[0].json();
    assert_eq!(body["model"], "claude-haiku-5-5");
    assert_eq!(body["max_tokens"], 64);
    assert!(body["system"].to_string().contains("Be brief."));
    assert!(body["messages"].to_string().contains("hi"), "{body}");
}

#[test]
fn infer_on_a_responses_route_streams_its_text_and_reports_its_usage() {
    let fixture = Fixture::new(vec![responses_stream()]);
    let (_directory, path) = configured(&three_routes(&fixture.origin));
    let output = llm(
        &["--config", &path, "infer", "--model", "gpt", "hi"],
        &KEYS,
        None,
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "Hello there\n");
    let usage = stderr(&output);
    assert!(
        usage.starts_with("route=gpt protocol=responses input_tokens="),
        "{usage}"
    );
    assert!(usage.trim_end().ends_with("output_tokens=2"), "{usage}");
    let received = fixture.received();
    assert_eq!(received[0].line, "POST /v1/responses HTTP/1.1");
    assert_eq!(
        received[0].header("authorization"),
        Some(format!("Bearer {API_KEY}").as_str())
    );
}

#[test]
fn infer_reads_the_prompt_from_stdin_and_an_unreported_counter_prints_unknown() {
    let fixture = Fixture::new(vec![chat_stream()]);
    let (_directory, path) = configured(&three_routes(&fixture.origin));
    let output = llm(
        &["--config", &path, "infer", "--model", "small"],
        &[],
        Some("a prompt from stdin"),
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "Moin moin\n");
    assert_eq!(
        stderr(&output),
        "route=small protocol=chat-completions input_tokens=unknown output_tokens=unknown\n"
    );
    let received = fixture.received();
    assert_eq!(received[0].line, "POST /v1/chat/completions HTTP/1.1");
    assert!(
        received[0]
            .json()
            .to_string()
            .contains("a prompt from stdin")
    );
}

/// A Messages server that streams one text delta and then holds the connection open until the
/// test drops the sender.
fn stalled_after_one_delta() -> (String, std::sync::mpsc::Sender<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let (release, held) = std::sync::mpsc::channel::<()>();
    thread::spawn(move || {
        let Ok((mut socket, _)) = listener.accept() else {
            return;
        };
        read_request(&mut socket);
        let mut body = String::from(SSE);
        for data in [
            json!({"type":"message_start","message":{"id":"msg_fixture","type":"message",
                "role":"assistant","model":"claude-haiku-5-5","content":[],"stop_reason":null,
                "stop_sequence":null,"usage":{"input_tokens":12,"output_tokens":1}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Guten "}}),
        ] {
            let _ = write!(
                body,
                "event: {}\ndata: {data}\n\n",
                data["type"].as_str().unwrap()
            );
        }
        let _ = socket.write_all(body.as_bytes());
        let _ = socket.flush();
        let _ = held.recv_timeout(std::time::Duration::from_secs(30));
    });
    (origin, release)
}

#[test]
fn an_interrupt_ends_the_turn_with_130_and_keeps_the_text_already_shown() {
    let (origin, release) = stalled_after_one_delta();
    let (_directory, path) = configured(&three_routes(&origin));
    let mut child = Command::new(env!("CARGO_BIN_EXE_llm"))
        .env_clear()
        .envs(KEYS.iter().copied())
        .args(["--config", &path, "infer", "--model", "haiku", "hi"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut out = child.stdout.take().unwrap();
    let mut shown = Vec::new();
    let mut byte = [0; 1];
    while !shown.ends_with(b"Guten ") {
        assert_eq!(out.read(&mut byte).unwrap(), 1, "stdout ended early");
        shown.push(byte[0]);
    }
    let interrupted = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(interrupted.success());
    out.read_to_end(&mut shown).unwrap();
    let mut errors = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut errors)
        .unwrap();
    let status = child.wait().unwrap();
    drop(release);
    assert_eq!(status.code(), Some(130), "{errors}");
    assert!(String::from_utf8(shown).unwrap().starts_with("Guten "));
    assert!(!errors.contains("route=haiku"), "{errors}");
}

#[test]
fn a_provider_failure_exits_1_with_the_error_kind_and_no_provider_prose() {
    let body =
        br#"{"type":"error","error":{"type":"api_error","message":"fixture provider prose"}}"#;
    let failure = answer(
        &format!(
            "HTTP/1.1 500 Internal Server Error\r\ncontent-type: application/json\r\n\
             content-length: {}\r\nconnection: close\r\n\r\n",
            body.len()
        ),
        body,
    );
    let fixture = Fixture::new(vec![failure]);
    let (_directory, path) = configured(&three_routes(&fixture.origin));
    let output = llm(
        &["--config", &path, "infer", "--model", "haiku", "hi"],
        &KEYS,
        None,
    );
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let message = stderr(&output);
    assert!(message.starts_with("llm: route `haiku`: "), "{message}");
    assert!(!message.contains("fixture provider prose"), "{message}");
    assert!(!message.contains(API_KEY), "{message}");
    assert_eq!(stdout(&output), "");
}

/// A refusal that exits 2 and leaves the fixture server unasked.
fn refused_before_sending(document: &str, args: &[&str], env: &[(&str, &str)]) -> String {
    let fixture = Fixture::new(vec![messages_stream()]);
    let (_directory, path) = configured(&document.replace("{origin}", &fixture.origin));
    let mut all = vec!["--config", path.as_str()];
    all.extend_from_slice(args);
    let output = llm(&all, env, None);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(fixture.received().is_empty());
    assert_eq!(stdout(&output), "");
    stderr(&output)
}

#[test]
fn an_unknown_model_or_alias_is_refused_naming_it_before_any_request() {
    let message = refused_before_sending(
        &three_routes("{origin}"),
        &["infer", "--model", "opus", "hi"],
        &KEYS,
    );
    assert!(message.contains("`opus`"), "{message}");
}

#[test]
fn an_alias_claimed_by_two_routes_is_refused_naming_it_before_any_request() {
    let document = three_routes("{origin}").replace(
        "model = \"openai/gpt-fixture\"",
        "model = \"openai/gpt-fixture\"\naliases = [\"fast\"]",
    );
    let message = refused_before_sending(&document, &["infer", "--model", "fast", "hi"], &KEYS);
    assert!(message.contains("alias `fast`"), "{message}");
}

#[test]
fn a_literal_secret_in_the_file_is_refused_without_echoing_it() {
    let document = three_routes("{origin}").replace(
        "secret = \"env:FIXTURE_ANTHROPIC_KEY\"",
        "secret = \"fixture-literal-secret\"",
    );
    let message = refused_before_sending(&document, &["infer", "--model", "haiku", "hi"], &KEYS);
    assert!(message.contains("auth.secret"), "{message}");
    assert!(!message.contains("fixture-literal-secret"), "{message}");
}

#[test]
fn an_unresolved_credential_is_refused_before_any_request() {
    let message = refused_before_sending(
        &three_routes("{origin}"),
        &["infer", "--model", "haiku", "hi"],
        &[],
    );
    assert!(
        message.starts_with("llm: route `haiku`: unauthorized"),
        "{message}"
    );
}

#[test]
fn a_bedrock_route_without_a_region_with_two_credential_sources_or_a_literal_key_is_refused() {
    let endpoint = |table: &str| {
        format!(
            "format = \"llm.routes/1\"\n[endpoints.bedrock]\nprotocol = \"bedrock-converse\"\n\
             {table}\n[routes.opus]\nmodel = \"bedrock/eu.anthropic.claude-opus-5-5\"\n"
        )
    };
    for (table, field) in [
        ("auth = { kind = \"aws-container\" }", "`bedrock.region`"),
        (
            "bedrock = { region = \"eu-central-1\" }\n\
             auth = { kind = \"aws-container\", access_key_id = \"env:FIXTURE_KEY_ID\" }",
            "`auth.access_key_id`",
        ),
        (
            "bedrock = { region = \"eu-central-1\" }\n\
             auth = { kind = \"aws-keys\", access_key_id = \"fixture-literal-key\", \
             secret_access_key = \"env:FIXTURE_SECRET\" }",
            "`auth.access_key_id`",
        ),
    ] {
        let message =
            refused_before_sending(&endpoint(table), &["infer", "--model", "opus", "hi"], &[]);
        assert!(message.contains(field), "{table}: {message}");
        assert!(!message.contains("fixture-literal-key"), "{message}");
    }
}

/// Opus on Converse with the container credentials endpoint, Sonnet on the Anthropic body over
/// `InvokeModel` with static keys, both at `origin`.
fn bedrock_routes(origin: &str) -> String {
    format!(
        r#"format = "llm.routes/1"

[endpoints.bedrock-converse]
base_url = "{origin}"
protocol = "bedrock-converse"
bedrock = {{ region = "eu-central-1" }}
auth = {{ kind = "aws-container" }}

[endpoints.bedrock-invoke]
base_url = "{origin}"
protocol = "messages"
bedrock = {{ region = "eu-central-1" }}
auth = {{ kind = "aws-keys", access_key_id = "env:FIXTURE_AWS_KEY_ID", secret_access_key = "env:FIXTURE_AWS_SECRET" }}

[routes.opus]
model = "bedrock-converse/eu.anthropic.claude-opus-5-5"
aliases = ["claude-opus"]

[routes.sonnet]
model = "bedrock-invoke/eu.anthropic.claude-sonnet-5-5"
"#
    )
}

fn signed_for_frankfurt(request: &Received) {
    let authorization = request.header("authorization").unwrap();
    assert!(
        authorization.starts_with(&format!("AWS4-HMAC-SHA256 Credential={ACCESS_KEY_ID}/")),
        "{authorization}"
    );
    assert!(
        authorization.contains("/eu-central-1/bedrock/aws4_request"),
        "{authorization}"
    );
    assert!(request.header("x-amz-date").is_some());
}

#[test]
fn model_list_shows_both_bedrock_routes_with_their_wire_and_region() {
    let (_directory, path) = configured(&bedrock_routes("http://127.0.0.1:9"));
    let output = llm(
        &["--config", &path, "model", "list", "--format", "json"],
        &[],
        None,
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let rows: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(
        rows,
        json!([
            {"id": "opus", "aliases": ["claude-opus"], "protocol": "bedrock-converse",
             "provider": "bedrock-converse", "host": "127.0.0.1", "region": "eu-central-1",
             "model": "eu.anthropic.claude-opus-5-5", "price_per_million": null},
            {"id": "sonnet", "aliases": [], "protocol": "messages",
             "provider": "bedrock-invoke", "host": "127.0.0.1", "region": "eu-central-1",
             "model": "eu.anthropic.claude-sonnet-5-5", "price_per_million": null},
        ])
    );
}

#[test]
fn infer_on_opus_signs_a_converse_request_with_the_container_credentials() {
    let bedrock = Fixture::new(vec![converse_stream()]);
    let container = Fixture::new(vec![container_answer()]);
    let (directory, path) = configured(&bedrock_routes(&bedrock.origin));
    let token_file = directory.path().join("token");
    write(&token_file, CONTAINER_TOKEN);
    // The file source reads only a file its owner alone may read.
    std::fs::set_permissions(&token_file, std::fs::Permissions::from_mode(0o600)).unwrap();
    let uri = format!("{}/v1/credentials", container.origin);
    let output = llm(
        &["--config", &path, "infer", "--model", "claude-opus", "hi"],
        &[
            ("AWS_CONTAINER_CREDENTIALS_FULL_URI", &uri),
            (
                "AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE",
                token_file.to_str().unwrap(),
            ),
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "Opus speaking\n");
    assert_eq!(
        stderr(&output),
        "route=opus protocol=bedrock-converse input_tokens=9 output_tokens=2\n"
    );
    let asked = container.received();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].header("authorization"), Some(CONTAINER_TOKEN));
    let received = bedrock.received();
    assert_eq!(received.len(), 1);
    assert_eq!(
        received[0].line,
        "POST /model/eu.anthropic.claude-opus-5-5/converse-stream HTTP/1.1"
    );
    signed_for_frankfurt(&received[0]);
}

#[test]
fn infer_on_sonnet_signs_an_invoke_model_request_with_the_static_keys() {
    let bedrock = Fixture::new(vec![invoke_stream()]);
    let (_directory, path) = configured(&bedrock_routes(&bedrock.origin));
    let output = llm(
        &["--config", &path, "infer", "--model", "sonnet", "hi"],
        &[
            ("FIXTURE_AWS_KEY_ID", ACCESS_KEY_ID),
            ("FIXTURE_AWS_SECRET", SECRET_ACCESS_KEY),
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "Sonnet here\n");
    let usage = stderr(&output);
    assert!(
        usage.starts_with("route=sonnet protocol=messages input_tokens="),
        "{usage}"
    );
    assert!(usage.trim_end().ends_with("output_tokens=3"), "{usage}");
    let received = bedrock.received();
    assert_eq!(
        received[0].line,
        "POST /model/eu.anthropic.claude-sonnet-5-5/invoke-with-response-stream HTTP/1.1"
    );
    signed_for_frankfurt(&received[0]);
    let message = format!("{received:?}");
    assert!(!message.contains(SECRET_ACCESS_KEY));
}

#[test]
fn a_container_route_without_the_container_endpoint_is_refused_before_any_request() {
    let message = refused_before_sending(
        &bedrock_routes("{origin}"),
        &["infer", "--model", "opus", "hi"],
        &[],
    );
    assert!(
        message.starts_with("llm: route `opus`: unauthorized"),
        "{message}"
    );
}

/// A file per lookup place, each declaring one route named after its place.
fn place(path: &Path, route: &str) {
    write(
        path,
        &format!(
            "format = \"llm.routes/1\"\n[endpoints.lab]\nbase_url = \"http://127.0.0.1:9/v1\"\n\
             protocol = \"chat-completions\"\nauth = {{ kind = \"anonymous\" }}\n\
             [routes.{route}]\nmodel = \"lab/small\"\n"
        ),
    );
}

/// The route ids `llm model list` printed.
fn listed(output: &Output) -> Vec<String> {
    assert_eq!(output.status.code(), Some(0), "{}", stderr(output));
    stdout(output)
        .lines()
        .skip(1)
        .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
        .collect()
}

/// A home with a file in each default place, an `LLM_CONFIG` file and a `--config` file.
fn every_place() -> (tempfile::TempDir, [String; 4]) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let paths = [
        root.join("flag.toml"),
        root.join("variable.toml"),
        root.join("home/.b10x/llm.toml"),
        root.join("xdg/b10x/llm.toml"),
    ];
    for (path, route) in paths.iter().zip(["flag", "variable", "b10x", "xdg"]) {
        place(path, route);
    }
    let strings = paths.map(|path| path.to_str().unwrap().to_owned());
    (directory, strings)
}

#[test]
fn the_config_flag_is_looked_at_first() {
    let (directory, [flag, variable, ..]) = every_place();
    let home = directory.path().join("home");
    let xdg = directory.path().join("xdg");
    let output = llm(
        &["model", "list", "--config", &flag],
        &[
            ("LLM_CONFIG", &variable),
            ("HOME", home.to_str().unwrap()),
            ("XDG_CONFIG_HOME", xdg.to_str().unwrap()),
        ],
        None,
    );
    assert_eq!(listed(&output), ["flag"]);
}

#[test]
fn the_config_variable_is_looked_at_second() {
    let (directory, [_, variable, ..]) = every_place();
    let home = directory.path().join("home");
    let xdg = directory.path().join("xdg");
    let output = llm(
        &["model", "list"],
        &[
            ("LLM_CONFIG", &variable),
            ("HOME", home.to_str().unwrap()),
            ("XDG_CONFIG_HOME", xdg.to_str().unwrap()),
        ],
        None,
    );
    assert_eq!(listed(&output), ["variable"]);
}

#[test]
fn the_b10x_directory_in_the_home_is_looked_at_third() {
    let (directory, _) = every_place();
    let home = directory.path().join("home");
    let xdg = directory.path().join("xdg");
    let output = llm(
        &["model", "list"],
        &[
            ("HOME", home.to_str().unwrap()),
            ("XDG_CONFIG_HOME", xdg.to_str().unwrap()),
        ],
        None,
    );
    assert_eq!(listed(&output), ["b10x"]);
}

#[test]
fn the_xdg_config_home_is_looked_at_fourth_and_defaults_to_dot_config() {
    let (directory, _) = every_place();
    let home = directory.path().join("other-home");
    let xdg = directory.path().join("xdg");
    let output = llm(
        &["model", "list"],
        &[
            ("HOME", home.to_str().unwrap()),
            ("XDG_CONFIG_HOME", xdg.to_str().unwrap()),
        ],
        None,
    );
    assert_eq!(listed(&output), ["xdg"]);
    place(&home.join(".config/b10x/llm.toml"), "dotconfig");
    let output = llm(
        &["model", "list"],
        &[("HOME", home.to_str().unwrap())],
        None,
    );
    assert_eq!(listed(&output), ["dotconfig"]);
}

#[test]
fn no_configuration_file_is_refused_naming_all_four_places() {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().to_str().unwrap();
    let output = llm(&["model", "list"], &[("HOME", home)], None);
    assert_eq!(output.status.code(), Some(2));
    let message = stderr(&output);
    for place in [
        "--config (not set)",
        "LLM_CONFIG (not set)",
        "~/.b10x/llm.toml (",
        "$XDG_CONFIG_HOME/b10x/llm.toml (",
    ] {
        assert!(message.contains(place), "{place}: {message}");
    }
    assert!(message.contains(".config/b10x/llm.toml"), "{message}");
}

#[test]
fn a_named_file_that_does_not_exist_is_refused_without_falling_back() {
    let (directory, _) = every_place();
    let home = directory.path().join("home");
    let missing = directory.path().join("missing.toml");
    let output = llm(
        &["model", "list"],
        &[
            ("LLM_CONFIG", missing.to_str().unwrap()),
            ("HOME", home.to_str().unwrap()),
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(2));
    let message = stderr(&output);
    assert!(
        message.contains("missing.toml (from LLM_CONFIG)"),
        "{message}"
    );
}

#[test]
fn a_yaml_file_is_read_as_the_yaml_document() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("routes.yaml");
    write(
        &path,
        "format: llm.routes/1\nendpoints:\n  lab: {base_url: http://127.0.0.1:9/v1, \
         protocol: chat-completions, auth: {kind: anonymous}}\nroutes:\n  small: lab/small\n",
    );
    let output = llm(
        &["model", "list", "--config", path.to_str().unwrap()],
        &[],
        None,
    );
    assert_eq!(listed(&output), ["small"]);
}
