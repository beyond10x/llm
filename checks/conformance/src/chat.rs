//! Observe the production Chat Completions projection, with no access to suite expectations.
//!
//! Every fact below is something a public `llm-chat` function returned. This module builds
//! fixture inputs and reads returned values; it never decides an outcome, never re-implements
//! a projection and never branches on a scenario name.
use ess_conformance::target::TargetError;
use llm_chat::{
    Account, BaseUrl, Binding, BindingDocument, Endpoint, IngressStream, Provider, ServedModel,
    ServingModel, SseEvent, StreamProjection, decode_completion, decode_ingress_request,
    encode_ingress_completion, project_request, project_response_bytes,
};
use llm_core::{
    AuthKind, BillingKind, Capabilities, Error, Id, Item, OutcomeDocument, Protocol, Provenance,
    StreamEvent, TurnDocument, TurnOutcome,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::target::Observed;

/// Every view name this domain answers through `query_view`.
pub const VIEWS: &[&str] = &[
    "llm.chat.LastRequestProjection",
    "llm.chat.LastResponseProjection",
    "llm.chat.LastIngressRequest",
    "llm.chat.LastIngressResponse",
];

/// Observe one command of this domain, or `None` when the command belongs to another.
pub fn observe(command: &str, input: &Value) -> Option<Result<Observed, TargetError>> {
    Some(match command {
        "llm.chat.ProjectRequest" => request_projection(input),
        "llm.chat.ProjectResponse" => response_projection(input),
        "llm.chat.DecodeIngress" => ingress_decoding(input),
        "llm.chat.EncodeIngress" => ingress_encoding(input),
        _ => return None,
    })
}

fn unavailable(error: impl std::fmt::Display) -> TargetError {
    TargetError::unavailable("chat observation", error.to_string())
}

fn observed(facts: Value, view: &'static str) -> Observed {
    Observed {
        facts,
        view,
        event: "llm.chat.Observed",
        field: "accepted",
    }
}

fn fault(facts: &mut Value, error: &Error) {
    facts["error_code"] = json!(error.code);
    // These diagnostics are fixed by construction; a scenario asserting one proves it
    // carries no byte of the request or of the upstream response.
    facts["error_message"] = json!(error.message);
    facts["dispatch"] = json!(error.dispatch);
}

fn identifier(value: &str) -> Result<Id, Error> {
    Id::new(value).map_err(|_| Error::invalid("fixture identifier"))
}

fn text_of(value: Option<&Value>) -> Value {
    json!(value.and_then(Value::as_str))
}

fn compact(value: Option<&Value>) -> Value {
    json!(value.map(ToString::to_string))
}

// ---------------------------------------------------------------- outgoing request

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestInput {
    turn_json: String,
    protocol: String,
    upstream_model: String,
    capabilities_json: String,
    streaming: bool,
}

/// The one binding shape a local, explicitly anonymous deployment declares. Only the
/// protocol, upstream model name and capabilities vary between scenarios.
fn fixture_binding(
    protocol: Protocol,
    upstream_model: &str,
    capabilities: Capabilities,
) -> Result<Binding, Error> {
    BindingDocument::new(
        Provider {
            id: identifier("my-lab")?,
            category: identifier("self-hosted")?,
        },
        Account {
            id: identifier("local")?,
            provider_id: identifier("my-lab")?,
            auth_kind: AuthKind::Anonymous,
            billing_kind: BillingKind::SelfHosted,
            secret_reference_id: None,
            api_key_header: None,
        },
        Endpoint {
            id: identifier("local-vllm")?,
            account_id: identifier("local")?,
            base_url: BaseUrl::new("http://127.0.0.1:8000/v1")?,
        },
        ServedModel {
            id: identifier("small")?,
            upstream_name: identifier(upstream_model)?,
        },
        ServingModel {
            id: identifier("local-small")?,
            endpoint_id: identifier("local-vllm")?,
            model_id: identifier("small")?,
            protocol,
            capabilities,
        },
    )
    .bind()
}

fn request_projection(input: &Value) -> Result<Observed, TargetError> {
    let input: RequestInput = serde_json::from_value(input.clone()).map_err(unavailable)?;
    let mut facts = json!({
        "accepted": false, "error_code": null, "error_message": null, "dispatch": null,
        "wire_model": null,
        "body_fields": [], "message_roles": [], "message_contents": [], "tool_arguments": [],
        "tool_choice": null, "output_limit_field": null, "output_limit": null,
        "stream": null, "include_usage": null
    });
    if let Err(error) = project(&input, &mut facts) {
        fault(&mut facts, &error);
    }
    Ok(observed(facts, "llm.chat.LastRequestProjection"))
}

fn project(input: &RequestInput, facts: &mut Value) -> Result<(), Error> {
    let protocol: Protocol = serde_json::from_value(json!(input.protocol))
        .map_err(|_| Error::invalid("unknown protocol name"))?;
    let capabilities: Capabilities = serde_json::from_str(&input.capabilities_json)
        .map_err(|_| Error::invalid("invalid capability declaration"))?;
    let binding = fixture_binding(protocol, &input.upstream_model, capabilities)?;
    let document: TurnDocument = serde_json::from_str(&input.turn_json)
        .map_err(|_| Error::invalid("invalid turn envelope"))?;

    let body = project_request(&document.request, &binding, input.streaming)?;

    facts["accepted"] = json!(true);
    facts["wire_model"] = text_of(body.get("model"));
    facts["tool_choice"] = compact(body.get("tool_choice"));
    facts["stream"] = json!(body.get("stream").and_then(Value::as_bool));
    facts["include_usage"] = json!(
        body.get("stream_options")
            .and_then(|options| options.get("include_usage"))
            .and_then(Value::as_bool)
    );
    for field in ["max_completion_tokens", "max_tokens"] {
        if let Some(limit) = body.get(field) {
            facts["output_limit_field"] = json!(field);
            facts["output_limit"] = json!(limit.to_string());
        }
    }
    if let Some(object) = body.as_object() {
        facts["body_fields"] = json!(object.keys().collect::<Vec<_>>());
    }
    let mut roles = Vec::new();
    let mut contents = Vec::new();
    let mut arguments = Vec::new();
    for message in body
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        roles.push(
            message
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        );
        contents.push(
            message
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        );
        for call in message
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let function = call.get("function");
            arguments.push(format!(
                "{}:{}:{}",
                call.get("id").and_then(Value::as_str).unwrap_or_default(),
                function
                    .and_then(|value| value.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                function
                    .and_then(|value| value.get("arguments"))
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            ));
        }
    }
    facts["message_roles"] = json!(roles);
    facts["message_contents"] = json!(contents);
    facts["tool_arguments"] = json!(arguments);
    Ok(())
}

// ---------------------------------------------------------------- incoming response

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResponseInput {
    response: String,
    streamed: bool,
    #[serde(default)]
    framed: Option<bool>,
}

/// The fixed serving binding a response is read against. Its own model identity stays
/// visible so a scenario can show that an unreported upstream model is not replaced by it.
fn fixture_target() -> Result<Provenance, Error> {
    Ok(Provenance {
        protocol: Protocol::ChatCompletions,
        provider: identifier("my-lab")?,
        account: identifier("local")?,
        endpoint: identifier("local-vllm")?,
        model: identifier("internal-model")?,
        binding_revision: identifier("binding-r1")?,
    })
}

fn response_projection(input: &Value) -> Result<Observed, TargetError> {
    let input: ResponseInput = serde_json::from_value(input.clone()).map_err(unavailable)?;
    let mut facts = json!({
        "accepted": false, "error_code": null, "error_message": null, "dispatch": null,
        "binding_model": "internal-model", "stream_text": "", "reasoning_text": "",
        "argument_deltas": [], "tool_stream": [], "assistant_text": null, "stop_reason": null,
        "tool_calls": [],
        "upstream_model": null, "response_id": null, "final_usage": null,
        "usage_reported": null, "input_tokens": null, "output_tokens": null,
        "cached_input_tokens": null, "cache_creation_input_tokens": null,
        "reasoning_output_tokens": null
    });
    if let Err(error) = decode_response(&input, &mut facts) {
        fault(&mut facts, &error);
    }
    Ok(observed(facts, "llm.chat.LastResponseProjection"))
}

fn decode_response(input: &ResponseInput, facts: &mut Value) -> Result<(), Error> {
    let target = fixture_target()?;
    facts["binding_model"] = json!(target.model.as_str());
    let outcome = if input.framed == Some(true) {
        // The third public entry point, reached the way a caller holding framed events
        // reaches it: no byte framing, no composing wrapper, just accept and finish.
        let payloads: Vec<Value> = serde_json::from_str(&input.response)
            .map_err(|_| Error::invalid("invalid framed payload list"))?;
        let mut projection = StreamProjection::new(target.clone());
        let mut events = Vec::new();
        let mut refusal = None;
        for payload in payloads {
            match projection.accept(&SseEvent::Payload {
                event: None,
                data: payload,
            }) {
                Ok(produced) => events.extend(produced),
                Err(error) => {
                    refusal = Some(error);
                    break;
                }
            }
        }
        report_events(&events, facts);
        if let Some(error) = refusal {
            return Err(error);
        }
        projection.finish()?
    } else if input.streamed {
        let (events, outcome) = project_response_bytes(input.response.as_bytes(), &target);
        report_events(&events, facts);
        outcome?
    } else {
        let body: Value = serde_json::from_str(&input.response)
            .map_err(|_| Error::invalid("invalid completion body"))?;
        decode_completion(&body, &target)?
    };
    facts["accepted"] = json!(true);
    report_outcome(&outcome, facts);
    Ok(())
}

fn report_events(events: &[StreamEvent], facts: &mut Value) {
    let mut stream_text = String::new();
    let mut reasoning_text = String::new();
    let mut deltas = Vec::new();
    let mut tool_stream = Vec::new();
    for event in events {
        match event {
            StreamEvent::TextDelta { text } => stream_text.push_str(text),
            StreamEvent::ReasoningDelta { text } => reasoning_text.push_str(text),
            StreamEvent::ToolCallStarted { call_id, name } => {
                tool_stream.push(format!("started:{call_id}:{name}"));
            }
            StreamEvent::ToolArgumentsDelta { call_id, delta } => {
                deltas.push(format!("{call_id}:{delta}"));
                tool_stream.push(format!("arguments:{call_id}:{delta}"));
            }
            StreamEvent::Warning { code, message } => deltas.push(format!("{code}:{message}")),
        }
    }
    facts["stream_text"] = json!(stream_text);
    facts["reasoning_text"] = json!(reasoning_text);
    facts["argument_deltas"] = json!(deltas);
    facts["tool_stream"] = json!(tool_stream);
}

fn report_outcome(outcome: &TurnOutcome, facts: &mut Value) {
    // Carried as text: the neutral stop reason is a tagged value, and a scenario asserts
    // the whole of it, including the reason an incomplete turn carries.
    facts["stop_reason"] = json!(serde_json::to_string(&outcome.stop_reason).ok());
    facts["assistant_text"] = json!(outcome.items.iter().find_map(|item| match item {
        Item::AssistantText { text } => Some(text.clone()),
        _ => None,
    }));
    facts["tool_calls"] = json!(
        outcome
            .tool_calls()
            .map(|call| format!("{}:{}:{}", call.call_id, call.name, call.arguments))
            .collect::<Vec<_>>()
    );
    let observation = &outcome.observation;
    facts["upstream_model"] = json!(observation.upstream_model.as_ref().map(Id::as_str));
    facts["response_id"] = json!(observation.response_id.as_ref().map(Id::as_str));
    facts["final_usage"] = json!(observation.final_usage);
    facts["usage_reported"] = json!(observation.usage.is_some());
    if let Some(usage) = &observation.usage {
        facts["input_tokens"] = json!(usage.input_tokens.map(|v| v.to_string()));
        facts["output_tokens"] = json!(usage.output_tokens.map(|v| v.to_string()));
        facts["cached_input_tokens"] = json!(usage.cached_input_tokens.map(|v| v.to_string()));
        facts["cache_creation_input_tokens"] =
            json!(usage.cache_creation_input_tokens.map(|v| v.to_string()));
        facts["reasoning_output_tokens"] =
            json!(usage.reasoning_output_tokens.map(|v| v.to_string()));
    }
}

// ---------------------------------------------------------------- gateway ingress

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IngressInput {
    request_json: String,
}

fn ingress_decoding(input: &Value) -> Result<Observed, TargetError> {
    let input: IngressInput = serde_json::from_value(input.clone()).map_err(unavailable)?;
    let mut facts = json!({
        "accepted": false, "error_code": null, "error_message": null, "dispatch": null,
        "model": null,
        "instructions": null, "items": [], "tools": [], "tool_choice": null,
        "output_limit": null, "stream": null, "include_usage": null, "turn_valid": null
    });
    if let Err(error) = decode_ingress(&input, &mut facts) {
        fault(&mut facts, &error);
    }
    Ok(observed(facts, "llm.chat.LastIngressRequest"))
}

fn decode_ingress(input: &IngressInput, facts: &mut Value) -> Result<(), Error> {
    let body: Value = serde_json::from_str(&input.request_json)
        .map_err(|_| Error::invalid("invalid chat request body"))?;
    let ingress = decode_ingress_request(&body)?;
    let request = ingress.request;
    facts["accepted"] = json!(true);
    facts["model"] = json!(request.model);
    facts["instructions"] = json!(request.instructions);
    facts["output_limit"] = json!(request.max_output_tokens.map(|v| v.to_string()));
    facts["stream"] = json!(ingress.stream);
    facts["include_usage"] = json!(ingress.include_usage);
    facts["tool_choice"] = json!(
        serde_json::to_string(&request.tool_choice).unwrap_or_else(|_| "unencodable".to_owned())
    );
    facts["tools"] = json!(
        request
            .tools
            .iter()
            .map(|tool| format!("{}:{}:{}", tool.name, tool.description, tool.input_schema))
            .collect::<Vec<_>>()
    );
    facts["items"] = json!(
        request
            .items
            .iter()
            .map(|item| match item {
                Item::UserText { text } => format!("user:{text}"),
                Item::AssistantText { text } => format!("assistant:{text}"),
                Item::ToolCall(call) => format!(
                    "tool-call:{}:{}:{}",
                    call.call_id, call.name, call.arguments
                ),
                Item::ToolResult {
                    call_id,
                    output,
                    failed,
                } => format!("tool-result:{call_id}:{output}:{failed}"),
                Item::Opaque { .. } => "opaque".to_owned(),
            })
            .collect::<Vec<_>>()
    );
    facts["turn_valid"] = json!(request.validate().is_ok());
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EncodeInput {
    outcome_json: String,
    streamed: bool,
    include_usage: bool,
    #[serde(default)]
    events_json: Option<String>,
}

fn ingress_encoding(input: &Value) -> Result<Observed, TargetError> {
    let input: EncodeInput = serde_json::from_value(input.clone()).map_err(unavailable)?;
    let mut facts = json!({
        "accepted": false, "error_code": null, "error_message": null, "dispatch": null,
        "model_reported": null,
        "content": null, "finish_reason": null, "tool_calls": [], "usage_json": null,
        "chunk_count": null, "streamed_tool_calls": []
    });
    if let Err(error) = encode_ingress(&input, &mut facts) {
        fault(&mut facts, &error);
    }
    Ok(observed(facts, "llm.chat.LastIngressResponse"))
}

fn encode_ingress(input: &EncodeInput, facts: &mut Value) -> Result<(), Error> {
    let document: OutcomeDocument = serde_json::from_str(&input.outcome_json)
        .map_err(|_| Error::invalid("invalid outcome envelope"))?;
    let outcome = document.outcome;
    if input.streamed {
        // Raw for the same reason the outcome is: a malformed event must reach the
        // production decoder rather than being filtered out by the test target.
        let events: Vec<StreamEvent> = match &input.events_json {
            Some(events) => {
                serde_json::from_str(events).map_err(|_| Error::invalid("invalid stream events"))?
            }
            None => Vec::new(),
        };
        let mut stream = IngressStream::new("chatcmpl-fixture", 1_772_000_000);
        let mut streamed = Vec::new();
        for event in &events {
            if let Some(calls) = stream
                .chunk(event)
                .as_ref()
                .and_then(|chunk| chunk.pointer("/choices/0/delta/tool_calls"))
                .and_then(Value::as_array)
            {
                streamed.extend(calls.iter().map(ToString::to_string));
            }
        }
        facts["streamed_tool_calls"] = json!(streamed);
        let chunks = stream.close(&outcome, input.include_usage)?;
        facts["accepted"] = json!(true);
        facts["chunk_count"] = json!(chunks.len().to_string());
        for chunk in &chunks {
            if let Some(model) = chunk.get("model") {
                facts["model_reported"] = text_of(Some(model));
            }
            if let Some(usage) = chunk.get("usage") {
                facts["usage_json"] = compact(Some(usage));
            }
            let choice = chunk.get("choices").and_then(|value| value.get(0));
            let delta = choice.and_then(|value| value.get("delta"));
            if let Some(reason) = choice
                .and_then(|value| value.get("finish_reason"))
                .filter(|reason| !reason.is_null())
            {
                facts["finish_reason"] = text_of(Some(reason));
            }
            if let Some(calls) = delta
                .and_then(|value| value.get("tool_calls"))
                .and_then(Value::as_array)
            {
                facts["tool_calls"] =
                    json!(calls.iter().map(ToString::to_string).collect::<Vec<_>>());
            }
        }
        return Ok(());
    }
    let body = encode_ingress_completion(&outcome, "chatcmpl-fixture", 1_772_000_000)?;
    facts["accepted"] = json!(true);
    facts["model_reported"] = text_of(body.get("model"));
    facts["usage_json"] = compact(body.get("usage"));
    let message = body
        .get("choices")
        .and_then(|value| value.get(0))
        .and_then(|choice| choice.get("message"));
    facts["content"] = text_of(message.and_then(|value| value.get("content")));
    facts["finish_reason"] = text_of(
        body.get("choices")
            .and_then(|value| value.get(0))
            .and_then(|choice| choice.get("finish_reason")),
    );
    if let Some(calls) = message
        .and_then(|value| value.get("tool_calls"))
        .and_then(Value::as_array)
    {
        facts["tool_calls"] = json!(calls.iter().map(ToString::to_string).collect::<Vec<_>>());
    }
    Ok(())
}
