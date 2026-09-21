//! Gateway ingress: a Chat Completions client request decoded onto the neutral subset, and a
//! neutral outcome encoded back onto the same wire.
//!
//! A request is read strictly. Every field outside the published subset is refused, because a
//! gateway that quietly ignores `response_format` or `n` answers a question it was not asked.
use llm_core::{
    CallId, Error, Item, Sampling, StopReason, StreamEvent, ToolChoice, ToolName, ToolSpec,
    TurnOutcome, TurnRequest, Usage,
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
];

/// Request fields this subset deliberately does not implement, named so a client learns which
/// one was refused. A field in neither list is refused too, with a diagnostic that quotes
/// nothing the client sent.
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
    "parallel_tool_calls",
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
/// Refuses any field, role, content part or tool choice outside the published subset,
/// contradictory output limits, instructions that do not precede the conversation, and every
/// malformed value the neutral vocabulary cannot hold.
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
        reasoning_effort: object
            .get("reasoning_effort")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
    };

    let stream = flag(object.get("stream"), "stream")?;
    let include_usage = match object.get("stream_options") {
        None | Some(Value::Null) => false,
        Some(Value::Object(options)) => {
            if options.keys().any(|key| key != "include_usage") {
                return Err(Error::unsupported(
                    "chat request stream_options carries an option outside the published subset",
                ));
            }
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
    if object
        .keys()
        .any(|key| !SUPPORTED_REQUEST_FIELDS.contains(&key.as_str()))
    {
        return Err(Error::unsupported(
            "chat request carries a field outside the published subset",
        ));
    }
    Ok(())
}

fn decode_messages(messages: &[Value], request: &mut TurnRequest) -> Result<(), Error> {
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::invalid("chat message carries no role"))?;
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
                if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
                    for call in calls {
                        request.items.push(decode_tool_call(call)?);
                    }
                }
            }
            "tool" => {
                let call_id = message
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::invalid("chat tool message names no call"))?;
                request.items.push(Item::ToolResult {
                    call_id: call_id_of(call_id)?,
                    // This wire carries the result as text. Keeping it a string is lossless;
                    // guessing that it was meant to be JSON would not be.
                    output: Value::String(content_text(message.get("content"))?),
                    // The wire has no place to say a call failed, so no result claims one did.
                    failed: false,
                });
            }
            _ => {
                return Err(Error::unsupported(
                    "chat message role is outside the published subset",
                ));
            }
        }
    }
    Ok(())
}

fn decode_tool_call(call: &Value) -> Result<Item, Error> {
    let call_id = call
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid("chat tool call carries no identifier"))?;
    let function = call
        .get("function")
        .ok_or_else(|| Error::invalid("chat tool call carries no function"))?;
    let name = function
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid("chat tool call carries no name"))?;
    let raw = function
        .get("arguments")
        .and_then(Value::as_str)
        .unwrap_or_default();
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
        let function = tool
            .get("function")
            .ok_or_else(|| Error::invalid("chat tool declaration carries no function"))?;
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
        out.push(ToolSpec {
            name: tool_name_of(name)?,
            description: function
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
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
        Some(Value::Object(choice)) => {
            let name = choice
                .get("function")
                .and_then(|function| function.get("name"))
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
                text.push_str(part.get("text").and_then(Value::as_str).unwrap_or_default());
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

/// Encoder for one streamed answer. It holds the wire indices of the calls it has announced.
#[derive(Debug, Clone)]
pub struct IngressStream {
    id: String,
    created: u64,
}

impl IngressStream {
    pub fn new(id: &str, created: u64) -> Self {
        Self {
            id: id.to_owned(),
            created,
        }
    }

    /// One chunk for one neutral delta, or `None` when this wire carries no such chunk.
    ///
    /// Tool arguments return `None`: a chunk announcing a call must name it, and the neutral
    /// stream vocabulary carries the call identifier without the tool name. The proposed calls
    /// are emitted once, complete, by [`IngressStream::close`], rather than streamed under an
    /// invented name.
    pub fn chunk(&self, event: &StreamEvent) -> Option<Value> {
        let delta = match event {
            StreamEvent::TextDelta { text } => json!({"content": text}),
            StreamEvent::ReasoningDelta { text } => json!({"reasoning_content": text}),
            StreamEvent::ToolArgumentsDelta { .. } | StreamEvent::Warning { .. } => return None,
        };
        Some(self.envelope(
            json!([{"index": 0, "delta": delta, "finish_reason": null}]),
            None,
        ))
    }

    /// The terminal chunks: the proposed calls, the finish reason, and the reported counters
    /// when the client asked for them.
    ///
    /// # Errors
    /// Refuses an outcome this wire cannot express, exactly as the non-streamed encoder does.
    pub fn close(&self, outcome: &TurnOutcome, include_usage: bool) -> Result<Vec<Value>, Error> {
        let (_, calls) = split_items(&outcome.items)?;
        let model = outcome
            .observation
            .upstream_model
            .as_ref()
            .map(|model| model.as_str().to_owned());
        let mut chunks = Vec::new();
        if !calls.is_empty() {
            let announced: Vec<Value> = calls
                .into_iter()
                .enumerate()
                .map(|(index, mut call)| {
                    if let Some(object) = call.as_object_mut() {
                        object.insert("index".to_owned(), json!(index));
                    }
                    call
                })
                .collect();
            chunks.push(self.envelope(
                json!([{"index": 0, "delta": {"tool_calls": announced}, "finish_reason": null}]),
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
            Item::UserText { .. } | Item::ToolResult { .. } => {
                return Err(Error::protocol(
                    "model outcome contains caller-owned content",
                ));
            }
        }
    }
    Ok((content, calls))
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
