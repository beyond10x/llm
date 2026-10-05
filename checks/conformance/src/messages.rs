//! Observe the production Messages codec, response decoder and event-stream decoder.
//!
//! Every fact below is what a public `llm_messages` function returned. Nothing here decides an
//! answer: no suite is read, no scenario is named, and no projection is reimplemented.

use ess_conformance::target::TargetError;
use llm_core::{
    Cancel, Error, ErrorCode, Id, Item, StopReason, StreamEvent, TurnDocument, TurnOutcome,
    TurnRequest, VecSink,
};
use llm_messages::{decode_message, decode_request, decode_stream, encode_request};
use llm_routing::Catalog;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::target::Observed;

/// Every view name this domain answers through `query_view`.
pub const VIEWS: &[&str] = &[
    "llm.messages.LastInspection",
    "llm.messages.LastPresentation",
];

const MAX_SINK_EVENTS: usize = 4096;
const MAX_SINK_BYTES: usize = 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectInput {
    request_json: String,
    ingress: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StreamInput {
    request_json: String,
    events_sse: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MessageInput {
    request_json: String,
    message_json: String,
}

/// Observe one command of this domain, or `None` when the command belongs to another.
pub fn observe(command: &str, input: &Value) -> Option<Result<Observed, TargetError>> {
    if command == "llm.messages.Present" {
        return Some(present(input).map(|facts| Observed {
            facts,
            view: "llm.messages.LastPresentation",
            event: "llm.messages.Presented",
            field: "accepted",
        }));
    }
    let facts = match command {
        "llm.messages.Project" => {
            inspect(input, |input: &ProjectInput, facts| project(input, facts))
        }
        "llm.messages.DecodeStream" => {
            inspect(input, |input: &StreamInput, facts| stream(input, facts))
        }
        "llm.messages.DecodeMessage" => {
            inspect(input, |input: &MessageInput, facts| message(input, facts))
        }
        _ => return None,
    };
    Some(facts.map(|facts| Observed {
        facts,
        view: "llm.messages.LastInspection",
        event: "llm.messages.Inspected",
        field: "accepted",
    }))
}

/// The observation shape. Absent evidence stays absent rather than being filled with a default.
fn blank() -> Value {
    json!({"accepted":false,"error_code":null,"error_message":null,
        "dispatch":null,"wire_request":null,
        "neutral_request":null,"stream":null,"items":[],"text":"","reasoning":"",
        "tool_arguments":[],"tool_stream":[],"upstream_model":null,"response_id":null,"final_usage":null,
        "usage_json":null,"stop_reason":null})
}

fn inspect<T: for<'de> Deserialize<'de>>(
    input: &Value,
    run: impl Fn(&T, &mut Value) -> Result<(), Error>,
) -> Result<Value, TargetError> {
    let input: T = serde_json::from_value(input.clone())
        .map_err(|error| TargetError::unavailable("messages observation", error.to_string()))?;
    let mut facts = blank();
    match run(&input, &mut facts) {
        Ok(()) => facts["accepted"] = json!(true),
        Err(error) => report_failure(&error, &mut facts),
    }
    Ok(facts)
}

/// The fixture deployment, declared exactly as an operator declares one.
///
/// Built through the real catalog rather than by hand, so the binding under observation is the
/// one an operator's configuration produces, revision included.
const FIXTURE_CATALOG: &str = r#"
format = "llm.catalog/1"

[[providers]]
id = "lab"
category = "hosted"

[[accounts]]
id = "account"
provider_id = "lab"
auth_kind = "api-key"
billing_kind = "metered"
secret_reference_id = "messages-key"
api_key_header = "x-api-key"

[[endpoints]]
id = "endpoint"
account_id = "account"
base_url = "https://messages.example.invalid/v1"

[[models]]
id = "internal-model"
upstream_name = "example/Model-Revision"

[[serving_models]]
id = "serving"
endpoint_id = "endpoint"
model_id = "internal-model"
protocol = "messages"
[serving_models.capabilities]
tools = true
tool_choice = true
temperature = true
top_p = true
reasoning_efforts = ["medium", "high"]
context_window = 32768
max_output_tokens = 2048

[[routes]]
id = "messages"
alias = "messages-route"

[[targets]]
id = "messages-primary"
route_id = "messages"
serving_model_id = "serving"
position = 0
"#;

fn fixture() -> Result<Catalog, Error> {
    Catalog::parse(FIXTURE_CATALOG)
}

fn serving() -> Result<Id, Error> {
    Id::new("serving").map_err(|_| Error::invalid("fixture identifier"))
}

fn turn(request_json: &str) -> Result<TurnRequest, Error> {
    let document: TurnDocument =
        serde_json::from_str(request_json).map_err(|_| Error::invalid("invalid turn envelope"))?;
    Ok(document.request)
}

fn document(request: &TurnRequest) -> Result<String, Error> {
    serde_json::to_string(&TurnDocument::new(request.clone()))
        .map_err(|_| Error::invalid("turn envelope cannot be encoded"))
}

fn project(input: &ProjectInput, facts: &mut Value) -> Result<(), Error> {
    let catalog = fixture()?;
    let binding = catalog
        .binding(&serving()?)
        .ok_or_else(|| Error::invalid("fixture binding is missing"))?;
    if input.ingress {
        let ingress = decode_request(input.request_json.as_bytes(), binding.provenance())?;
        facts["stream"] = json!(ingress.stream);
        facts["neutral_request"] = json!(document(&ingress.request)?);
        facts["items"] = json!(labels(&ingress.request.items));
    } else {
        let request = turn(&input.request_json)?;
        let wire: Value = serde_json::from_slice(&encode_request(&request, binding)?)
            .map_err(|_| Error::protocol("the projection is not JSON"))?;
        facts["wire_request"] = json!(
            serde_json::to_string(&wire)
                .map_err(|_| Error::protocol("the projection cannot be encoded"))?
        );
        facts["items"] = json!(labels(&request.items));
    }
    Ok(())
}

fn stream(input: &StreamInput, facts: &mut Value) -> Result<(), Error> {
    let catalog = fixture()?;
    let binding = catalog
        .binding(&serving()?)
        .ok_or_else(|| Error::invalid("fixture binding is missing"))?;
    let request = turn(&input.request_json)?;
    let mut sink = VecSink::new(MAX_SINK_EVENTS, MAX_SINK_BYTES);
    let outcome = tokio::runtime::Builder::new_current_thread()
        .build()
        .map_err(|_| Error::new(ErrorCode::Unavailable, "no local runtime"))?
        .block_on(decode_stream(
            input.events_sse.as_bytes(),
            &request,
            binding.provenance(),
            &mut sink,
            &Cancel::new(),
        ));
    // The prefix a caller already saw is reported whether or not the turn then failed.
    report_sink(&sink, facts);
    report_outcome(&outcome?, facts);
    Ok(())
}

fn message(input: &MessageInput, facts: &mut Value) -> Result<(), Error> {
    let catalog = fixture()?;
    let binding = catalog
        .binding(&serving()?)
        .ok_or_else(|| Error::invalid("fixture binding is missing"))?;
    let request = turn(&input.request_json)?;
    let message: Value = serde_json::from_str(&input.message_json)
        .map_err(|_| Error::invalid("the Messages response is not JSON"))?;
    report_outcome(
        &decode_message(&message, &request, binding.provenance())?,
        facts,
    );
    Ok(())
}

fn report_sink(sink: &VecSink, facts: &mut Value) {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut arguments = Vec::new();
    let mut tool_stream = Vec::new();
    for event in sink.events() {
        match event {
            StreamEvent::TextDelta { text: delta } => text.push_str(delta),
            StreamEvent::ReasoningDelta { text: delta } => reasoning.push_str(delta),
            StreamEvent::ToolCallStarted { call_id, name } => {
                tool_stream.push(format!("started:{call_id}:{name}"));
            }
            StreamEvent::ToolArgumentsDelta { call_id, delta } => {
                arguments.push(format!("{call_id}:{delta}"));
                tool_stream.push(format!("arguments:{call_id}:{delta}"));
            }
            StreamEvent::Warning { code, message } => arguments.push(format!("{code}:{message}")),
        }
    }
    facts["text"] = json!(text);
    facts["reasoning"] = json!(reasoning);
    facts["tool_arguments"] = json!(arguments);
    facts["tool_stream"] = json!(tool_stream);
}

fn report_outcome(outcome: &TurnOutcome, facts: &mut Value) {
    facts["items"] = json!(labels(&outcome.items));
    facts["stop_reason"] = json!(stop_label(&outcome.stop_reason));
    facts["upstream_model"] = json!(outcome.observation.upstream_model);
    facts["response_id"] = json!(outcome.observation.response_id);
    facts["final_usage"] = json!(outcome.observation.final_usage);
    facts["usage_json"] = json!(
        outcome
            .observation
            .usage
            .as_ref()
            .and_then(|usage| serde_json::to_string(usage).ok())
    );
}

/// A refusal is an observation too: its code, its dispatch and whatever it retained.
fn report_failure(error: &Error, facts: &mut Value) {
    facts["error_code"] = json!(error.code);
    facts["error_message"] = json!(error.message);
    facts["dispatch"] = json!(error.dispatch);
    let Some(observation) = &error.observation else {
        return;
    };
    facts["upstream_model"] = json!(observation.upstream_model);
    facts["response_id"] = json!(observation.response_id);
    facts["final_usage"] = json!(observation.final_usage);
    facts["usage_json"] = json!(
        observation
            .usage
            .as_ref()
            .and_then(|usage| serde_json::to_string(usage).ok())
    );
}

fn labels(items: &[Item]) -> Vec<String> {
    items.iter().map(label).collect()
}

fn label(item: &Item) -> String {
    match item {
        Item::UserText { text } => format!("user-text:{text}"),
        Item::AssistantText { text } => format!("assistant-text:{text}"),
        Item::ToolCall(call) => format!(
            "tool-call:{}:{}:{}",
            call.name, call.call_id, call.arguments
        ),
        Item::ToolResult {
            call_id,
            output,
            failed,
        } => format!("tool-result:{call_id}:{failed}:{output}"),
        // The binding is the whole point of an opaque item: a projection that rebound it to
        // the wrong one would be invisible in a label that showed only the payload.
        Item::Opaque {
            provenance,
            payload,
        } => format!(
            "opaque:{}:{payload}",
            serde_json::to_string(provenance).unwrap_or_default()
        ),
        // No binding to show, which is the point: a label that invented one would hide a
        // projection that stamped the reader onto state it could not attribute.
        Item::UnattributedOpaque { protocol, payload } => format!(
            "unattributed-opaque:{}:{payload}",
            serde_json::to_string(protocol).unwrap_or_default()
        ),
    }
}

fn stop_label(stop: &StopReason) -> String {
    match stop {
        StopReason::EndTurn => "end-turn".to_owned(),
        StopReason::ToolCalls => "tool-calls".to_owned(),
        StopReason::MaxOutputTokens => "max-output-tokens".to_owned(),
        StopReason::Incomplete { reason } => format!("incomplete:{reason}"),
    }
}

// ---------------------------------------------------------------------------------------------
// llm.messages.Present: what the production client puts on the wire under one account
// presentation, observed by a scripted server on the loopback interface. The resolver answers a
// new fixture credential on every resolution; no provider, credential store or file is involved.
// ---------------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PresentInput {
    request_json: String,
    auth_kind: String,
    billing_kind: String,
    #[serde(deserialize_with = "crate::target::token_bound")]
    turns: Option<u64>,
}

const MAX_TURNS: u64 = 4;
const MAX_CAPTURED_REQUEST_BYTES: usize = 256 * 1024;
/// The prefix of every credential the resolver answers; the answer's ordinal follows it.
const CREDENTIAL: &str = "llm-fixture-presented-credential-";
const PRESENT_REFERENCE: &str = "messages-credential";

const PRESENT_STREAM: &[u8] = b"event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_present\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"example-model\",\"content\":[],\"usage\":{\"input_tokens\":3,\"output_tokens\":1}}}\n\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"input_tokens\":3,\"output_tokens\":2}}\n\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\n";

fn present_unavailable(error: impl std::fmt::Display) -> TargetError {
    TargetError::unavailable("messages presentation", error.to_string())
}

/// Answers `CREDENTIAL<n>` on its n-th resolution and records each reference it was asked for.
#[derive(Default)]
struct Rotating {
    asked: std::sync::Mutex<Vec<String>>,
    answers: std::sync::atomic::AtomicUsize,
}

impl llm_credentials::SecretResolver for Rotating {
    fn resolve<'a>(
        &'a self,
        reference: &'a llm_credentials::SecretRef,
    ) -> llm_core::BoxFuture<
        'a,
        Result<llm_credentials::ResolvedSecret, llm_credentials::SecretError>,
    > {
        if let Ok(mut asked) = self.asked.lock() {
            asked.push(reference.as_str().to_owned());
        }
        let n = self
            .answers
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        Box::pin(async move {
            Ok(llm_credentials::ResolvedSecret {
                secret: llm_credentials::Secret::new(format!("{CREDENTIAL}{n}").into_bytes())?,
                version: llm_credentials::SecretVersion::new(format!("generation-{n}"))?,
            })
        })
    }
}

/// The fixture deployment with the account's presentation chosen by the scenario.
fn presentation_catalog(auth_kind: &str, billing_kind: &str, base_url: &str) -> String {
    let header = if auth_kind == "api-key" {
        "api_key_header = \"x-api-key\"\n"
    } else {
        ""
    };
    FIXTURE_CATALOG
        .replace(
            "auth_kind = \"api-key\"\nbilling_kind = \"metered\"\nsecret_reference_id = \"messages-key\"\napi_key_header = \"x-api-key\"\n",
            &format!(
                "auth_kind = {}\nbilling_kind = {}\nsecret_reference_id = \"{PRESENT_REFERENCE}\"\n{header}",
                quoted(auth_kind),
                quoted(billing_kind),
            ),
        )
        .replace("https://messages.example.invalid/v1", base_url)
}

/// A TOML basic string holding exactly `value`: a JSON string literal is one, so whatever the
/// scenario names stays one value and the catalog, not this adapter, decides whether it is a kind.
fn quoted(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}

async fn read_captured(socket: &mut tokio::net::TcpStream) -> std::io::Result<String> {
    use tokio::io::AsyncReadExt;
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let length = socket.read(&mut buffer).await?;
        if length == 0 || bytes.len() + length > MAX_CAPTURED_REQUEST_BYTES {
            return Err(std::io::Error::other("unterminated or oversized request"));
        }
        bytes.extend_from_slice(&buffer[..length]);
        let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
        let declared = head
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        if bytes.len() >= end + 4 + declared {
            return String::from_utf8(bytes).map_err(std::io::Error::other);
        }
    }
}

/// Serves each connection one scripted stream and keeps the request it carried.
async fn serve(
    listener: tokio::net::TcpListener,
    captured: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
) {
    use tokio::io::AsyncWriteExt;
    while let Ok((mut socket, _)) = listener.accept().await {
        let Ok(request) = read_captured(&mut socket).await else {
            continue;
        };
        if let Ok(mut captured) = captured.lock() {
            captured.push(request);
        }
        let _written = socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await;
        let _written = socket.write_all(PRESENT_STREAM).await;
        let _closed = socket.shutdown().await;
    }
}

/// The ordinal of the resolver answer a request head carries, or 0 for none.
fn presented(head: &str) -> u64 {
    head.find(CREDENTIAL).map_or(0, |at| {
        head[at + CREDENTIAL.len()..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .unwrap_or(0)
    })
}

/// `<name>: <value>` per header, sorted, without the transport's own framing; the fixture
/// credential reads `<credential>`.
fn header_lines(head: &str) -> Vec<String> {
    let mut lines: Vec<String> = head
        .split("\r\n")
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .filter(|(name, _)| name != "host" && name != "content-length")
        .map(|(name, value)| {
            let value = match value.find(CREDENTIAL) {
                Some(at) => {
                    let tail = &value[at + CREDENTIAL.len()..];
                    let digits = tail.chars().take_while(char::is_ascii_digit).count();
                    format!("{}<credential>{}", &value[..at], &tail[digits..])
                }
                None => value,
            };
            format!("{name}: {value}")
        })
        .collect();
    lines.sort();
    lines
}

fn present(input: &Value) -> Result<Value, TargetError> {
    let input: PresentInput = serde_json::from_value(input.clone()).map_err(present_unavailable)?;
    let turns = input.turns.unwrap_or(0);
    if turns == 0 || turns > MAX_TURNS {
        return Err(present_unavailable("turns must be between one and four"));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(present_unavailable)?;
    runtime.block_on(present_on_loopback(&input))
}

async fn present_on_loopback(input: &PresentInput) -> Result<Value, TargetError> {
    let mut facts = json!({"accepted":false,"error_code":null,"error_message":null,
        "resolved_references":[],"requests":0,"request_headers":[],"wire_request":null,
        "presented_credentials":[]});
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(present_unavailable)?;
    let url = format!(
        "http://{}/v1",
        listener.local_addr().map_err(present_unavailable)?
    );
    let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let server = tokio::spawn(serve(listener, captured.clone()));
    let resolver = std::sync::Arc::new(Rotating::default());
    let outcome = run_turns(input, &url, resolver.clone()).await;
    server.abort();
    match outcome {
        Ok(()) => facts["accepted"] = json!(true),
        Err(error) => {
            facts["error_code"] = json!(error.code);
            facts["error_message"] = json!(error.message);
        }
    }
    facts["resolved_references"] = json!(
        resolver
            .asked
            .lock()
            .map(|asked| asked.clone())
            .unwrap_or_default()
    );
    let captured = captured
        .lock()
        .map(|captured| captured.clone())
        .unwrap_or_default();
    facts["requests"] = json!(captured.len());
    facts["presented_credentials"] = json!(
        captured
            .iter()
            .map(|request| presented(request.split("\r\n\r\n").next().unwrap_or_default()))
            .collect::<Vec<_>>()
    );
    if let Some((head, body)) = captured
        .first()
        .and_then(|first| first.split_once("\r\n\r\n"))
    {
        facts["request_headers"] = json!(header_lines(head));
        facts["wire_request"] = json!(
            serde_json::from_str::<Value>(body)
                .ok()
                .and_then(|body| serde_json::to_string(&body).ok())
        );
    }
    Ok(facts)
}

async fn run_turns(
    input: &PresentInput,
    url: &str,
    resolver: std::sync::Arc<Rotating>,
) -> Result<(), Error> {
    use llm_core::Model;
    let catalog = Catalog::parse(&presentation_catalog(
        &input.auth_kind,
        &input.billing_kind,
        url,
    ))?;
    let binding = catalog
        .binding(&serving()?)
        .ok_or_else(|| Error::invalid("fixture binding is missing"))?
        .clone();
    let total = std::time::Duration::from_secs(10);
    let client = llm_messages::MessagesClient::new(
        binding,
        llm_http::HttpClient::new(llm_http::Limits {
            response_headers: total,
            idle: total,
            total,
        })?,
        resolver,
    )?;
    let request = turn(&input.request_json)?;
    for _ in 0..input.turns.unwrap_or(0) {
        let mut sink = VecSink::new(MAX_SINK_EVENTS, MAX_SINK_BYTES);
        client.turn(&request, &mut sink, &Cancel::new()).await?;
    }
    Ok(())
}
