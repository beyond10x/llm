//! Observe the production Responses projection, with no access to suite expectations.
//!
//! Every fact below is a returned value of a public `llm-responses` function, reshaped for the
//! view and nothing more. This module reimplements no projection rule, reads no suite, and
//! branches on no scenario name.

use ess_conformance::target::TargetError;
use llm_core::{
    Error, Id, Item, Provenance, StopReason, StreamEvent, TurnDocument, TurnObservation,
};
use llm_responses::{
    Binding, Conversation, PATH, decode_stream, encode_request, ingest_request, project_request,
    request_headers,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::target::Observed;

/// Every view name this domain answers through `query_view`.
pub const VIEWS: &[&str] = &[
    "llm.responses.LastProjection",
    "llm.responses.LastStream",
    "llm.responses.LastIngestion",
];

/// Observe one command of this domain, or `None` when the command belongs to another.
pub fn observe(command: &str, input: &Value) -> Option<Result<Observed, TargetError>> {
    match command {
        "llm.responses.Project" => Some(project(input)),
        "llm.responses.DecodeStream" => Some(stream(input)),
        "llm.responses.Ingest" => Some(ingest(input)),
        _ => None,
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectInput {
    binding_json: String,
    upstream_model: String,
    turn_json: String,
    conversation_id: Option<String>,
    originator: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StreamInput {
    binding_json: String,
    upstream_model: String,
    stream_json: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IngestInput {
    binding_json: String,
    upstream_model: String,
    body_json: String,
}

fn unavailable(error: impl std::fmt::Display) -> TargetError {
    TargetError::unavailable("responses observation", error.to_string())
}

/// The binding under observation. Both inputs stay raw so the production decoders see them.
fn binding(binding_json: &str, upstream_model: &str) -> Result<Binding, Error> {
    let provenance: Provenance = serde_json::from_str(binding_json)
        .map_err(|_| Error::invalid("invalid binding document"))?;
    let upstream =
        Id::new(upstream_model).map_err(|_| Error::invalid("invalid upstream model name"))?;
    Ok(Binding::new(provenance, upstream))
}

fn project(input: &Value) -> Result<Observed, TargetError> {
    let input: ProjectInput = serde_json::from_value(input.clone()).map_err(unavailable)?;
    let mut facts = json!({
        "accepted": false, "error_code": null, "path": null, "wire_model": null,
        "stream": null, "store": null, "include": [], "input_json": null,
        "tool_names": [], "tool_choice": null, "max_output_tokens": null,
        "sampling": [], "request_preserved": false, "encoded_body": null, "body_fields": [],
        "prompt_cache_key": null, "request_headers": []
    });
    if let Err(error) = projection(&input, &mut facts) {
        facts["error_code"] = json!(error.code);
    }
    Ok(Observed {
        facts,
        view: "llm.responses.LastProjection",
        event: "llm.responses.Projected",
        field: "accepted",
    })
}

fn projection(input: &ProjectInput, facts: &mut Value) -> Result<(), Error> {
    let binding = binding(&input.binding_json, &input.upstream_model)?;
    let document: TurnDocument = serde_json::from_str(&input.turn_json)
        .map_err(|_| Error::invalid("invalid turn envelope"))?;
    let conversation = conversation(input)?;
    // The bytes the client would send, read back for every fact below: the projection's own
    // value is never consulted beside them, so no fact can describe a body that was not encoded.
    let encoded = encode_request(&binding, &document.request, conversation.as_ref())?;
    let body: Value = serde_json::from_slice(&encoded)
        .map_err(|_| Error::invalid("encoded request is not JSON"))?;
    facts["accepted"] = json!(true);
    facts["encoded_body"] = json!(
        String::from_utf8(encoded).map_err(|_| Error::invalid("encoded request is not UTF-8"))?
    );
    facts["body_fields"] = json!(
        body.as_object()
            .map(|fields| fields.keys().map(String::as_str).collect::<Vec<_>>())
            .unwrap_or_default()
    );
    facts["prompt_cache_key"] = body.get("prompt_cache_key").cloned().unwrap_or(Value::Null);
    facts["request_headers"] = json!(
        request_headers(conversation.as_ref(), 0)
            .into_iter()
            .map(|(name, value)| format!("{name}: {value}"))
            .collect::<Vec<_>>()
    );
    facts["path"] = json!(PATH);
    facts["wire_model"] = body["model"].clone();
    facts["stream"] = body["stream"].clone();
    facts["store"] = body["store"].clone();
    facts["include"] = json!(
        body["include"]
            .as_array()
            .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>())
            .unwrap_or_default()
    );
    facts["input_json"] = json!(compact(&body["input"]));
    facts["tool_names"] = json!(
        body["tools"]
            .as_array()
            .map(|tools| tools
                .iter()
                .filter_map(|tool| tool.get("name").and_then(Value::as_str))
                .collect::<Vec<_>>())
            .unwrap_or_default()
    );
    facts["tool_choice"] = match body.get("tool_choice") {
        None => Value::Null,
        Some(choice) => json!(compact(choice)),
    };
    facts["max_output_tokens"] = json!(
        body.get("max_output_tokens")
            .and_then(Value::as_u64)
            .map(|limit| limit.to_string())
    );
    facts["sampling"] = json!(
        ["temperature", "top_p", "reasoning"]
            .into_iter()
            .filter(|field| body.get(*field).is_some())
            .collect::<Vec<_>>()
    );
    // The same contract read the other way. A body that does not return the turn that produced
    // it is a translation that lost something, whatever else it got right.
    facts["request_preserved"] =
        json!(ingest_request(&binding, &body).is_ok_and(|returned| returned == document.request));
    Ok(())
}

/// The conversation the scenario opted the client into, built with the production constructors.
fn conversation(input: &ProjectInput) -> Result<Option<Conversation>, Error> {
    let identifier =
        |value: &str| Id::new(value).map_err(|_| Error::invalid("invalid conversation identifier"));
    match (&input.conversation_id, &input.originator) {
        (None, None) => Ok(None),
        (None, Some(_)) => Err(Error::invalid("an originator names no conversation")),
        (Some(conversation), originator) => {
            let mut conversation = Conversation::new(identifier(conversation)?);
            if let Some(originator) = originator {
                conversation = conversation.with_originator(identifier(originator)?);
            }
            Ok(Some(conversation))
        }
    }
}

fn ingest(input: &Value) -> Result<Observed, TargetError> {
    let input: IngestInput = serde_json::from_value(input.clone()).map_err(unavailable)?;
    let mut facts = json!({
        "accepted": false, "error_code": null, "turn_json": null, "model": null,
        "instructions": null, "item_kinds": [], "tool_names": [],
        "reprojected": false, "body_preserved": false, "forward_refusal": null,
        "bound_input_json": null, "bound_body_preserved": false
    });
    if let Err(error) = ingestion(&input, &mut facts) {
        facts["error_code"] = json!(error.code);
    }
    Ok(Observed {
        facts,
        view: "llm.responses.LastIngestion",
        event: "llm.responses.Ingested",
        field: "accepted",
    })
}

fn ingestion(input: &IngestInput, facts: &mut Value) -> Result<(), Error> {
    let binding = binding(&input.binding_json, &input.upstream_model)?;
    let body: Value =
        serde_json::from_str(&input.body_json).map_err(|_| Error::invalid("invalid wire body"))?;
    let request = ingest_request(&binding, &body)?;
    facts["accepted"] = json!(true);
    facts["model"] = json!(request.model);
    facts["instructions"] = json!(request.instructions);
    facts["item_kinds"] = json!(request.items.iter().map(item_kind).collect::<Vec<_>>());
    facts["tool_names"] = json!(
        request
            .tools
            .iter()
            .map(|tool| tool.name.to_string())
            .collect::<Vec<_>>()
    );
    // The same contract read back out. A gateway that accepts a request its own egress refuses,
    // or that rewrites a field it accepted and never read, disagrees with itself; both are
    // visible here and in neither of the other two views.
    match project_request(&binding, &request) {
        Ok(reprojected) => {
            facts["reprojected"] = json!(true);
            facts["body_preserved"] = json!(preserved(&body, &reprojected));
        }
        // The forward attempt without the caller's binding decision. The diagnostic is fixed by
        // construction, so the scenario can pin which refusal it was.
        Err(error) => facts["forward_refusal"] = json!(error.message),
    }
    // The caller's explicit decision, made here because this adapter stands in for the caller:
    // bind whatever ingress carried unattributed to the binding that read it, then forward it.
    let mut bound = request.clone();
    if bound.bind_unattributed(binding.provenance()).is_ok()
        && let Ok(reprojected) = project_request(&binding, &bound)
    {
        facts["bound_input_json"] = json!(compact(&reprojected["input"]));
        facts["bound_body_preserved"] = json!(preserved(&body, &reprojected));
    }
    facts["turn_json"] = json!(compact(&serialize(&TurnDocument::new(request))?));
    Ok(())
}

/// Every field the body carried comes back identically. A body may legitimately omit a field the
/// projection always writes, so the comparison is over what was sent, not over what was produced.
fn preserved(sent: &Value, reprojected: &Value) -> bool {
    sent.as_object().is_some_and(|sent| {
        sent.iter()
            .all(|(field, value)| reprojected.get(field) == Some(value))
    })
}

fn stream(input: &Value) -> Result<Observed, TargetError> {
    let input: StreamInput = serde_json::from_value(input.clone()).map_err(unavailable)?;
    let mut facts = json!({
        "accepted": false, "error_code": null, "error_message": null, "dispatch": null,
        "stop_reason": null, "incomplete_reason": null, "items_json": null,
        "stream_events": [], "upstream_model": null, "response_id": null, "final_usage": null,
        "input_tokens": null, "output_tokens": null, "cached_input_tokens": null,
        "cache_creation_input_tokens": null, "reasoning_output_tokens": null
    });
    if let Err(error) = decoding(&input, &mut facts) {
        facts["error_code"] = json!(error.code);
    }
    Ok(Observed {
        facts,
        view: "llm.responses.LastStream",
        event: "llm.responses.StreamDecoded",
        field: "accepted",
    })
}

fn decoding(input: &StreamInput, facts: &mut Value) -> Result<(), Error> {
    let binding = binding(&input.binding_json, &input.upstream_model)?;
    let payloads: Vec<Value> = serde_json::from_str(&input.stream_json)
        .map_err(|_| Error::invalid("invalid stream fixture"))?;
    let decoding = decode_stream(&binding, &payloads);
    facts["stream_events"] = json!(decoding.events.iter().map(stream_event).collect::<Vec<_>>());
    match decoding.result {
        Ok(outcome) => {
            facts["accepted"] = json!(true);
            facts["stop_reason"] = json!(stop_reason(&outcome.stop_reason));
            facts["incomplete_reason"] = match &outcome.stop_reason {
                StopReason::Incomplete { reason } => json!(reason),
                _ => Value::Null,
            };
            facts["items_json"] = json!(compact(&serialize(&outcome.items)?));
            observation(&outcome.observation, facts);
            Ok(())
        }
        Err(error) => {
            facts["error_message"] = json!(error.message);
            facts["dispatch"] = json!(error.dispatch);
            // The refusal is not the end of what is known: a failure that reached the provider
            // still reports whatever counters it reported.
            if let Some(retained) = &error.observation {
                observation(retained, facts);
            }
            Err(error)
        }
    }
}

fn observation(observation: &TurnObservation, facts: &mut Value) {
    facts["upstream_model"] = json!(observation.upstream_model);
    facts["response_id"] = json!(observation.response_id);
    facts["final_usage"] = json!(observation.final_usage);
    if let Some(usage) = &observation.usage {
        // Decimal text, so an exact u64 crosses without becoming a float.
        facts["input_tokens"] = json!(usage.input_tokens.map(|v| v.to_string()));
        facts["output_tokens"] = json!(usage.output_tokens.map(|v| v.to_string()));
        facts["cached_input_tokens"] = json!(usage.cached_input_tokens.map(|v| v.to_string()));
        facts["cache_creation_input_tokens"] =
            json!(usage.cache_creation_input_tokens.map(|v| v.to_string()));
        facts["reasoning_output_tokens"] =
            json!(usage.reasoning_output_tokens.map(|v| v.to_string()));
    }
}

fn stop_reason(reason: &StopReason) -> &'static str {
    match reason {
        StopReason::EndTurn => "end-turn",
        StopReason::ToolCalls => "tool-calls",
        StopReason::MaxOutputTokens => "max-output-tokens",
        StopReason::Incomplete { .. } => "incomplete",
    }
}

fn item_kind(item: &Item) -> &'static str {
    match item {
        Item::UserText { .. } => "user-text",
        Item::AssistantText { .. } => "assistant-text",
        Item::ToolCall(_) => "tool-call",
        Item::ToolResult { .. } => "tool-result",
        Item::Opaque { .. } => "opaque",
        Item::UnattributedOpaque { .. } => "unattributed-opaque",
    }
}

fn stream_event(event: &StreamEvent) -> String {
    match event {
        StreamEvent::TextDelta { text } => format!("text-delta:{text}"),
        StreamEvent::ReasoningDelta { text } => format!("reasoning-delta:{text}"),
        StreamEvent::ToolCallStarted { call_id, name } => {
            format!("tool-call-started:{call_id}:{name}")
        }
        StreamEvent::ToolArgumentsDelta { call_id, delta } => {
            format!("tool-arguments-delta:{call_id}:{delta}")
        }
        StreamEvent::Warning { code, .. } => format!("warning:{code}"),
    }
}

fn serialize<T: serde::Serialize>(value: &T) -> Result<Value, Error> {
    serde_json::to_value(value).map_err(|_| Error::invalid("returned value is not encodable"))
}

fn compact(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}
