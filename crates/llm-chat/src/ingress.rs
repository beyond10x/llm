//! Gateway ingress: a Chat Completions client request decoded onto the neutral subset, and a
//! neutral outcome encoded back onto the same wire.
//!
//! A request is read strictly. Every field outside the published subset is refused, because a
//! gateway that quietly ignores `response_format` or `n` answers a question it was not asked.
//! That holds at every level: each object of the request is closed, and a key it does not
//! publish is refused naming its path (`messages[].content[].cache_control`).
use llm_core::{
    CallId, Error, Item, MAX_TOOL_RESULT_BYTES, Sampling, StopReason, StreamEvent, ToolChoice,
    ToolName, ToolSpec, TurnOutcome, TurnRequest, Usage, Verbosity, field_path,
};
use serde_json::{Map, Value, json};

/// Every Chat Completions request field this projection implements.
pub const SUPPORTED_REQUEST_FIELDS: &[&str] = &[
    "model",
    "messages",
    "tools",
    "tool_choice",
    "max_completion_tokens",
    "max_tokens",
    "temperature",
    "top_p",
    "reasoning_effort",
    "stream",
    "stream_options",
    "parallel_tool_calls",
    "prompt_cache_key",
    "verbosity",
];

// The keys each closed level below the top publishes (`docs/design.md` "Client request
// fields"). Anything else is refused by its path.

/// A `system`, `developer` or `user` message.
const TEXT_MESSAGE_KEYS: &[&str] = &["role", "content"];
const ASSISTANT_MESSAGE_KEYS: &[&str] = &["role", "content", "tool_calls"];
const TOOL_MESSAGE_KEYS: &[&str] = &["role", "content", "tool_call_id"];
const CONTENT_PART_KEYS: &[&str] = &["type", "text"];
const TOOL_CALL_KEYS: &[&str] = &["id", "type", "function"];
const TOOL_CALL_FUNCTION_KEYS: &[&str] = &["name", "arguments"];
const TOOL_KEYS: &[&str] = &["type", "function"];
const TOOL_FUNCTION_KEYS: &[&str] = &["name", "description", "parameters", "strict"];
const TOOL_CHOICE_KEYS: &[&str] = &["type", "function"];
const TOOL_CHOICE_FUNCTION_KEYS: &[&str] = &["name"];
const STREAM_OPTION_KEYS: &[&str] = &["include_usage"];

/// Request fields this subset deliberately does not implement, named so a client learns which
/// one was refused. A field in neither list is refused too, by the path `llm_core::field_path`
/// writes for it, which quotes no value the client sent.
pub const NAMED_UNSUPPORTED_REQUEST_FIELDS: &[&str] = &[
    "n",
    "logprobs",
    "top_logprobs",
    "response_format",
    "seed",
    "stop",
    "frequency_penalty",
    "presence_penalty",
    "logit_bias",
    "functions",
    "function_call",
    "modalities",
    "audio",
    "prediction",
    "web_search_options",
    "store",
    "metadata",
    "service_tier",
    "user",
];

/// One accepted client request: the neutral turn it asks for, and how it wants to be answered.
#[derive(Debug, Clone)]
pub struct IngressRequest {
    pub request: TurnRequest,
    pub stream: bool,
    pub include_usage: bool,
}

/// Decode one Chat Completions request body onto the neutral subset.
///
/// # Errors
/// Refuses any field at any level, role, content part, tool call type, tool choice or strict
/// schema outside the published subset, contradictory output limits, instructions that do not
/// precede the conversation, and every malformed value the neutral vocabulary cannot hold.
pub fn decode_ingress_request(body: &Value) -> Result<IngressRequest, Error> {
    let object = body
        .as_object()
        .ok_or_else(|| Error::invalid("chat request is not a JSON object"))?;
    refuse_unsupported_fields(object)?;

    let model = object
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid("chat request names no model"))?;
    let messages = object
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::invalid("chat request carries no messages"))?;

    let mut request = TurnRequest::new(model, Vec::new());
    decode_messages(messages, &mut request)?;
    if let Some(tools) = object.get("tools") {
        request.tools = decode_tools(tools)?;
    }
    request.tool_choice = decode_tool_choice(object.get("tool_choice"))?;
    request.max_output_tokens = decode_output_limit(object)?;
    request.sampling = Sampling {
        temperature: number(object.get("temperature"), "temperature")?,
        top_p: number(object.get("top_p"), "top_p")?,
        reasoning_effort: text(object.get("reasoning_effort"), "reasoning_effort")?,
    };
    request.controls.parallel_tool_calls =
        optional_flag(object.get("parallel_tool_calls"), "parallel_tool_calls")?;
    request.controls.hints.prompt_cache_key =
        text(object.get("prompt_cache_key"), "prompt_cache_key")?;
    request.controls.hints.verbosity = decode_verbosity(object.get("verbosity"))?;

    let stream = flag(object.get("stream"), "stream")?;
    let include_usage = match object.get("stream_options") {
        None | Some(Value::Null) => false,
        Some(options @ Value::Object(_)) => {
            closed(options, "stream_options", STREAM_OPTION_KEYS)?;
            flag(options.get("include_usage"), "include_usage")?
        }
        Some(_) => {
            return Err(Error::invalid(
                "chat request stream_options is not an object",
            ));
        }
    };
    // The neutral subset is the contract this gateway serves, and `validate` is what says
    // what is in it: duplicate call identifiers, a tool result answering no call, a named
    // tool choice with no published tools, a temperature outside its range, a zero output
    // limit. Accepting those here would only move the same refusal to `project_request`,
    // after the client has been told yes and after a route has been chosen.
    request.validate()?;
    Ok(IngressRequest {
        request,
        stream,
        include_usage,
    })
}

fn refuse_unsupported_fields(object: &Map<String, Value>) -> Result<(), Error> {
    for field in NAMED_UNSUPPORTED_REQUEST_FIELDS {
        if object.contains_key(*field) {
            // The name comes from this list, never from the request, so no client byte
            // can reach a diagnostic through it.
            return Err(Error::unsupported(format!(
                "chat request field `{field}` is outside the published subset"
            )));
        }
    }
    closed_object(object, "", SUPPORTED_REQUEST_FIELDS)
}

/// Refuse the first key of `value` outside `published`, naming its path under `at`. A value
/// that is not an object has no keys; the reader of that level refuses its shape.
fn closed(value: &Value, at: &str, published: &[&str]) -> Result<(), Error> {
    match value.as_object() {
        Some(object) => closed_object(object, at, published),
        None => Ok(()),
    }
}

fn closed_object(object: &Map<String, Value>, at: &str, published: &[&str]) -> Result<(), Error> {
    match object.keys().find(|key| !published.contains(&key.as_str())) {
        // `field_path` copies only a bounded name of the wire's own shape, so no client byte
        // beyond such a name reaches the diagnostic.
        Some(key) => Err(Error::unsupported(format!(
            "chat request field is outside the published subset: {}",
            field_path(at, key)
        ))),
        None => Ok(()),
    }
}

fn decode_messages(messages: &[Value], request: &mut TurnRequest) -> Result<(), Error> {
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::invalid("chat message carries no role"))?;
        let published = match role {
            "system" | "developer" | "user" => TEXT_MESSAGE_KEYS,
            "assistant" => ASSISTANT_MESSAGE_KEYS,
            "tool" => TOOL_MESSAGE_KEYS,
            _ => {
                return Err(Error::unsupported(
                    "chat message role is outside the published subset",
                ));
            }
        };
        closed(message, "messages[]", published)?;
        match role {
            "system" | "developer" => {
                if !request.items.is_empty() {
                    return Err(Error::unsupported(
                        "chat system instructions must precede the conversation",
                    ));
                }
                let text = content_text(message.get("content"))?;
                if !request.instructions.is_empty() {
                    request.instructions.push('\n');
                }
                request.instructions.push_str(&text);
            }
            "user" => {
                let text = content_text(message.get("content"))?;
                request.items.push(Item::user(text));
            }
            "assistant" => {
                let text = content_text(message.get("content"))?;
                if !text.is_empty() {
                    request.items.push(Item::assistant(text));
                }
                match message.get("tool_calls") {
                    None | Some(Value::Null) => {}
                    Some(Value::Array(calls)) => {
                        for call in calls {
                            request.items.push(decode_tool_call(call)?);
                        }
                    }
                    Some(_) => {
                        return Err(Error::invalid("chat message tool_calls is not an array"));
                    }
                }
            }
            "tool" => {
                let call_id = message
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::invalid("chat tool message names no call"))?;
                // The call is checked before the content is read, so a message wrong in both
                // is refused for its call.
                let call_id = call_id_of(call_id)?;
                // This wire carries the result as text and has no place to say a call failed,
                // so the outgoing projection sends a failure as the object
                // `{"error": <output>, "ok": false}`; only that object reads back as failed.
                let (output, failed) = read_tool_result(content_text(message.get("content"))?);
                request.items.push(Item::ToolResult {
                    call_id,
                    output,
                    failed,
                });
            }
            // Every other role was refused above, before its keys were read.
            _ => {}
        }
    }
    Ok(())
}

/// The object the outgoing projection sends for a failed result (`outgoing.rs` `result_text`):
/// exactly `error` and `ok`, each once.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FailureObject {
    error: Value,
    ok: bool,
}

/// A tool message's text read back as a result: failed with the `error` value as its output
/// when the text is a `FailureObject` whose `ok` is `false`, and otherwise successful with the
/// text as a string, byte for byte. Keeping other text a string is lossless; guessing that it
/// was meant to be JSON would not be. A repeated or extra key is not the failure object.
///
/// Only text that opens with `{` and fits the tool-result bound is parsed: longer text is left to
/// that bound, which refuses it, and `serde_json` parses at most 127 levels deep, so a failed
/// output nested deeper than 126 levels reads back as a successful string. A successful result
/// whose text is exactly the failure object reads back as failed: the wire cannot tell the two
/// apart.
fn read_tool_result(text: String) -> (Value, bool) {
    if text.len() <= MAX_TOOL_RESULT_BYTES
        && text.trim_start().starts_with('{')
        && let Ok(FailureObject { error, ok: false }) = serde_json::from_str(&text)
    {
        return (error, true);
    }
    (Value::String(text), false)
}

/// Whether a `type` key leaves its object in the subset: absent, `null` or `function`.
fn is_function_type(value: &Value) -> bool {
    match value.get("type") {
        None | Some(Value::Null) => true,
        Some(kind) => kind.as_str() == Some("function"),
    }
}

fn decode_tool_call(call: &Value) -> Result<Item, Error> {
    if !is_function_type(call) {
        return Err(Error::unsupported(
            "chat tool call type is outside the published subset",
        ));
    }
    closed(call, "messages[].tool_calls[]", TOOL_CALL_KEYS)?;
    let call_id = call
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid("chat tool call carries no identifier"))?;
    let function = call
        .get("function")
        .ok_or_else(|| Error::invalid("chat tool call carries no function"))?;
    closed(
        function,
        "messages[].tool_calls[].function",
        TOOL_CALL_FUNCTION_KEYS,
    )?;
    let name = function
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid("chat tool call carries no name"))?;
    let raw = match function.get("arguments") {
        None | Some(Value::Null) => "",
        Some(Value::String(raw)) => raw,
        Some(_) => return Err(Error::invalid("chat tool call arguments are not text")),
    };
    let arguments = if raw.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(raw)
            .map_err(|_| Error::invalid("chat tool call arguments are not valid JSON"))?
    };
    Ok(Item::ToolCall(llm_core::ToolCall {
        call_id: call_id_of(call_id)?,
        name: tool_name_of(name)?,
        arguments,
    }))
}

fn decode_tools(tools: &Value) -> Result<Vec<ToolSpec>, Error> {
    let tools = tools
        .as_array()
        .ok_or_else(|| Error::invalid("chat request tools is not an array"))?;
    let mut out = Vec::with_capacity(tools.len());
    for tool in tools {
        if tool.get("type").and_then(Value::as_str) != Some("function") {
            return Err(Error::unsupported(
                "chat tool type is outside the published subset",
            ));
        }
        closed(tool, "tools[]", TOOL_KEYS)?;
        let function = tool
            .get("function")
            .ok_or_else(|| Error::invalid("chat tool declaration carries no function"))?;
        closed(function, "tools[].function", TOOL_FUNCTION_KEYS)?;
        match function.get("strict") {
            None | Some(Value::Null | Value::Bool(false)) => {}
            // The neutral tool has no strict mode. Accepting `true` would promise the client a
            // schema-conformant call that nothing downstream enforces.
            Some(Value::Bool(true)) => {
                return Err(Error::unsupported(
                    "chat strict function schemas are outside the published subset",
                ));
            }
            Some(_) => {
                return Err(Error::invalid(
                    "chat tool declaration strict is not a boolean",
                ));
            }
        }
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::invalid("chat tool declaration carries no name"))?;
        // A schema is required rather than invented: an omitted one would have to be guessed,
        // and the guess would be published to the model as the operator's declaration.
        let input_schema = function
            .get("parameters")
            .cloned()
            .ok_or_else(|| Error::invalid("chat tool declaration omits its parameters schema"))?;
        let description = match function.get("description") {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(description)) => description.clone(),
            Some(_) => {
                return Err(Error::invalid(
                    "chat tool declaration description is not text",
                ));
            }
        };
        out.push(ToolSpec {
            name: tool_name_of(name)?,
            description,
            input_schema,
        });
    }
    Ok(out)
}

fn decode_tool_choice(value: Option<&Value>) -> Result<ToolChoice, Error> {
    match value {
        None | Some(Value::Null) => Ok(ToolChoice::Auto),
        Some(Value::String(choice)) if choice == "auto" => Ok(ToolChoice::Auto),
        Some(Value::String(choice)) if choice == "required" => Ok(ToolChoice::Required),
        Some(choice @ Value::Object(_)) => {
            if !is_function_type(choice) {
                return Err(Error::unsupported(
                    "chat tool choice type is outside the published subset",
                ));
            }
            closed(choice, "tool_choice", TOOL_CHOICE_KEYS)?;
            let function = choice
                .get("function")
                .ok_or_else(|| Error::invalid("chat tool choice names no function"))?;
            closed(function, "tool_choice.function", TOOL_CHOICE_FUNCTION_KEYS)?;
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::invalid("chat tool choice names no function"))?;
            Ok(ToolChoice::Named(tool_name_of(name)?))
        }
        // `none` and `allowed_tools` have no neutral equivalent, so neither is accepted and
        // then quietly treated as `auto`.
        Some(_) => Err(Error::unsupported(
            "chat tool choice is outside the published subset",
        )),
    }
}

fn decode_output_limit(object: &Map<String, Value>) -> Result<Option<u64>, Error> {
    let current = whole(object.get("max_completion_tokens"), "max_completion_tokens")?;
    let deprecated = whole(object.get("max_tokens"), "max_tokens")?;
    match (current, deprecated) {
        (Some(current), Some(deprecated)) if current != deprecated => Err(Error::invalid(
            "chat request output limits contradict each other",
        )),
        (Some(limit), _) | (None, Some(limit)) => Ok(Some(limit)),
        (None, None) => Ok(None),
    }
}

fn content_text(content: Option<&Value>) -> Result<String, Error> {
    match content {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(text)) => Ok(text.clone()),
        Some(Value::Array(parts)) => {
            let mut text = String::new();
            for part in parts {
                if part.get("type").and_then(Value::as_str) != Some("text") {
                    return Err(Error::unsupported(
                        "chat content part type is outside the published subset",
                    ));
                }
                closed(part, "messages[].content[]", CONTENT_PART_KEYS)?;
                match part.get("text") {
                    None | Some(Value::Null) => {}
                    Some(Value::String(part)) => text.push_str(part),
                    Some(_) => return Err(Error::invalid("chat content part text is not text")),
                }
            }
            Ok(text)
        }
        Some(_) => Err(Error::invalid("chat message content is not text")),
    }
}

fn flag(value: Option<&Value>, field: &'static str) -> Result<bool, Error> {
    match value {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(flag)) => Ok(*flag),
        Some(_) => Err(Error::invalid(format!(
            "chat request field `{field}` is not a boolean"
        ))),
    }
}

fn optional_flag(value: Option<&Value>, field: &'static str) -> Result<Option<bool>, Error> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(flag)) => Ok(Some(*flag)),
        Some(_) => Err(Error::invalid(format!(
            "chat request field `{field}` is not a boolean"
        ))),
    }
}

fn text(value: Option<&Value>, field: &'static str) -> Result<Option<String>, Error> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(Error::invalid(format!(
            "chat request field `{field}` is not a string"
        ))),
    }
}

fn decode_verbosity(value: Option<&Value>) -> Result<Option<Verbosity>, Error> {
    Ok(match text(value, "verbosity")?.as_deref() {
        None => None,
        Some("low") => Some(Verbosity::Low),
        Some("medium") => Some(Verbosity::Medium),
        Some("high") => Some(Verbosity::High),
        // The value is the client's, so the diagnostic names the field and not the value.
        Some(_) => {
            return Err(Error::unsupported(
                "chat request verbosity is outside the published subset",
            ));
        }
    })
}

fn number(value: Option<&Value>, field: &'static str) -> Result<Option<f64>, Error> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(found) => found
            .as_f64()
            .map(Some)
            .ok_or_else(|| Error::invalid(format!("chat request field `{field}` is not a number"))),
    }
}

fn whole(value: Option<&Value>, field: &'static str) -> Result<Option<u64>, Error> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(found) => found.as_u64().map(Some).ok_or_else(|| {
            Error::invalid(format!("chat request field `{field}` is not a whole count"))
        }),
    }
}

fn call_id_of(value: &str) -> Result<CallId, Error> {
    CallId::new(value).map_err(|_| Error::invalid("chat call identifier is unusable"))
}

fn tool_name_of(value: &str) -> Result<ToolName, Error> {
    ToolName::new(value).map_err(|_| Error::invalid("chat tool name is unusable"))
}

/// Encode a neutral outcome as one non-streamed Chat Completions response.
///
/// `id` and `created` come from the caller: this library owns no clock and mints no identity.
///
/// # Errors
/// Refuses an outcome carrying content this wire cannot express, including caller-owned items,
/// opaque continuation state and a stop reason outside the published subset.
pub fn encode_ingress_completion(
    outcome: &TurnOutcome,
    id: &str,
    created: u64,
) -> Result<Value, Error> {
    let (content, calls) = split_items(&outcome.items)?;
    let mut message = Map::new();
    message.insert("role".to_owned(), json!("assistant"));
    message.insert(
        "content".to_owned(),
        content.map_or(Value::Null, Value::String),
    );
    if !calls.is_empty() {
        message.insert("tool_calls".to_owned(), Value::Array(calls));
    }
    let mut body = Map::new();
    body.insert("id".to_owned(), json!(id));
    body.insert("object".to_owned(), json!("chat.completion"));
    body.insert("created".to_owned(), json!(created));
    // An unreported upstream model leaves the field out. Substituting the configured alias
    // would turn an absent observation into a reported one.
    if let Some(model) = &outcome.observation.upstream_model {
        body.insert("model".to_owned(), json!(model.as_str()));
    }
    body.insert(
        "choices".to_owned(),
        json!([{
            "index": 0,
            "message": Value::Object(message),
            "finish_reason": finish_reason(&outcome.stop_reason)?,
        }]),
    );
    if let Some(usage) = outcome.observation.usage.as_ref().and_then(encode_usage) {
        body.insert("usage".to_owned(), usage);
    }
    Ok(Value::Object(body))
}

/// One call this encoder has announced on the wire, and the argument text it relayed for it.
#[derive(Debug, Clone)]
struct Announced {
    call_id: CallId,
    name: ToolName,
    arguments: String,
}

/// Encoder for one streamed answer. It holds the wire indices of the calls it has announced.
#[derive(Debug, Clone)]
pub struct IngressStream {
    id: String,
    created: u64,
    /// In announcement order; a call's position here is its wire index.
    announced: Vec<Announced>,
    /// Calls whose arguments arrived before any announcement. Their fragments were not relayed,
    /// so a later announcement cannot be honoured and they are emitted complete at close.
    unannounced: Vec<CallId>,
}

impl IngressStream {
    pub fn new(id: &str, created: u64) -> Self {
        Self {
            id: id.to_owned(),
            created,
            announced: Vec::new(),
            unannounced: Vec::new(),
        }
    }

    /// One chunk for one neutral delta, or `None` when this wire carries no such chunk.
    ///
    /// A call is named in the chunk that announces it, under the next wire index, exactly as
    /// the provider named it; its argument fragments follow under that index. A fragment for a
    /// call that was never announced returns `None`: a chunk opening a call must name it, and
    /// inventing a name is not an option. Such a call is emitted once, complete, by
    /// [`IngressStream::close`].
    pub fn chunk(&mut self, event: &StreamEvent) -> Option<Value> {
        let delta = match event {
            StreamEvent::TextDelta { text } => json!({"content": text}),
            StreamEvent::ReasoningDelta { text } => json!({"reasoning_content": text}),
            StreamEvent::ToolCallStarted { call_id, name } => {
                if self.position(call_id).is_some() || self.unannounced.contains(call_id) {
                    return None;
                }
                let index = self.announced.len();
                self.announced.push(Announced {
                    call_id: call_id.clone(),
                    name: name.clone(),
                    arguments: String::new(),
                });
                json!({"tool_calls": [{
                    "index": index,
                    "id": call_id.as_str(),
                    "type": "function",
                    "function": {"name": name.as_str(), "arguments": ""},
                }]})
            }
            StreamEvent::ToolArgumentsDelta { call_id, delta } => {
                let Some(index) = self.position(call_id) else {
                    if !self.unannounced.contains(call_id) {
                        self.unannounced.push(call_id.clone());
                    }
                    return None;
                };
                self.announced[index].arguments.push_str(delta);
                json!({"tool_calls": [{"index": index, "function": {"arguments": delta}}]})
            }
            StreamEvent::Warning { .. } => return None,
        };
        Some(self.envelope(
            json!([{"index": 0, "delta": delta, "finish_reason": null}]),
            None,
        ))
    }

    /// The terminal chunks: the calls the stream never announced, the finish reason, and the
    /// reported counters when the client asked for them.
    ///
    /// A call already announced is not repeated. When none of its arguments were relayed, its
    /// arguments are sent here under its own index, so the client never assembles an empty one.
    ///
    /// # Errors
    /// Refuses an outcome this wire cannot express, exactly as the non-streamed encoder does,
    /// and an outcome that contradicts what was already streamed: an announced call it does
    /// not carry, a different name, or arguments other than the relayed ones.
    pub fn close(&self, outcome: &TurnOutcome, include_usage: bool) -> Result<Vec<Value>, Error> {
        split_items(&outcome.items)?;
        let model = outcome
            .observation
            .upstream_model
            .as_ref()
            .map(|model| model.as_str().to_owned());
        let mut remaining = Vec::new();
        // Wire indices continue after the announced calls, one per call never announced.
        let mut next_index = self.announced.len();
        let mut seen = vec![false; self.announced.len()];
        for call in outcome.tool_calls() {
            let Some(index) = self.position(&call.call_id) else {
                remaining.push(json!({
                    "index": next_index,
                    "id": call.call_id.as_str(),
                    "type": "function",
                    "function": {"name": call.name.as_str(), "arguments": call.arguments.to_string()},
                }));
                next_index += 1;
                continue;
            };
            seen[index] = true;
            let announced = &self.announced[index];
            if announced.name != call.name {
                return Err(streamed_contradiction());
            }
            if announced.arguments.is_empty() {
                remaining.push(json!({
                    "index": index,
                    "function": {"arguments": call.arguments.to_string()},
                }));
            } else if relayed_arguments(&announced.arguments).as_ref() != Some(&call.arguments) {
                return Err(streamed_contradiction());
            }
        }
        if seen.contains(&false) {
            return Err(streamed_contradiction());
        }
        let mut chunks = Vec::new();
        if !remaining.is_empty() {
            chunks.push(self.envelope(
                json!([{"index": 0, "delta": {"tool_calls": remaining}, "finish_reason": null}]),
                model.clone(),
            ));
        }
        chunks.push(self.envelope(
            json!([{
                "index": 0,
                "delta": {},
                "finish_reason": finish_reason(&outcome.stop_reason)?,
            }]),
            model.clone(),
        ));
        if include_usage
            && let Some(usage) = outcome.observation.usage.as_ref().and_then(encode_usage)
        {
            let mut chunk = self.envelope(json!([]), model);
            if let Some(object) = chunk.as_object_mut() {
                object.insert("usage".to_owned(), usage);
            }
            chunks.push(chunk);
        }
        Ok(chunks)
    }

    fn position(&self, call_id: &CallId) -> Option<usize> {
        self.announced
            .iter()
            .position(|announced| &announced.call_id == call_id)
    }

    fn envelope(&self, choices: Value, model: Option<String>) -> Value {
        let mut chunk = Map::new();
        chunk.insert("id".to_owned(), json!(self.id));
        chunk.insert("object".to_owned(), json!("chat.completion.chunk"));
        chunk.insert("created".to_owned(), json!(self.created));
        if let Some(model) = model {
            chunk.insert("model".to_owned(), json!(model));
        }
        chunk.insert("choices".to_owned(), choices);
        Value::Object(chunk)
    }
}

/// The assistant text and the proposed calls an outcome carries, refusing anything this wire
/// cannot express rather than dropping it.
fn split_items(items: &[Item]) -> Result<(Option<String>, Vec<Value>), Error> {
    let mut content: Option<String> = None;
    let mut calls = Vec::new();
    for item in items {
        match item {
            Item::AssistantText { text } => match &mut content {
                Some(existing) => {
                    existing.push('\n');
                    existing.push_str(text);
                }
                slot => *slot = Some(text.clone()),
            },
            Item::ToolCall(call) => calls.push(json!({
                "id": call.call_id.as_str(),
                "type": "function",
                "function": {"name": call.name.as_str(), "arguments": call.arguments.to_string()},
            })),
            Item::Opaque { .. } => {
                return Err(Error::unsupported(
                    "chat completions carries no opaque continuation state",
                ));
            }
            Item::UnattributedOpaque { .. } => {
                return Err(Error::unsupported(Item::UNATTRIBUTED_REFUSAL));
            }
            Item::UserText { .. } | Item::ToolResult { .. } => {
                return Err(Error::protocol(
                    "model outcome contains caller-owned content",
                ));
            }
        }
    }
    Ok((content, calls))
}

/// The arguments a Chat client assembles from the relayed text, read by the one rule the
/// projection reads them by (`incoming::parse_arguments`): blank text is `{}`.
fn relayed_arguments(text: &str) -> Option<Value> {
    crate::incoming::parse_arguments(text).ok()
}

/// The client already assembled what was streamed; an outcome that disagrees with it cannot be
/// reconciled by a terminal chunk.
fn streamed_contradiction() -> Error {
    Error::protocol("model outcome contradicts the tool calls already streamed")
}

fn finish_reason(stop_reason: &StopReason) -> Result<Value, Error> {
    match stop_reason {
        StopReason::EndTurn => Ok(json!("stop")),
        StopReason::MaxOutputTokens => Ok(json!("length")),
        StopReason::ToolCalls => Ok(json!("tool_calls")),
        StopReason::Incomplete { reason } if reason == "content-filter" => {
            Ok(json!("content_filter"))
        }
        StopReason::Incomplete { .. } => Err(Error::unsupported(
            "chat completions cannot report this terminal reason",
        )),
    }
}

/// Only counters the upstream actually reported. An absent one is left out rather than
/// reported as zero, and a total is derived only from two known parts.
fn encode_usage(usage: &Usage) -> Option<Value> {
    let mut out = Map::new();
    if let Some(value) = usage.input_tokens {
        out.insert("prompt_tokens".to_owned(), json!(value));
    }
    if let Some(value) = usage.output_tokens {
        out.insert("completion_tokens".to_owned(), json!(value));
    }
    if let Some(total) = usage
        .input_tokens
        .zip(usage.output_tokens)
        .and_then(|(input, output)| input.checked_add(output))
    {
        out.insert("total_tokens".to_owned(), json!(total));
    }
    let mut prompt_details = Map::new();
    if let Some(value) = usage.cached_input_tokens {
        prompt_details.insert("cached_tokens".to_owned(), json!(value));
    }
    if let Some(value) = usage.cache_creation_input_tokens {
        // This wire has no cache-write counter. A named extension keeps the reported count
        // readable; dropping it would delete an observation the upstream actually made.
        prompt_details.insert("cache_creation_input_tokens".to_owned(), json!(value));
    }
    if !prompt_details.is_empty() {
        out.insert(
            "prompt_tokens_details".to_owned(),
            Value::Object(prompt_details),
        );
    }
    if let Some(value) = usage.reasoning_output_tokens {
        out.insert(
            "completion_tokens_details".to_owned(),
            json!({"reasoning_tokens": value}),
        );
    }
    if out.is_empty() {
        None
    } else {
        Some(Value::Object(out))
    }
}
