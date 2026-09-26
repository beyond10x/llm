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
pub const VIEWS: &[&str] = &["llm.messages.LastInspection"];

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
