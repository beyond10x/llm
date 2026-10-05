//! HTTP transport conformance observations against a scripted local server.
//!
//! This module builds the real `b10x-llm-http` `HttpClient`, sends one `post_sse_until` to a
//! loopback listener whose answer the fixture scripts, reads the returned stream to its end and
//! reports what the client returned: every event, the refusal that ended the exchange, its
//! dispatch evidence, and how many connections the server saw. It decides no bound and no
//! refusal, reads no suite and branches on no scenario name. Nothing leaves the host.

use std::{
    fmt::Write,
    net::SocketAddr,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use ess_conformance::target::TargetError;
use llm_core::{Cancel, Error};
use llm_http::{
    CONNECT_TIMEOUT, Framing, HeaderMap, HeaderName, HeaderValue, HttpClient, Limits, SseEvent,
    SseStream,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use crate::target::Observed;

/// Every view name this domain answers through `query_view`.
pub const VIEWS: &[&str] = &[
    "llm.transport.LastExchange",
    "llm.transport.LastJsonExchange",
];

const MAX_PROGRAM_BYTES: usize = 64 * 1024;
const MAX_CHUNKS: usize = 64;
const MAX_EVENTS: usize = 1_024;
const MAX_EXTRA_HEADERS: usize = 256;
/// The largest fixture body this adapter will build: one byte past the published bound.
const MAX_FIXTURE_BODY: usize = 16 * 1024 * 1024 + 1;
/// One byte past the published SSE line bound: the length of the whole unterminated line.
const OVERSIZED_LINE: usize = 1024 * 1024 + 1;
/// The largest request head the scripted server will read.
const MAX_REQUEST_HEAD: usize = 64 * 1024;
/// A marker only the scripted server's error body carries.
const UNTRUSTED_BODY: &str = "llm-fixture-untrusted-error-body";
/// How long the adapter waits for a second connection that would reveal a resend.
const RESEND_GRACE: Duration = Duration::from_millis(50);
/// How long a connection attempt to an unaccepting listener may stall before its queue counts
/// as full.
const QUEUE_FULL: Duration = Duration::from_millis(200);
/// A backstop so a fixture can never hang the gate. Far above every authored bound.
const BACKSTOP: Duration = Duration::from_secs(30);

/// Observe one command of this domain, or `None` when the command belongs to another.
pub fn observe(command: &str, input: &Value) -> Option<Result<Observed, TargetError>> {
    match command {
        "llm.transport.Exchange" => Some(exercise(input)),
        "llm.transport.PostJson" => Some(post_json(input)),
        _ => None,
    }
}

fn unavailable(error: impl std::fmt::Display) -> TargetError {
    TargetError::unavailable("transport observation", error.to_string())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Exchange {
    program_json: String,
}

fn exercise(input: &Value) -> Result<Observed, TargetError> {
    let request: Exchange = serde_json::from_value(input.clone()).map_err(unavailable)?;
    Ok(Observed {
        facts: run(&request.program_json),
        view: "llm.transport.LastExchange",
        event: "llm.transport.Exchanged",
        field: "valid_program",
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Program {
    #[serde(default)]
    limits: Option<LimitsInput>,
    #[serde(default)]
    framing: Option<FramingInput>,
    /// The endpoint, with `{addr}` standing for the scripted server's address.
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    extra_headers: usize,
    #[serde(default)]
    body_bytes: Option<usize>,
    /// A caller instant this many milliseconds after the call starts.
    #[serde(default)]
    until_ms: Option<u64>,
    /// Report whether the exchange ended within this many milliseconds of its start.
    #[serde(default)]
    within_ms: Option<u64>,
    #[serde(default)]
    cancel: CancelInput,
    /// Run the exchange in a child process whose proxy variables all name a local listener.
    #[serde(default)]
    ambient_proxy: bool,
    server: ServerInput,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LimitsInput {
    #[serde(rename = "response_headers_ms")]
    response_headers: u64,
    #[serde(rename = "idle_ms")]
    idle: u64,
    #[serde(rename = "total_ms")]
    total: u64,
    /// The connect bound; the client's default when absent.
    #[serde(rename = "connect_ms", default)]
    connect: Option<u64>,
}

#[derive(Deserialize, Clone, Copy)]
enum FramingInput {
    DoneSentinel,
    PayloadsOnly,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct CancelInput {
    before: bool,
    after_ms: Option<u64>,
    after_events: Option<usize>,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields, default)]
struct ServerInput {
    /// `false` holds a listener whose accept queue is full, so a connection neither completes
    /// nor is refused. Absent means accepting.
    accepting: Option<bool>,
    respond: bool,
    status: u16,
    content_type: Option<String>,
    retry_after: Option<String>,
    redirect: bool,
    chunks: Vec<String>,
    oversized_line: bool,
    keepalive_ms: Option<u64>,
    end: EndInput,
}

impl Default for ServerInput {
    fn default() -> Self {
        Self {
            accepting: None,
            respond: true,
            status: 200,
            content_type: Some("text/event-stream".to_owned()),
            retry_after: None,
            redirect: false,
            chunks: Vec::new(),
            oversized_line: false,
            keepalive_ms: None,
            end: EndInput::Close,
        }
    }
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum EndInput {
    Close,
    Hold,
}

/// What the scripted servers saw.
#[derive(Default)]
struct Seen {
    accepted: AtomicUsize,
    redirected: AtomicUsize,
    request_line: Mutex<Option<String>>,
}

/// Reads one request head and its declared body. Returns the request line.
async fn read_request(socket: &mut TcpStream) -> Option<String> {
    let mut bytes = Vec::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    let end = loop {
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break end;
        }
        if bytes.len() > MAX_REQUEST_HEAD {
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
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().ok())
                .flatten()
        })
        .unwrap_or(0);
    let mut remaining = length.saturating_sub(bytes.len().saturating_sub(end + 4));
    while remaining > 0 {
        let read = socket.read(&mut buffer).await.ok()?;
        if read == 0 {
            break;
        }
        remaining = remaining.saturating_sub(read);
    }
    head.lines().next().map(str::to_owned)
}

async fn answer(
    mut socket: TcpStream,
    script: ServerInput,
    location: Option<String>,
    seen: Arc<Seen>,
) {
    let line = read_request(&mut socket).await;
    {
        let mut recorded = seen
            .request_line
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if recorded.is_none() {
            *recorded = line;
        }
    }
    if !script.respond {
        std::future::pending::<()>().await;
    }
    let mut head = format!("HTTP/1.1 {} Scripted\r\n", script.status);
    if let Some(content_type) = &script.content_type {
        let _ = write!(head, "content-type: {content_type}\r\n");
    }
    if let Some(retry_after) = &script.retry_after {
        let _ = write!(head, "retry-after: {retry_after}\r\n");
    }
    if let Some(location) = &location {
        let _ = write!(head, "location: {location}\r\n");
    }
    if script.status != 200 {
        let _ = write!(
            head,
            "content-length: {}\r\nconnection: close\r\n\r\n{UNTRUSTED_BODY}",
            UNTRUSTED_BODY.len()
        );
        let _written = socket.write_all(head.as_bytes()).await;
        return;
    }
    head.push_str("connection: close\r\n\r\n");
    if socket.write_all(head.as_bytes()).await.is_err() {
        return;
    }
    for chunk in &script.chunks {
        if socket.write_all(chunk.as_bytes()).await.is_err() || socket.flush().await.is_err() {
            return;
        }
        tokio::task::yield_now().await;
    }
    if script.oversized_line {
        let mut line = b"data: ".to_vec();
        line.resize(OVERSIZED_LINE, b'x');
        if socket.write_all(&line).await.is_err() {
            return;
        }
    }
    if let Some(interval) = script.keepalive_ms {
        loop {
            if socket.write_all(b":keepalive\n\n").await.is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(interval.max(1))).await;
        }
    }
    if script.end == EndInput::Hold {
        std::future::pending::<()>().await;
    }
}

async fn listen(
    listener: TcpListener,
    script: ServerInput,
    location: Option<String>,
    seen: Arc<Seen>,
) {
    while let Ok((socket, _)) = listener.accept().await {
        seen.accepted.fetch_add(1, Ordering::SeqCst);
        tokio::spawn(answer(
            socket,
            script.clone(),
            location.clone(),
            Arc::clone(&seen),
        ));
    }
}

async fn count(listener: TcpListener, seen: Arc<Seen>) {
    while let Ok((_socket, _)) = listener.accept().await {
        seen.redirected.fetch_add(1, Ordering::SeqCst);
    }
}

fn label(value: impl serde::Serialize) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn refusal(facts: &mut Value, error: &Error) {
    facts["error_code"] = label(error.code);
    facts["dispatch"] = label(error.dispatch);
    facts["retry_after_ms"] = json!(error.retry_after_ms);
    facts["retriable"] = json!(error.retriable);
    if format!("{error:?}{error}").contains(UNTRUSTED_BODY) {
        facts["diagnostics_safe"] = json!(false);
    }
}

fn event_fact(event: &SseEvent) -> String {
    match event {
        SseEvent::Done => "done".to_owned(),
        SseEvent::Payload { event, data } => {
            format!("payload:{}:{data}", event.as_deref().unwrap_or("-"))
        }
    }
}

fn headers(count: usize) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for index in 0..count.min(MAX_EXTRA_HEADERS) {
        if let Ok(name) = HeaderName::from_bytes(format!("x-fixture-{index}").as_bytes()) {
            headers.insert(name, HeaderValue::from_static("fixture"));
        }
    }
    headers
}

/// Binds a listener that never accepts and fills its accept queue, so the kernel drops every
/// further connection attempt. The listener and the queued connections live until the runtime
/// that observes the exchange is dropped.
async fn start_unaccepting() -> Result<SocketAddr, String> {
    let bind_error = |_| "fixture:bind".to_owned();
    let socket = tokio::net::TcpSocket::new_v4().map_err(bind_error)?;
    socket
        .bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .map_err(bind_error)?;
    let listener = socket.listen(0).map_err(bind_error)?;
    let addr = listener.local_addr().map_err(bind_error)?;
    let mut queued = Vec::new();
    loop {
        if queued.len() >= MAX_CHUNKS {
            return Err("fixture:accept-queue".to_owned());
        }
        match tokio::time::timeout(QUEUE_FULL, TcpStream::connect(addr)).await {
            Ok(Ok(stream)) => queued.push(stream),
            Ok(Err(_)) => return Err("fixture:accept-queue".to_owned()),
            Err(_) => break,
        }
    }
    tokio::spawn(async move {
        let _held = (listener, queued);
        std::future::pending::<()>().await;
    });
    Ok(addr)
}

/// Binds the scripted server, and the redirect target it points at when it redirects.
async fn start_server(script: &ServerInput, seen: &Arc<Seen>) -> Result<SocketAddr, String> {
    if script.accepting == Some(false) {
        return start_unaccepting().await;
    }
    let bind_error = |_| "fixture:bind".to_owned();
    let listener = TcpListener::bind("127.0.0.1:0").await.map_err(bind_error)?;
    let addr = listener.local_addr().map_err(bind_error)?;
    let location = if script.redirect {
        let second = TcpListener::bind("127.0.0.1:0").await.map_err(bind_error)?;
        let target = second.local_addr().map_err(bind_error)?;
        tokio::spawn(count(second, Arc::clone(seen)));
        Some(format!("http://{target}/elsewhere"))
    } else {
        None
    };
    tokio::spawn(listen(listener, script.clone(), location, Arc::clone(seen)));
    Ok(addr)
}

/// Reads the stream to its end or its first refusal, then once more to see the refusal repeat.
async fn read_stream(
    stream: &mut SseStream,
    cancel: &Cancel,
    cancel_after_events: Option<usize>,
    facts: &mut Value,
) -> Result<(), String> {
    let mut events = Vec::new();
    let outcome = loop {
        if events.len() >= MAX_EVENTS {
            break Err("fixture:too-many-events".to_owned());
        }
        match stream.next().await {
            Ok(Some(event)) => {
                events.push(event_fact(&event));
                if cancel_after_events == Some(events.len()) {
                    cancel.cancel();
                }
            }
            Ok(None) => {
                facts["end"] = json!("end");
                break Ok(());
            }
            Err(error) => {
                refusal(facts, &error);
                facts["end"] = json!("error");
                facts["sticky"] = json!(stream.next().await.err() == Some(error));
                break Ok(());
            }
        }
    };
    facts["events"] = json!(events);
    outcome
}

async fn exchange(program: Program, facts: &mut Value, seen: &Arc<Seen>) -> Result<(), String> {
    let connect = program
        .limits
        .as_ref()
        .and_then(|limits| limits.connect)
        .map_or(CONNECT_TIMEOUT, Duration::from_millis);
    let limits = program.limits.map_or_else(
        || Limits {
            response_headers: Duration::from_secs(5),
            idle: Duration::from_secs(5),
            total: Duration::from_secs(10),
        },
        |limits| Limits {
            response_headers: Duration::from_millis(limits.response_headers),
            idle: Duration::from_millis(limits.idle),
            total: Duration::from_millis(limits.total),
        },
    );
    let addr = start_server(&program.server, seen).await?;
    let client = match HttpClient::with_connect_timeout(limits, connect) {
        Ok(client) => client,
        Err(error) => {
            refusal(facts, &error);
            return Ok(());
        }
    };
    let url = program
        .url
        .as_deref()
        .unwrap_or("http://{addr}/arbitrary/prefix")
        .replace("{addr}", &addr.to_string());
    let body = program.body_bytes.map_or_else(
        || b"{}".to_vec(),
        |bytes| vec![b'x'; bytes.min(MAX_FIXTURE_BODY)],
    );
    let framing = match program.framing.unwrap_or(FramingInput::PayloadsOnly) {
        FramingInput::DoneSentinel => Framing::DoneSentinel,
        FramingInput::PayloadsOnly => Framing::PayloadsOnly,
    };
    let cancel = Cancel::new();
    if program.cancel.before {
        cancel.cancel();
    }
    if let Some(ms) = program.cancel.after_ms {
        let later = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(ms)).await;
            later.cancel();
        });
    }
    let until = program
        .until_ms
        .map(|ms| std::time::Instant::now() + Duration::from_millis(ms));
    let sent = client
        .post_sse_until(
            &url,
            headers(program.extra_headers),
            body,
            framing,
            &cancel,
            until,
        )
        .await;
    let mut stream = match sent {
        Ok(stream) => stream,
        Err(error) => {
            refusal(facts, &error);
            return Ok(());
        }
    };
    read_stream(&mut stream, &cancel, program.cancel.after_events, facts).await
}

fn blank() -> Value {
    json!({
        "valid_program": false, "error_code": null, "dispatch": null, "retry_after_ms": null,
        "retriable": null, "events": [], "end": null, "sticky": null, "requests": 0, "redirected_requests": 0,
        "request_line": null, "diagnostics_safe": true, "ended_within": null,
        "proxied_requests": null
    })
}

/// Observe one program, in a child process when it asks for ambient proxy variables.
pub fn run(program_json: &str) -> Value {
    let ambient = program_json.len() <= MAX_PROGRAM_BYTES
        && serde_json::from_str::<Program>(program_json).is_ok_and(|program| program.ambient_proxy);
    if ambient {
        return in_child(program_json);
    }
    run_here(program_json)
}

/// Observe one program in this process, whatever its `ambient_proxy` says.
pub fn run_here(program_json: &str) -> Value {
    let mut facts = blank();
    if program_json.len() > MAX_PROGRAM_BYTES {
        return facts;
    }
    let Ok(program) = serde_json::from_str::<Program>(program_json) else {
        return facts;
    };
    if program.server.chunks.len() > MAX_CHUNKS {
        return facts;
    }
    facts["valid_program"] = json!(true);
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        facts["error_code"] = json!("fixture:no-runtime");
        return facts;
    };
    let seen = Arc::new(Seen::default());
    let within = program.within_ms.map(Duration::from_millis);
    let started = std::time::Instant::now();
    let outcome = runtime.block_on(async {
        let outcome = tokio::time::timeout(BACKSTOP, exchange(program, &mut facts, &seen)).await;
        facts["ended_within"] = json!(within.map(|within| started.elapsed() <= within));
        tokio::time::sleep(RESEND_GRACE).await;
        outcome
    });
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(code)) => facts["error_code"] = json!(code),
        Err(_) => facts["error_code"] = json!("fixture:backstop"),
    }
    facts["requests"] = json!(seen.accepted.load(Ordering::SeqCst));
    facts["redirected_requests"] = json!(seen.redirected.load(Ordering::SeqCst));
    facts["request_line"] = json!(
        seen.request_line
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    );
    drop(runtime);
    facts
}

/// Every variable name an HTTP stack reads as an ambient proxy.
pub(crate) const PROXY_VARIABLES: &[&str] = &[
    "HTTP_PROXY",
    "http_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "ALL_PROXY",
    "all_proxy",
];

/// A local listener standing in for an ambient proxy. It answers nothing and counts every
/// connection that opens with a request (`POST` or `CONNECT`); a connection from some other
/// local process that happens to reach the port is not counted.
fn start_proxy() -> std::io::Result<(String, Arc<AtomicBool>, std::thread::JoinHandle<usize>)> {
    use std::io::{ErrorKind, Read};
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let url = format!("http://{}", listener.local_addr()?);
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = Arc::clone(&stop);
    let thread = std::thread::spawn(move || {
        let mut proxied = 0;
        loop {
            match listener.accept() {
                Ok((mut socket, _)) => {
                    let _blocking = socket.set_nonblocking(false);
                    let _timeout = socket.set_read_timeout(Some(Duration::from_secs(1)));
                    let mut bytes = vec![0_u8; 8192];
                    let read = socket.read(&mut bytes).unwrap_or(0);
                    let head = bytes.get(..read).unwrap_or_default();
                    if head.starts_with(b"POST ") || head.starts_with(b"CONNECT ") {
                        proxied += 1;
                    }
                }
                // Stop only once the backlog is empty, so a connection made before the child
                // exited is always counted.
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    if stopped.load(Ordering::SeqCst) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
        proxied
    });
    Ok((url, stop, thread))
}

/// Re-executes this binary on the program with every proxy variable naming a local listener
/// and no proxy exemption, so the process-wide environment of every other lane is untouched.
fn in_child(program_json: &str) -> Value {
    let mut facts = blank();
    facts["valid_program"] = json!(true);
    let Ok((url, stop, proxy)) = start_proxy() else {
        facts["error_code"] = json!("fixture:bind");
        return facts;
    };
    let output = std::env::current_exe().and_then(|binary| {
        let mut command = std::process::Command::new(binary);
        command
            .args(["transport-child", program_json])
            .env_remove("NO_PROXY")
            .env_remove("no_proxy")
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        for variable in PROXY_VARIABLES {
            command.env(variable, &url);
        }
        command.output()
    });
    stop.store(true, Ordering::SeqCst);
    let proxied = proxy.join().ok();
    match output
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| serde_json::from_slice::<Value>(&output.stdout).ok())
    {
        Some(child) => facts = child,
        None => facts["error_code"] = json!("fixture:child"),
    }
    facts["proxied_requests"] = json!(proxied);
    facts
}

// One JSON document exchange (`HttpClient::post_json`) against a scripted local server.

/// A marker only the request body of a JSON exchange carries.
const JSON_REQUEST_MARKER: &str = "llm-fixture-json-request-private-marker";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonProgram {
    server: JsonServer,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, default)]
struct JsonServer {
    listening: bool,
    status: u16,
    content_type: Option<String>,
    body: Option<String>,
    body_bytes: Option<usize>,
    redirect: bool,
}

impl Default for JsonServer {
    fn default() -> Self {
        Self {
            listening: true,
            status: 200,
            content_type: Some("application/json".to_owned()),
            body: None,
            body_bytes: None,
            redirect: false,
        }
    }
}

impl JsonServer {
    /// The answer body: a failing status carries the untrusted marker; a success carries `body`
    /// or a JSON string literal of exactly `body_bytes` bytes.
    fn answer(&self) -> Vec<u8> {
        if !(200..300).contains(&self.status) {
            return format!(r#"{{"error":"{UNTRUSTED_BODY}"}}"#).into_bytes();
        }
        match (self.body_bytes, &self.body) {
            (Some(total), _) if total >= 2 => format!("\"{}\"", "x".repeat(total - 2)).into_bytes(),
            (Some(total), _) => vec![b'x'; total],
            (None, Some(body)) => body.clone().into_bytes(),
            (None, None) => b"{}".to_vec(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PostJsonInput {
    program_json: String,
}

fn post_json(input: &Value) -> Result<Observed, TargetError> {
    let request: PostJsonInput = serde_json::from_value(input.clone()).map_err(unavailable)?;
    Ok(Observed {
        facts: run_json(&request.program_json),
        view: "llm.transport.LastJsonExchange",
        event: "llm.transport.JsonPosted",
        field: "valid_program",
    })
}

fn json_blank() -> Value {
    json!({
        "valid_program": false, "error_code": null, "dispatch": null, "retriable": null,
        "requests": 0, "redirected_requests": 0, "request_content_type": null,
        "answer_matches": null, "diagnostics_safe": true
    })
}

/// Reads one request head and its declared body; returns the `content-type` it named.
async fn read_json_request(socket: &mut TcpStream) -> Option<Option<String>> {
    let mut bytes = Vec::new();
    let mut buffer = vec![0_u8; 16 * 1024];
    let end = loop {
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break end;
        }
        if bytes.len() > MAX_REQUEST_HEAD {
            return None;
        }
        let read = socket.read(&mut buffer).await.ok()?;
        if read == 0 {
            return None;
        }
        bytes.extend_from_slice(buffer.get(..read)?);
    };
    let head = String::from_utf8_lossy(bytes.get(..end)?).into_owned();
    let header = |wanted: &str| {
        head.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case(wanted)
                .then(|| value.trim().to_owned())
        })
    };
    let length: usize = header("content-length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let mut read_body = bytes.len().saturating_sub(end + 4);
    while read_body < length {
        let read = socket.read(&mut buffer).await.ok()?;
        if read == 0 {
            return None;
        }
        read_body += read;
    }
    Some(header("content-type"))
}

async fn json_exchange(
    server: JsonServer,
    facts: &mut Value,
    seen: &Arc<Seen>,
) -> Result<(), String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| "fixture:bind")?;
    let address = listener.local_addr().map_err(|_| "fixture:bind")?;
    let target = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| "fixture:bind")?;
    let location = format!(
        "http://{}/elsewhere",
        target.local_addr().map_err(|_| "fixture:bind")?
    );
    tokio::spawn(count(target, seen.clone()));
    let content_type: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    if server.listening {
        let (seen, content_type) = (seen.clone(), content_type.clone());
        let answer = server.answer();
        let (status, media, redirect) =
            (server.status, server.content_type.clone(), server.redirect);
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                seen.accepted.fetch_add(1, Ordering::SeqCst);
                let Some(received) = read_json_request(&mut socket).await else {
                    continue;
                };
                *content_type.lock().unwrap_or_else(PoisonError::into_inner) = received;
                let mut head = format!("HTTP/1.1 {status} Fixture\r\n");
                if redirect {
                    let _ = write!(head, "location: {location}\r\n");
                }
                if let Some(media) = &media {
                    let _ = write!(head, "content-type: {media}\r\n");
                }
                let _ = write!(
                    head,
                    "content-length: {}\r\nconnection: close\r\n\r\n",
                    answer.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(&answer).await;
                let _ = socket.shutdown().await;
            }
        });
    } else {
        drop(listener);
    }
    let client = HttpClient::new(Limits {
        response_headers: Duration::from_secs(30),
        idle: Duration::from_secs(30),
        total: Duration::from_secs(30),
    })
    .map_err(|_| "fixture:client")?;
    let body =
        serde_json::to_vec(&json!({"secret": JSON_REQUEST_MARKER})).map_err(|_| "fixture:body")?;
    let outcome = client
        .post_json(
            &format!("http://{address}/oauth/token"),
            HeaderMap::new(),
            body,
            &Cancel::new(),
        )
        .await;
    match outcome {
        Ok(document) => {
            let sent: Option<Value> = serde_json::from_slice(&server.answer()).ok();
            facts["answer_matches"] = json!(sent.as_ref() == Some(&document));
        }
        Err(error) => {
            facts["error_code"] = label(error.code);
            facts["dispatch"] = label(error.dispatch);
            facts["retriable"] = json!(error.retriable);
            let rendered = format!("{error:?}{error}");
            if rendered.contains(UNTRUSTED_BODY) || rendered.contains(JSON_REQUEST_MARKER) {
                facts["diagnostics_safe"] = json!(false);
            }
        }
    }
    facts["request_content_type"] = json!(
        content_type
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    );
    Ok(())
}

/// Observe one JSON exchange program.
fn run_json(program_json: &str) -> Value {
    let mut facts = json_blank();
    if program_json.len() > MAX_PROGRAM_BYTES {
        return facts;
    }
    let Ok(program) = serde_json::from_str::<JsonProgram>(program_json) else {
        return facts;
    };
    if program
        .server
        .body_bytes
        .is_some_and(|bytes| bytes > MAX_FIXTURE_BODY)
    {
        return facts;
    }
    facts["valid_program"] = json!(true);
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        facts["error_code"] = json!("fixture:no-runtime");
        return facts;
    };
    let seen = Arc::new(Seen::default());
    let outcome = runtime.block_on(async {
        let outcome =
            tokio::time::timeout(BACKSTOP, json_exchange(program.server, &mut facts, &seen)).await;
        tokio::time::sleep(RESEND_GRACE).await;
        outcome
    });
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(code)) => facts["error_code"] = json!(code),
        Err(_) => facts["error_code"] = json!("fixture:backstop"),
    }
    facts["requests"] = json!(seen.accepted.load(Ordering::SeqCst));
    facts["redirected_requests"] = json!(seen.redirected.load(Ordering::SeqCst));
    drop(runtime);
    facts
}
