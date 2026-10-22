use crate::{absent, fields, optional, string};
use llm_core::{
    Advisory, AuthKind, CacheTtl, CallId, Capabilities, Error, Item, MAX_REQUEST_BYTES,
    MAX_TOOL_ARGUMENT_BYTES, PromptCache, Protocol, Provenance, Sampling, Thinking, ToolCall,
    ToolChoice, ToolName, ToolSpec, TurnRequest, Unforwarded, exceeds, wire_effort,
};
use llm_providers::Binding;
use serde_json::{Value, json};

/// The block a subscription token's `system` opens with (Harness
/// `harness-messages/src/project.rs:47`). Sent only for a `subscription-oauth` account.
pub const SUBSCRIPTION_CLIENT_PREAMBLE: &str =
    "You are Claude Code, Anthropic's official CLI for Claude.";

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
            opaque(payload, "messages[].content[]")?;
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

/// The breakpoint this projection places: `ephemeral`, with the lifetime `1h` when the turn's
/// prompt-cache hint asks for it. The default lifetime is written by leaving `ttl` out.
fn ephemeral(request: &TurnRequest) -> Value {
    match request.controls.hints.prompt_cache {
        Some(PromptCache {
            ttl: Some(CacheTtl::OneHour),
        }) => json!({"type":"ephemeral","ttl":"1h"}),
        _ => json!({"type":"ephemeral"}),
    }
}

/// Places the rolling prompt-cache breakpoint on the last markable block of the last message.
///
/// The caller replays the whole transcript every turn. A breakpoint on the tail makes each turn
/// write the prefix the next one reads back, so the conversation's growth is paid for once rather
/// than on every remaining turn. A tail with nothing markable carries no marker: a missing
/// breakpoint costs money, a modified opaque block costs the turn.
fn mark_rolling_breakpoint(messages: &mut [Value], mark: Value) {
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
        block.insert("cache_control".to_owned(), mark);
    }
}

fn user(item: &Item) -> bool {
    matches!(item, Item::UserText { .. } | Item::ToolResult { .. })
}

/// Project a bound request. The binding's declared maximum supplies Messages' mandatory default.
/// # Errors
/// Refuses incompatible bindings, unsupported wire semantics and request bounds before I/O.
pub fn encode_request(request: &TurnRequest, binding: &Binding) -> Result<Vec<u8>, Error> {
    let body = project(
        request,
        binding.provenance(),
        binding.capabilities(),
        binding.upstream_model(),
        binding.declaration().account.auth_kind == AuthKind::SubscriptionOauth,
    )?;
    serde_json::to_vec(&body).map_err(|_| Error::invalid("Messages request encoding failed"))
}

/// Project a request for a target that is not described by a [`Binding`]: the Messages body
/// [`encode_request`] sends, as a JSON object, for a caller that frames, authenticates and
/// sends it itself (Amazon Bedrock's `InvokeModel` carries this body under its own envelope).
///
/// `provenance` and `capabilities` are what the request is validated against, exactly as a
/// binding's are; `upstream_model` is written as `model`. The body carries `"stream": true` and
/// `model` as it does for a binding: a caller whose envelope names neither removes them. No
/// subscription-token preamble is written, since that belongs to a `subscription-oauth`
/// binding alone.
///
/// # Errors
/// Refuses a provenance whose protocol is not Messages, unsupported wire semantics and request
/// bounds, as [`encode_request`] does.
pub fn encode_body(
    request: &TurnRequest,
    provenance: &Provenance,
    capabilities: &Capabilities,
    upstream_model: &str,
) -> Result<Value, Error> {
    project(request, provenance, capabilities, upstream_model, false)
}

/// The one Messages encoder behind [`encode_request`] and [`encode_body`]. `subscription` opens
/// `system` with the client preamble a `subscription-oauth` account must send.
fn project(
    request: &TurnRequest,
    provenance: &Provenance,
    capabilities: &Capabilities,
    upstream_model: &str,
    subscription: bool,
) -> Result<Value, Error> {
    request.validate_for(provenance, capabilities)?;
    if provenance.protocol != Protocol::Messages {
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
    mark_rolling_breakpoint(&mut messages, ephemeral(request));
    let mut body = json!({"model":upstream_model,"max_tokens":request.max_output_tokens.unwrap_or(capabilities.max_output_tokens),"messages":messages,"stream":true});
    // A block list rather than a string, so it can carry the breakpoint that caches the constant
    // head (tools, then system) of every turn. An empty instruction sends nothing to mark; the
    // rolling breakpoint below already covers the tools.
    //
    // A subscription token is served only when `system` opens with the client preamble as its
    // own block, exactly and alone; any other shape is answered 429 without rate-limit headers.
    // The preamble stays unmarked so the instruction, the last block, keeps the breakpoint that
    // covers the whole constant head.
    let mut system = Vec::with_capacity(2);
    if subscription {
        system.push(json!({"type":"text","text":SUBSCRIPTION_CLIENT_PREAMBLE}));
    }
    if !request.instructions.is_empty() {
        system.push(
            json!({"type":"text","text":request.instructions,"cache_control":ephemeral(request)}),
        );
    }
    if !system.is_empty() {
        body["system"] = Value::Array(system);
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
    if let Some(effort) = wire_effort(Protocol::Messages, request)? {
        body["output_config"] = json!({"effort":effort});
    }
    match request.controls.thinking {
        None => {}
        Some(Thinking::Adaptive) => body["thinking"] = json!({"type":"adaptive"}),
        Some(Thinking::Enabled { budget_tokens }) => {
            body["thinking"] = json!({"type":"enabled","budget_tokens":budget_tokens});
        }
        Some(Thinking::Disabled) => body["thinking"] = json!({"type":"disabled"}),
    }
    let mut choice = match &request.tool_choice {
        ToolChoice::Auto => None,
        ToolChoice::Required => Some(json!({"type":"any"})),
        ToolChoice::Named(n) => Some(json!({"type":"tool","name":n})),
    };
    // This wire carries the parallel control inside the choice, so an automatic choice that
    // needs to carry it is written out.
    if let Some(parallel) = request.controls.parallel_tool_calls {
        choice.get_or_insert_with(|| json!({"type":"auto"}))["disable_parallel_tool_use"] =
            json!(!parallel);
    }
    if let Some(choice) = choice {
        body["tool_choice"] = choice;
    }
    if exceeds(&body, MAX_REQUEST_BYTES) {
        return Err(Error::too_large("Messages encoded request exceeds bound"));
    }
    Ok(body)
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
    let mut value: Value = serde_json::from_slice(bytes)
        .map_err(|_| Error::invalid("Messages ingress is not JSON"))?;
    fields(
        &value,
        "",
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
            "thinking",
            "metadata",
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
    let mut marks = Marks::default();
    request.instructions = instructions(&value, &mut marks)?;
    marks.strip_content(&mut value)?;
    decode_messages(&value, &mut request)?;
    decode_tools(&value, &mut request, &mut marks)?;
    decode_sampling(&value, &mut request)?;
    decode_tool_choice(&value, &mut request)?;
    request.controls.thinking = thinking(&value)?;
    if client_metadata(&value)? {
        request
            .controls
            .hints
            .unforwarded
            .insert(Unforwarded::ClientMetadata);
    }
    request.controls.hints.prompt_cache = marks.prompt_cache();
    let stream = match optional(&value, "stream") {
        None => false,
        Some(Value::Bool(b)) => *b,
        _ => return Err(Error::invalid("Messages stream must be boolean")),
    };
    request.validate()?;
    Ok(IngressRequest { request, stream })
}
/// The prompt-cache marks one arriving request carries, read into one hint.
///
/// A mark is `{"type":"ephemeral"}` with an optional `ttl` of `5m` or `1h`. `5m` is the route's
/// default, so a mark without a `ttl` and a `5m` mark have the same lifetime; only `1h` differs.
/// The neutral hint holds one lifetime, so marks of two lifetimes in one request are refused
/// rather than one of them silently winning.
#[derive(Default)]
struct Marks {
    /// `None` until a mark is read; then whether its lifetime is one hour.
    one_hour: Option<bool>,
}

impl Marks {
    /// Reads the `cache_control` of the object at `at`, when it has one.
    fn read(&mut self, object: &Value, at: &str) -> Result<(), Error> {
        let Some(mark) = optional(object, "cache_control") else {
            return Ok(());
        };
        if !mark.is_object() {
            return Err(Error::unsupported(
                "Messages cache mark is outside the declared subset",
            ));
        }
        fields(mark, &format!("{at}.cache_control"), &["type", "ttl"])?;
        if mark.get("type").and_then(Value::as_str) != Some("ephemeral") {
            return Err(Error::unsupported(
                "Messages cache mark type is outside the declared subset",
            ));
        }
        let one_hour = match optional(mark, "ttl").map(Value::as_str) {
            None | Some(Some("5m")) => false,
            Some(Some("1h")) => true,
            Some(_) => {
                return Err(Error::unsupported(
                    "Messages cache mark lifetime is outside the declared subset",
                ));
            }
        };
        if self.one_hour.is_some_and(|seen| seen != one_hour) {
            return Err(Error::unsupported(
                "Messages cache marks carry more than one lifetime",
            ));
        }
        self.one_hour = Some(one_hour);
        Ok(())
    }

    /// Reads and removes the mark of every content block that may carry one, so each block then
    /// decodes through the same reader a response block does. A block of any other type keeps
    /// its mark, and the block reader refuses it by its path; a shape that is not a list of
    /// messages holding blocks is left for the message reader to refuse.
    fn strip_content(&mut self, body: &mut Value) -> Result<(), Error> {
        let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) else {
            return Ok(());
        };
        for message in messages {
            let Some(blocks) = message.get_mut("content").and_then(Value::as_array_mut) else {
                continue;
            };
            for block in blocks {
                let markable = block
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| matches!(kind, "text" | "tool_use" | "tool_result"));
                if !markable || block.get("cache_control").is_none() {
                    continue;
                }
                self.read(block, "messages[].content[]")?;
                if let Some(object) = block.as_object_mut() {
                    object.remove("cache_control");
                }
            }
        }
        Ok(())
    }

    fn prompt_cache(&self) -> Option<PromptCache> {
        self.one_hour.map(|one_hour| PromptCache {
            ttl: one_hour.then_some(CacheTtl::OneHour),
        })
    }
}

/// The system prompt: text, or text blocks joined by a line feed as Chat joins system messages.
fn instructions(value: &Value, marks: &mut Marks) -> Result<String, Error> {
    match optional(value, "system") {
        None => Ok(String::new()),
        Some(Value::String(text)) => Ok(text.clone()),
        Some(Value::Array(blocks)) => {
            let mut texts = Vec::with_capacity(blocks.len());
            for block in blocks {
                if block.get("type").and_then(Value::as_str) != Some("text") {
                    return Err(Error::unsupported("Messages system block is not text"));
                }
                fields(block, "system[]", &["type", "text", "cache_control"])?;
                marks.read(block, "system[]")?;
                texts.push(string(block, "text")?);
            }
            Ok(texts.join("\n"))
        }
        _ => Err(Error::unsupported(
            "Messages system shape is outside the declared subset",
        )),
    }
}

/// The thinking control: `adaptive`, `enabled` with a positive `budget_tokens`, or `disabled`.
fn thinking(value: &Value) -> Result<Option<Thinking>, Error> {
    let Some(thinking) = optional(value, "thinking") else {
        return Ok(None);
    };
    let kind = thinking
        .as_object()
        .and_then(|object| object.get("type"))
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid("Messages thinking must name its type"))?;
    Ok(Some(match kind {
        "adaptive" => {
            fields(thinking, "thinking", &["type"])?;
            Thinking::Adaptive
        }
        "disabled" => {
            fields(thinking, "thinking", &["type"])?;
            Thinking::Disabled
        }
        "enabled" => {
            fields(thinking, "thinking", &["type", "budget_tokens"])?;
            Thinking::Enabled {
                budget_tokens: thinking
                    .get("budget_tokens")
                    .and_then(Value::as_u64)
                    .filter(|n| *n != 0)
                    .ok_or_else(|| {
                        Error::invalid("Messages thinking budget must be a positive integer")
                    })?,
            }
        }
        _ => {
            return Err(Error::unsupported(
                "Messages thinking type is outside the declared subset",
            ));
        }
    }))
}

/// Whether the request carries client telemetry: `metadata` holding only `user_id`, a string.
/// Its value has no model semantics, is never kept and is never re-emitted.
fn client_metadata(value: &Value) -> Result<bool, Error> {
    let Some(metadata) = optional(value, "metadata") else {
        return Ok(false);
    };
    fields(metadata, "metadata", &["user_id"])?;
    match optional(metadata, "user_id") {
        None => Ok(false),
        Some(Value::String(_)) => Ok(true),
        Some(_) => Err(Error::invalid("Messages metadata user_id must be text")),
    }
}

fn decode_messages(value: &Value, request: &mut TurnRequest) -> Result<(), Error> {
    let messages = value
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::invalid("Messages messages must be an array"))?;
    for message in messages {
        fields(message, "messages[]", &["role", "content"])?;
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
                request.items.push(decode_block(
                    block,
                    role == "user",
                    None,
                    "messages[].content[]",
                )?);
            }
        }
    }
    if request.items.first().is_none_or(|item| !user(item)) {
        return Err(Error::invalid("Messages requires initial user content"));
    }
    Ok(())
}

fn decode_tools(value: &Value, request: &mut TurnRequest, marks: &mut Marks) -> Result<(), Error> {
    let Some(tools) = optional(value, "tools") else {
        return Ok(());
    };
    for tool in tools
        .as_array()
        .ok_or_else(|| Error::invalid("Messages tools must be an array"))?
    {
        fields(
            tool,
            "tools[]",
            &["name", "description", "input_schema", "cache_control"],
        )?;
        marks.read(tool, "tools[]")?;
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
        fields(config, "output_config", &["effort"])?;
        request.sampling.reasoning_effort = Some(string(config, "effort")?.to_owned());
    }
    Ok(())
}

fn decode_tool_choice(value: &Value, request: &mut TurnRequest) -> Result<(), Error> {
    let Some(choice) = optional(value, "tool_choice") else {
        return Ok(());
    };
    fields(
        choice,
        "tool_choice",
        &["type", "name", "disable_parallel_tool_use"],
    )?;
    request.controls.parallel_tool_calls = match optional(choice, "disable_parallel_tool_use") {
        None => None,
        Some(Value::Bool(disable)) => Some(!disable),
        Some(_) => {
            return Err(Error::invalid(
                "Messages disable_parallel_tool_use must be boolean",
            ));
        }
    };
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

pub(crate) fn opaque(value: &Value, at: &str) -> Result<(), Error> {
    match string(value, "type")? {
        "thinking" => {
            fields(value, at, &["type", "thinking", "signature"])?;
            string(value, "thinking")?;
            if string(value, "signature")?.is_empty() {
                return Err(Error::protocol("Messages thinking signature is empty"));
            }
        }
        "redacted_thinking" => {
            fields(value, at, &["type", "data"])?;
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
/// egress refuses from any other binding. `at` is where the block sits, for a refusal to name.
pub(crate) fn decode_block(
    value: &Value,
    user: bool,
    attribution: Option<&Provenance>,
    at: &str,
) -> Result<Item, Error> {
    match string(value, "type")? {
        "text" => {
            fields(value, at, &["type", "text", "citations"])?;
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
            fields(
                value,
                at,
                &["type", "id", "name", "input", "caller", "toolset_name"],
            )?;
            // `{"type":"direct"}` is the route's documented default: the model called the tool
            // itself, which is what a neutral tool call is. Any other caller is a server tool this
            // projection never declares.
            if value.get("caller") != Some(&json!({"type": "direct"})) {
                absent(value, "caller")?;
            }
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
            fields(value, at, &["type", "tool_use_id", "content", "is_error"])?;
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
            opaque(value, at)?;
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

/// The advisory hints this projection sends: only the prompt cache, as the lifetime of the
/// breakpoints it places (the advisory table of `docs/design.md`).
const CARRIED: [Advisory; 1] = [Advisory::PromptCache];

/// The advisory hints `request` carries that this wire does not send, in the design's order.
///
/// A gateway can report them before it forwards anything; [`crate::MessagesClient`] emits them
/// as one `advisory-omitted` warning after a turn has succeeded.
pub fn omitted_advisories(request: &TurnRequest) -> Vec<Advisory> {
    request.controls.hints.omitted_by(&CARRIED)
}
