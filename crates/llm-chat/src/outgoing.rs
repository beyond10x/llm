//! The outgoing Chat Completions request built from a validated neutral turn.
use llm_core::{Advisory, Error, Item, Protocol, ToolChoice, ToolSpec, TurnRequest, wire_effort};
use llm_providers::Binding;
use serde_json::{Map, Value, json};

/// The advisory hints this wire sends: the Chat Completions column of the design's table.
const CARRIED: &[Advisory] = &[Advisory::PromptCacheKey, Advisory::Verbosity];

/// The advisory hints `request` carries that [`project_request`] does not send, in
/// [`Advisory`] order, so a gateway can report them before it forwards anything.
pub fn omitted_advisories(request: &TurnRequest) -> Vec<Advisory> {
    request.controls.hints.omitted_by(CARRIED)
}

/// Project a neutral turn onto one Chat Completions request body.
///
/// The body names the binding's upstream model, never the caller's route alias, and carries
/// only the settings the caller actually asked for.
///
/// # Errors
/// Refuses a binding that serves another protocol, a request the binding's declared
/// capabilities do not support, opaque continuation state this wire cannot carry, a thinking
/// budget and disabled thinking beside an effort ([`wire_effort`]), and every failure
/// [`TurnRequest::validate_for`] reports.
pub fn project_request(
    request: &TurnRequest,
    binding: &Binding,
    streaming: bool,
) -> Result<Value, Error> {
    if binding.provenance().protocol != Protocol::ChatCompletions {
        return Err(Error::invalid(
            "selected binding does not serve Chat Completions",
        ));
    }
    request.validate_for(binding.provenance(), binding.capabilities())?;

    let mut body = Map::new();
    body.insert("model".to_owned(), json!(binding.upstream_model()));
    body.insert("messages".to_owned(), Value::Array(messages(request)?));
    if !request.tools.is_empty() {
        body.insert(
            "tools".to_owned(),
            Value::Array(request.tools.iter().map(tool).collect()),
        );
    }
    match &request.tool_choice {
        // Omission is this wire's own default; sending it changes nothing and hides nothing.
        ToolChoice::Auto => {}
        ToolChoice::Required => {
            body.insert("tool_choice".to_owned(), json!("required"));
        }
        ToolChoice::Named(name) => {
            body.insert(
                "tool_choice".to_owned(),
                json!({"type": "function", "function": {"name": name.as_str()}}),
            );
        }
    }
    if let Some(limit) = request.max_output_tokens {
        // `max_tokens` is deprecated on this wire; the current field is the one sent.
        body.insert("max_completion_tokens".to_owned(), json!(limit));
    }
    if let Some(value) = request.sampling.temperature {
        body.insert("temperature".to_owned(), json!(value));
    }
    if let Some(value) = request.sampling.top_p {
        body.insert("top_p".to_owned(), json!(value));
    }
    // Not the neutral effort as it is: `max` and disabled thinking have their own names here,
    // and a thinking budget has none.
    if let Some(effort) = wire_effort(Protocol::ChatCompletions, request)? {
        body.insert("reasoning_effort".to_owned(), json!(effort));
    }
    if let Some(parallel) = request.controls.parallel_tool_calls {
        body.insert("parallel_tool_calls".to_owned(), json!(parallel));
    }
    let hints = &request.controls.hints;
    if let Some(key) = &hints.prompt_cache_key {
        body.insert("prompt_cache_key".to_owned(), json!(key));
    }
    if let Some(verbosity) = hints.verbosity {
        body.insert("verbosity".to_owned(), json!(verbosity));
    }
    body.insert("stream".to_owned(), json!(streaming));
    if streaming {
        // Without this the server reports no usage at all, and an absent report would be
        // indistinguishable from a model that consumed nothing.
        body.insert("stream_options".to_owned(), json!({"include_usage": true}));
    }
    Ok(Value::Object(body))
}

/// One assistant turn on this wire is one message carrying its text and its proposed calls,
/// so a run of neutral assistant items collapses into a single message.
#[derive(Default)]
struct OpenAssistant {
    content: Option<String>,
    refusal: Option<String>,
    calls: Vec<Value>,
}

/// Appends `text` to `slot`, a newline between items as one message joins them.
pub(crate) fn join(slot: &mut Option<String>, text: &str) {
    match slot {
        Some(existing) => {
            existing.push('\n');
            existing.push_str(text);
        }
        slot => *slot = Some(text.to_owned()),
    }
}

impl OpenAssistant {
    fn close(open: &mut Option<Self>, out: &mut Vec<Value>) {
        let Some(assistant) = open.take() else {
            return;
        };
        let mut message = Map::new();
        message.insert("role".to_owned(), json!("assistant"));
        if let Some(content) = assistant.content {
            message.insert("content".to_owned(), json!(content));
        }
        if let Some(refusal) = assistant.refusal {
            message.insert("refusal".to_owned(), json!(refusal));
        }
        if !assistant.calls.is_empty() {
            message.insert("tool_calls".to_owned(), Value::Array(assistant.calls));
        }
        out.push(Value::Object(message));
    }
}

fn messages(request: &TurnRequest) -> Result<Vec<Value>, Error> {
    let mut out = Vec::new();
    if !request.instructions.is_empty() {
        out.push(json!({"role": "system", "content": request.instructions}));
    }
    let mut open: Option<OpenAssistant> = None;
    for item in &request.items {
        match item {
            Item::UserText { text } => {
                OpenAssistant::close(&mut open, &mut out);
                out.push(json!({"role": "user", "content": text}));
            }
            Item::AssistantText { text } => {
                join(
                    &mut open.get_or_insert_with(OpenAssistant::default).content,
                    text,
                );
            }
            // The assistant message's own `refusal` field, never its content.
            Item::AssistantRefusal { text } => {
                join(
                    &mut open.get_or_insert_with(OpenAssistant::default).refusal,
                    text,
                );
            }
            Item::ToolCall(call) => {
                let assistant = open.get_or_insert_with(OpenAssistant::default);
                assistant.calls.push(json!({
                    "id": call.call_id.as_str(),
                    "type": "function",
                    "function": {
                        "name": call.name.as_str(),
                        // This wire carries arguments as encoded text, not as an object.
                        "arguments": compact(&call.arguments),
                    }
                }));
            }
            Item::ToolResult {
                call_id,
                output,
                failed,
            } => {
                OpenAssistant::close(&mut open, &mut out);
                out.push(json!({
                    "role": "tool",
                    "tool_call_id": call_id.as_str(),
                    "content": result_text(output, *failed),
                }));
            }
            Item::Opaque { .. } => {
                // Preserved or refused, never dropped to make a translation look successful.
                return Err(Error::unsupported(
                    "chat completions carries no opaque continuation state",
                ));
            }
            // Not sendable anywhere until a caller binds it, and not sendable here even then.
            Item::UnattributedOpaque { .. } => {
                return Err(Error::unsupported(Item::UNATTRIBUTED_REFUSAL));
            }
        }
    }
    OpenAssistant::close(&mut open, &mut out);
    Ok(out)
}

fn tool(spec: &ToolSpec) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": spec.name.as_str(),
            "description": spec.description,
            "parameters": spec.input_schema,
        }
    })
}

/// A tool message carries text only, so a failure has to travel inside it. The envelope is the
/// one `beyond10x/harness` 9e401e40 uses on the equally text-only Responses wire
/// (`crates/harness-responses/src/project.rs`): an empty failed result must never read to the
/// model as an empty success.
fn result_text(output: &Value, failed: bool) -> String {
    if failed {
        return json!({"ok": false, "error": output}).to_string();
    }
    match output {
        // A string result reaches the model as prose, not as a quoted blob.
        Value::String(text) => text.clone(),
        other => compact(other),
    }
}

fn compact(value: &Value) -> String {
    value.to_string()
}
