use crate::{absent, fields, optional, string};
use llm_core::{
    CallId, Error, Item, MAX_REQUEST_BYTES, MAX_TOOL_ARGUMENT_BYTES, Protocol, Provenance,
    Sampling, ToolCall, ToolChoice, ToolName, ToolSpec, TurnRequest, exceeds,
};
use llm_providers::Binding;
use serde_json::{Value, json};

#[derive(Debug)]
pub struct IngressRequest {
    pub request: TurnRequest,
    pub stream: bool,
}

fn name(name: &str) -> Result<ToolName, Error> {
    if name.len() > 128
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(Error::unsupported(
            "Messages tool name is outside the supported character or length bound",
        ));
    }
    ToolName::new(name).map_err(|_| Error::invalid("invalid Messages tool name"))
}

pub(crate) fn block(item: &Item) -> Result<Value, Error> {
    Ok(match item {
        Item::UserText { text } | Item::AssistantText { text } => {
            json!({"type":"text","text":text})
        }
        Item::ToolCall(call) => {
            name(call.name.as_str())?;
            if !call.arguments.is_object() {
                return Err(Error::unsupported("Messages tool input must be an object"));
            }
            json!({"type":"tool_use","id":call.call_id,"name":call.name,"input":call.arguments})
        }
        Item::ToolResult {
            call_id,
            output,
            failed,
        } => {
            json!({"type":"tool_result","tool_use_id":call_id,"content":output.as_str().map_or_else(|| output.to_string(), str::to_owned),"is_error":failed})
        }
        Item::Opaque { payload, .. } => {
            opaque(payload)?;
            payload.clone()
        }
        // `encode_request` refuses it in `validate_for` first; this arm keeps the refusal on
        // every path that reaches a block, not only on the one that validates.
        Item::UnattributedOpaque { .. } => {
            return Err(Error::unsupported(Item::UNATTRIBUTED_REFUSAL));
        }
    })
}
/// Content blocks this projection builds itself, and may therefore annotate.
///
/// A marker modifies the block it lands on. On a block carried through from the model — a
/// `thinking` block, whose signature covers the block as it was produced — that is a turn the
/// route rejects. An allowlist, so that a new opaque shape never becomes markable unnoticed.
const MARKABLE: [&str; 2] = ["text", "tool_result"];

fn ephemeral() -> Value {
    json!({"type":"ephemeral"})
}

/// Places the rolling prompt-cache breakpoint on the last markable block of the last message.
///
/// The caller replays the whole transcript every turn. A breakpoint on the tail makes each turn
/// write the prefix the next one reads back, so the conversation's growth is paid for once rather
/// than on every remaining turn. A tail with nothing markable carries no marker: a missing
/// breakpoint costs money, a modified opaque block costs the turn.
fn mark_rolling_breakpoint(messages: &mut [Value]) {
    let tail = messages
        .last_mut()
        .and_then(|message| message.get_mut("content"))
        .and_then(Value::as_array_mut)
        .and_then(|blocks| {
            blocks.iter_mut().rev().find(|block| {
                block
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| MARKABLE.contains(&kind))
            })
        })
        .and_then(Value::as_object_mut);
    if let Some(block) = tail {
        block.insert("cache_control".to_owned(), ephemeral());
    }
}

fn user(item: &Item) -> bool {
    matches!(item, Item::UserText { .. } | Item::ToolResult { .. })
}

/// Project a bound request. The binding's declared maximum supplies Messages' mandatory default.
/// # Errors
/// Refuses incompatible bindings, unsupported wire semantics and request bounds before I/O.
pub fn encode_request(request: &TurnRequest, binding: &Binding) -> Result<Vec<u8>, Error> {
    request.validate_for(binding.provenance(), binding.capabilities())?;
    if binding.provenance().protocol != Protocol::Messages {
        return Err(Error::unsupported("Messages requires a Messages binding"));
    }
    if request.items.first().is_none_or(|item| !user(item)) {
        return Err(Error::unsupported(
            "Messages requires an initial user message",
        ));
    }
    if request.sampling.temperature.is_some_and(|n| n > 1.0) {
        return Err(Error::unsupported("Messages temperature exceeds one"));
    }
    let mut messages: Vec<Value> = Vec::new();
    for item in &request.items {
        let role = if user(item) { "user" } else { "assistant" };
        if messages.last().is_none_or(|m| m["role"] != role) {
            messages.push(json!({"role":role,"content":[]}));
        }
        messages
            .last_mut()
            .and_then(|m| m["content"].as_array_mut())
            .ok_or_else(|| Error::protocol("Messages grouping failed"))?
            .push(block(item)?);
    }
    let mut tools = Vec::new();
    for tool in &request.tools {
        name(tool.name.as_str())?;
        if !tool.input_schema.is_object() {
            return Err(Error::unsupported("Messages tool schema must be an object"));
        }
        tools.push(json!({"name":tool.name,"description":tool.description,"input_schema":tool.input_schema}));
    }
    mark_rolling_breakpoint(&mut messages);
    let mut body = json!({"model":binding.upstream_model(),"max_tokens":request.max_output_tokens.unwrap_or(binding.capabilities().max_output_tokens),"messages":messages,"stream":true});
    // A block list rather than a string, so it can carry the breakpoint that caches the constant
    // head (tools, then system) of every turn. An empty instruction sends nothing to mark; the
    // rolling breakpoint below already covers the tools.
    if !request.instructions.is_empty() {
        body["system"] =
            json!([{"type":"text","text":request.instructions,"cache_control":ephemeral()}]);
    }
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    if let Some(n) = request.sampling.temperature {
        body["temperature"] = json!(n);
    }
    if let Some(n) = request.sampling.top_p {
        body["top_p"] = json!(n);
    }
    if let Some(effort) = &request.sampling.reasoning_effort {
        body["output_config"] = json!({"effort":effort});
    }
    match &request.tool_choice {
        ToolChoice::Auto => {}
        ToolChoice::Required => body["tool_choice"] = json!({"type":"any"}),
        ToolChoice::Named(n) => body["tool_choice"] = json!({"type":"tool","name":n}),
    }
    if exceeds(&body, MAX_REQUEST_BYTES) {
        return Err(Error::too_large("Messages encoded request exceeds bound"));
    }
    serde_json::to_vec(&body).map_err(|_| Error::invalid("Messages request encoding failed"))
}

/// Decode only the declared neutral subset. The wire's model remains the caller's routing name.
///
/// `origin` states which protocol this gateway is reading, and nothing more. It is deliberately
/// **not** used to attribute arriving opaque state: a request carries no evidence of what served
/// the reasoning inside it, so this decoder carries that state as [`Item::UnattributedOpaque`]
/// rather than binding it to the reader. [`encode_request`] refuses it by name until the caller
/// binds it with [`llm_core::TurnRequest::bind_unattributed`]; see `docs/messages.md`.
///
/// # Errors
/// Refuses unsupported fields/content rather than silently dropping provider semantics.
pub fn decode_request(bytes: &[u8], origin: &Provenance) -> Result<IngressRequest, Error> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(Error::too_large("Messages ingress exceeds bound"));
    }
    if origin.protocol != Protocol::Messages {
        return Err(Error::unsupported(
            "Messages ingress requires Messages origin",
        ));
    }
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| Error::invalid("Messages ingress is not JSON"))?;
    fields(
        &value,
        &[
            "model",
            "max_tokens",
            "system",
            "messages",
            "tools",
            "stream",
            "temperature",
            "top_p",
            "output_config",
            "tool_choice",
        ],
    )?;
    let mut request = TurnRequest::new(string(&value, "model")?, Vec::new());
    request.max_output_tokens = Some(
        value
            .get("max_tokens")
            .and_then(Value::as_u64)
            .filter(|n| *n != 0)
            .ok_or_else(|| Error::invalid("Messages max_tokens must be positive"))?,
    );
    request.instructions = instructions(&value)?;
    decode_messages(&value, &mut request)?;
    decode_tools(&value, &mut request)?;
    decode_sampling(&value, &mut request)?;
    decode_tool_choice(&value, &mut request)?;
    let stream = match optional(&value, "stream") {
        None => false,
        Some(Value::Bool(b)) => *b,
        _ => return Err(Error::invalid("Messages stream must be boolean")),
    };
    request.validate()?;
    Ok(IngressRequest { request, stream })
}
/// The system prompt, which this route carries either as text or as one text block.
fn instructions(value: &Value) -> Result<String, Error> {
    match optional(value, "system") {
        None => Ok(String::new()),
        Some(Value::String(text)) => Ok(text.clone()),
        Some(Value::Array(blocks)) if blocks.len() == 1 => {
            fields(&blocks[0], &["type", "text"])?;
            if string(&blocks[0], "type")? != "text" {
                return Err(Error::unsupported("Messages system block is not text"));
            }
            Ok(string(&blocks[0], "text")?.to_owned())
        }
        _ => Err(Error::unsupported(
            "Messages system shape is outside the declared subset",
        )),
    }
}

fn decode_messages(value: &Value, request: &mut TurnRequest) -> Result<(), Error> {
    let messages = value
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::invalid("Messages messages must be an array"))?;
    for message in messages {
        fields(message, &["role", "content"])?;
        let role = string(message, "role")?;
        if !matches!(role, "user" | "assistant") {
            return Err(Error::unsupported(
                "Messages role is outside the declared subset",
            ));
        }
        let content = message
            .get("content")
            .ok_or_else(|| Error::invalid("Messages content is missing"))?;
        if let Some(text) = content.as_str() {
            request.items.push(if role == "user" {
                Item::user(text)
            } else {
                Item::assistant(text)
            });
        } else {
            for block in content
                .as_array()
                .ok_or_else(|| Error::invalid("Messages content must be text or blocks"))?
            {
                request
                    .items
                    .push(decode_block(block, role == "user", None)?);
            }
        }
    }
    if request.items.first().is_none_or(|item| !user(item)) {
        return Err(Error::invalid("Messages requires initial user content"));
    }
    Ok(())
}

fn decode_tools(value: &Value, request: &mut TurnRequest) -> Result<(), Error> {
    let Some(tools) = optional(value, "tools") else {
        return Ok(());
    };
    for tool in tools
        .as_array()
        .ok_or_else(|| Error::invalid("Messages tools must be an array"))?
    {
        fields(tool, &["name", "description", "input_schema"])?;
        let schema = tool
            .get("input_schema")
            .filter(|schema| schema.is_object())
            .ok_or_else(|| Error::unsupported("Messages tool schema must be an object"))?;
        let description = match optional(tool, "description") {
            None => String::new(),
            Some(Value::String(text)) => text.clone(),
            _ => return Err(Error::invalid("Messages tool description must be text")),
        };
        request.tools.push(ToolSpec {
            name: name(string(tool, "name")?)?,
            description,
            input_schema: schema.clone(),
        });
    }
    Ok(())
}

fn decode_sampling(value: &Value, request: &mut TurnRequest) -> Result<(), Error> {
    request.sampling = Sampling {
        temperature: number(value, "temperature")?,
        top_p: number(value, "top_p")?,
        reasoning_effort: None,
    };
    if request.sampling.temperature.is_some_and(|n| n > 1.0) {
        return Err(Error::unsupported("Messages temperature exceeds one"));
    }
    if let Some(config) = optional(value, "output_config") {
        fields(config, &["effort"])?;
        request.sampling.reasoning_effort = Some(string(config, "effort")?.to_owned());
    }
    Ok(())
}

fn decode_tool_choice(value: &Value, request: &mut TurnRequest) -> Result<(), Error> {
    let Some(choice) = optional(value, "tool_choice") else {
        return Ok(());
    };
    fields(choice, &["type", "name"])?;
    request.tool_choice = match string(choice, "type")? {
        "auto" => {
            absent(choice, "name")?;
            ToolChoice::Auto
        }
        "any" => {
            absent(choice, "name")?;
            ToolChoice::Required
        }
        "tool" => ToolChoice::Named(name(string(choice, "name")?)?),
        _ => {
            return Err(Error::unsupported(
                "Messages tool choice is outside the declared subset",
            ));
        }
    };
    Ok(())
}

fn number(value: &Value, field: &str) -> Result<Option<f64>, Error> {
    optional(value, field)
        .map(|v| {
            v.as_f64()
                .ok_or_else(|| Error::invalid("Messages sampling must be numeric"))
        })
        .transpose()
}

pub(crate) fn opaque(value: &Value) -> Result<(), Error> {
    match string(value, "type")? {
        "thinking" => {
            fields(value, &["type", "thinking", "signature"])?;
            string(value, "thinking")?;
            if string(value, "signature")?.is_empty() {
                return Err(Error::protocol("Messages thinking signature is empty"));
            }
        }
        "redacted_thinking" => {
            fields(value, &["type", "data"])?;
            if string(value, "data")?.is_empty() {
                return Err(Error::protocol("Messages redacted thinking is empty"));
            }
        }
        _ => {
            return Err(Error::unsupported(
                "Messages opaque block is outside the declared subset",
            ));
        }
    }
    Ok(())
}

/// Decode one content block.
///
/// `attribution` is the binding that **served** this block, and is `Some` only where that is a
/// fact: a response this client read from its own bound endpoint. An arriving request carries no
/// binding, so ingress passes `None` and opaque state is carried there as
/// [`Item::UnattributedOpaque`]. Stamping the reading binding onto it would launder state that
/// egress refuses from any other binding.
pub(crate) fn decode_block(
    value: &Value,
    user: bool,
    attribution: Option<&Provenance>,
) -> Result<Item, Error> {
    match string(value, "type")? {
        "text" => {
            fields(value, &["type", "text", "citations"])?;
            if value
                .get("citations")
                .is_some_and(|v| !v.is_null() && v.as_array().is_none_or(|v| !v.is_empty()))
            {
                return Err(Error::unsupported(
                    "Messages citations are outside the declared subset",
                ));
            }
            let text = string(value, "text")?;
            Ok(if user {
                Item::user(text)
            } else {
                Item::assistant(text)
            })
        }
        "tool_use" if !user => {
            fields(value, &["type", "id", "name", "input", "caller"])?;
            absent(value, "caller")?;
            let arguments = value
                .get("input")
                .filter(|v| v.is_object())
                .ok_or_else(|| Error::protocol("Messages tool input must be an object"))?;
            if exceeds(arguments, MAX_TOOL_ARGUMENT_BYTES) {
                return Err(Error::too_large("Messages tool arguments exceed bound"));
            }
            Ok(Item::ToolCall(ToolCall {
                call_id: CallId::new(string(value, "id")?)
                    .map_err(|_| Error::protocol("invalid Messages tool call ID"))?,
                name: name(string(value, "name")?)?,
                arguments: arguments.clone(),
            }))
        }
        "tool_result" if user => {
            fields(value, &["type", "tool_use_id", "content", "is_error"])?;
            let failed = match optional(value, "is_error") {
                None => false,
                Some(Value::Bool(b)) => *b,
                _ => {
                    return Err(Error::protocol(
                        "Messages tool result is_error must be boolean",
                    ));
                }
            };
            Ok(Item::ToolResult {
                call_id: CallId::new(string(value, "tool_use_id")?)
                    .map_err(|_| Error::protocol("invalid Messages tool result ID"))?,
                output: json!(string(value, "content")?),
                failed,
            })
        }
        "thinking" | "redacted_thinking" if !user => {
            opaque(value)?;
            // Where nothing observed the serving binding, the block is carried unattributed and
            // is refused on egress until a caller binds it. It is never bound to the reader.
            Ok(match attribution {
                Some(provenance) => Item::Opaque {
                    provenance: provenance.clone(),
                    payload: value.clone(),
                },
                None => Item::UnattributedOpaque {
                    protocol: Protocol::Messages,
                    payload: value.clone(),
                },
            })
        }
        _ => Err(Error::unsupported(
            "Messages content is outside the declared subset",
        )),
    }
}
