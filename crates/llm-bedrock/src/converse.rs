//! The Converse wire: `/converse-stream` and `/converse`.
//!
//! Converse is Bedrock's own request shape, the same for every model that supports it: the
//! conversation as `messages` of `user` and `assistant` content blocks, the instructions as
//! `system`, sampling as `inferenceConfig`, tools as `toolConfig`. A streamed answer is a
//! sequence of event-stream frames, each one event named by its `:event-type` header; a whole
//! answer is one JSON document.
//!
//! What a turn asks for and this wire cannot carry is refused by name before any I/O: a
//! reasoning effort, the parallel tool call control, a thinking control for a model that is not
//! Anthropic's, opaque state this wire did not produce. Advisory hints are never sent; a turn
//! that carried any says so in one `advisory-omitted` warning after it succeeded.

use crate::{client::BedrockTarget, errors, eventstream::Message};
use llm_core::{
    Advisory, CallId, Cancel, Dispatch, Error, ErrorCode, Item, MAX_ITEMS, MAX_REQUEST_BYTES,
    MAX_TOOL_ARGUMENT_BYTES, Protocol, Provenance, StopReason, StreamEvent, StreamSink, Thinking,
    ToolCall, ToolChoice, ToolName, TurnObservation, TurnOutcome, TurnRequest, Usage, exceeds,
};
use llm_http::within_tokio_runtime;
use serde_json::{Map, Value, json};
use std::{collections::BTreeMap, time::Instant};

/// The wire name an `advisory-omitted` warning carries.
pub(crate) const WIRE: &str = "Bedrock Converse";

/// The longest tool name and tool use id Converse accepts; both are `[A-Za-z0-9_-]`.
const MAX_WIRE_NAME_BYTES: usize = 64;
const MAX_STOP_REASON_BYTES: usize = 64;

const UNKNOWN_EVENT: &str = "unknown-stream-event";
const UNKNOWN_BLOCK: &str = "unknown-output-item";

/// Every field Converse's `TokenUsage` may carry. `totalTokens` is checked to be a count and
/// never read: the input total is the sum [`read_usage`] states.
const USAGE_FIELDS: [&str; 8] = [
    "inputTokens",
    "outputTokens",
    "totalTokens",
    "cacheReadInputTokens",
    "cacheWriteInputTokens",
    "cacheReadInputTokenCount",
    "cacheWriteInputTokenCount",
    "serverToolUsage",
];

/// Every field the `metadata` event may carry. Only `usage` is read: `metrics` (latency),
/// `trace`, `performanceConfig` and `serviceTier` describe the call, not the turn. `p` is
/// padding that hides the event's length.
const METADATA_FIELDS: [&str; 6] = [
    "usage",
    "metrics",
    "trace",
    "performanceConfig",
    "serviceTier",
    "p",
];

/// Every field a whole `/converse` answer may carry; the unread ones as for [`METADATA_FIELDS`].
const ANSWER_FIELDS: [&str; 8] = [
    "output",
    "stopReason",
    "usage",
    "metrics",
    "additionalModelResponseFields",
    "trace",
    "performanceConfig",
    "serviceTier",
];

/// Encodes the Converse body for `request` on `target`.
///
/// # Errors
/// Refuses, before any I/O, a target that is not `bedrock-converse`, what
/// [`TurnRequest::validate_for`] refuses (opaque state of another binding among it), and every
/// control and item this wire cannot carry.
pub(crate) fn encode_request(
    request: &TurnRequest,
    target: &BedrockTarget,
) -> Result<Vec<u8>, Error> {
    let body = encode_body(request, target)?;
    if exceeds(&body, MAX_REQUEST_BYTES) {
        return Err(Error::too_large(
            "Bedrock Converse encoded request exceeds bound",
        ));
    }
    serde_json::to_vec(&body)
        .map_err(|_| Error::invalid("Bedrock Converse request encoding failed"))
}

fn encode_body(request: &TurnRequest, target: &BedrockTarget) -> Result<Value, Error> {
    request.validate_for(&target.provenance, &target.capabilities)?;
    if target.provenance.protocol != Protocol::BedrockConverse {
        return Err(Error::unsupported(
            "Bedrock Converse requires a bedrock-converse target",
        ));
    }
    if request.items.first().is_none_or(|item| !from_caller(item)) {
        return Err(Error::unsupported(
            "Bedrock Converse requires an initial user message",
        ));
    }
    if request.sampling.reasoning_effort.is_some() {
        return Err(Error::unsupported(
            "Bedrock Converse carries no reasoning effort",
        ));
    }
    if request.controls.parallel_tool_calls.is_some() {
        return Err(Error::unsupported(
            "Bedrock Converse carries no parallel tool call control",
        ));
    }
    // Bedrock refuses a conversation holding tool use or tool result blocks without a tool
    // configuration; refused here, before I/O, by name.
    if request.tools.is_empty()
        && request
            .items
            .iter()
            .any(|item| matches!(item, Item::ToolCall(_) | Item::ToolResult { .. }))
    {
        return Err(Error::unsupported(
            "Bedrock Converse carries tool calls in a conversation only beside published tools",
        ));
    }
    let thinking = match request.controls.thinking {
        None => None,
        Some(_) if !anthropic_model(&target.model_id) => {
            return Err(Error::unsupported(
                "Bedrock Converse carries a thinking control only for an Anthropic model",
            ));
        }
        Some(thinking) => Some(thinking_field(thinking)),
    };
    let mut body = Map::new();
    body.insert(
        "messages".to_owned(),
        Value::Array(messages(&request.items)?),
    );
    if !request.instructions.is_empty() {
        body.insert("system".to_owned(), json!([{"text": request.instructions}]));
    }
    let config = inference_config(request);
    if !config.is_empty() {
        body.insert("inferenceConfig".to_owned(), Value::Object(config));
    }
    if !request.tools.is_empty() {
        body.insert("toolConfig".to_owned(), tool_config(request)?);
    }
    if let Some(thinking) = thinking {
        body.insert(
            "additionalModelRequestFields".to_owned(),
            json!({"thinking": thinking}),
        );
    }
    Ok(Value::Object(body))
}

/// Whether an item is the caller's: Converse sends tool results in a `user` message.
const fn from_caller(item: &Item) -> bool {
    matches!(item, Item::UserText { .. } | Item::ToolResult { .. })
}

/// The conversation as Converse messages, consecutive items of one role in one message.
fn messages(items: &[Item]) -> Result<Vec<Value>, Error> {
    let mut messages: Vec<Value> = Vec::new();
    for item in items {
        let role = if from_caller(item) {
            "user"
        } else {
            "assistant"
        };
        let block = content_block(item)?;
        match messages.last_mut() {
            Some(last) if last["role"] == role => last["content"]
                .as_array_mut()
                .ok_or_else(|| Error::protocol("Bedrock Converse message grouping failed"))?
                .push(block),
            _ => messages.push(json!({"role": role, "content": [block]})),
        }
    }
    Ok(messages)
}

fn content_block(item: &Item) -> Result<Value, Error> {
    Ok(match item {
        Item::UserText { text } | Item::AssistantText { text } => json!({"text": text}),
        // Carried or refused, never dropped: Converse has no block for refusal text.
        Item::AssistantRefusal { .. } => {
            return Err(Error::unsupported(
                "Bedrock Converse carries no assistant refusal text",
            ));
        }
        Item::ToolCall(call) => {
            wire_name(call.call_id.as_str())?;
            wire_name(call.name.as_str())?;
            if !call.arguments.is_object() {
                return Err(Error::unsupported(
                    "Bedrock Converse tool input must be an object",
                ));
            }
            json!({"toolUse": {"toolUseId": call.call_id, "name": call.name,
                "input": call.arguments}})
        }
        Item::ToolResult {
            call_id,
            output,
            failed,
        } => {
            wire_name(call_id.as_str())?;
            let text = output
                .as_str()
                .map_or_else(|| output.to_string(), str::to_owned);
            let mut result = json!({"toolUseId": call_id, "content": [{"text": text}]});
            // `success` is Converse's default, and several models refuse the field; a failure is
            // sent, and a model that cannot carry it refuses the turn.
            if *failed {
                result["status"] = json!("error");
            }
            json!({"toolResult": result})
        }
        Item::Opaque { payload, .. } => {
            reasoning_block(payload)?;
            payload.clone()
        }
        // `validate_for` refuses it first; this arm keeps the refusal on every path.
        Item::UnattributedOpaque { .. } => {
            return Err(Error::unsupported(Item::UNATTRIBUTED_REFUSAL));
        }
    })
}

/// Refuses opaque state other than a reasoning block in the two shapes this wire produces:
/// `{"reasoningContent": {"reasoningText": {"text", "signature"?}}}` and
/// `{"reasoningContent": {"redactedContent": "<base64>"}}`. Anything else came from somewhere
/// this wire cannot send it back to.
fn reasoning_block(payload: &Value) -> Result<(), Error> {
    let refused =
        || Error::unsupported("Bedrock Converse carries only its own reasoning blocks back");
    let content = payload
        .as_object()
        .filter(|fields| fields.len() == 1)
        .and_then(|fields| fields.get("reasoningContent"))
        .and_then(Value::as_object)
        .filter(|fields| fields.len() == 1)
        .ok_or_else(refused)?;
    if let Some(reasoning) = content.get("reasoningText") {
        let reasoning = reasoning.as_object().ok_or_else(refused)?;
        let shaped = reasoning.get("text").is_some_and(Value::is_string)
            && reasoning.get("signature").is_none_or(Value::is_string)
            && reasoning
                .keys()
                .all(|key| key == "text" || key == "signature");
        return if shaped { Ok(()) } else { Err(refused()) };
    }
    if content.get("redactedContent").is_some_and(Value::is_string) {
        Ok(())
    } else {
        Err(refused())
    }
}

/// A tool name or tool use id as Converse takes it: 1 to 64 of `[A-Za-z0-9_-]`.
fn wire_name(name: &str) -> Result<(), Error> {
    if name.is_empty()
        || name.len() > MAX_WIRE_NAME_BYTES
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(Error::unsupported(
            "Bedrock Converse tool name or call id is outside its character or length bound",
        ));
    }
    Ok(())
}

/// Only the settings the turn states; Converse applies the model's own default to the rest.
fn inference_config(request: &TurnRequest) -> Map<String, Value> {
    let mut config = Map::new();
    if let Some(limit) = request.max_output_tokens {
        config.insert("maxTokens".to_owned(), json!(limit));
    }
    if let Some(temperature) = request.sampling.temperature {
        config.insert("temperature".to_owned(), json!(temperature));
    }
    if let Some(top_p) = request.sampling.top_p {
        config.insert("topP".to_owned(), json!(top_p));
    }
    config
}

fn tool_config(request: &TurnRequest) -> Result<Value, Error> {
    let mut tools = Vec::with_capacity(request.tools.len());
    for tool in &request.tools {
        wire_name(tool.name.as_str())?;
        if !tool.input_schema.is_object() {
            return Err(Error::unsupported(
                "Bedrock Converse tool schema must be an object",
            ));
        }
        let mut spec = json!({"name": tool.name, "inputSchema": {"json": tool.input_schema}});
        if !tool.description.is_empty() {
            spec["description"] = json!(tool.description);
        }
        tools.push(json!({"toolSpec": spec}));
    }
    let mut config = json!({"tools": tools});
    match &request.tool_choice {
        ToolChoice::Auto => {}
        ToolChoice::Required => config["toolChoice"] = json!({"any": {}}),
        ToolChoice::Named(name) => config["toolChoice"] = json!({"tool": {"name": name}}),
    }
    Ok(config)
}

/// Whether `model_id` names an Anthropic model: a model id (`anthropic.…`) or a cross-Region
/// inference profile (`us.anthropic.…`, `global.anthropic.…`), alone or as the last segment of
/// an ARN. An application inference profile's ARN names no model and reads as not Anthropic.
fn anthropic_model(model_id: &str) -> bool {
    let name = model_id.rsplit('/').next().unwrap_or(model_id);
    name.split('.').take(2).any(|part| part == "anthropic")
}

/// The Anthropic `thinking` field, as the Messages wire writes it.
fn thinking_field(thinking: Thinking) -> Value {
    match thinking {
        Thinking::Adaptive => json!({"type": "adaptive"}),
        Thinking::Enabled { budget_tokens } => {
            json!({"type": "enabled", "budget_tokens": budget_tokens})
        }
        Thinking::Disabled => json!({"type": "disabled"}),
    }
}

/// The advisory hints `request` carries, every one of which this wire omits.
pub(crate) fn omitted_advisories(request: &TurnRequest) -> Vec<Advisory> {
    request.controls.hints.omitted_by(&[])
}

/// What the answer reported about the turn as a whole.
#[derive(Default)]
struct Header {
    stop_reason: Option<String>,
    usage: Option<Usage>,
}

impl Header {
    fn reason(&mut self, reported: Option<&Value>) -> Result<(), Error> {
        let reported = reported
            .and_then(Value::as_str)
            .ok_or_else(|| Error::protocol("Bedrock Converse stop names no reason"))?;
        if self.stop_reason.is_some() {
            return Err(Error::protocol(
                "Bedrock Converse reported two terminal reasons",
            ));
        }
        if reported.is_empty()
            || reported.len() > MAX_STOP_REASON_BYTES
            || !reported.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(Error::protocol(
                "Bedrock Converse terminal reason is not a bounded name",
            ));
        }
        self.stop_reason = Some(reported.to_owned());
        Ok(())
    }

    /// The last valid bound snapshot. Converse names no response id and no serving model in
    /// its body, so both stay unknown.
    fn observation(&self, target: &Provenance, final_usage: bool) -> TurnObservation {
        TurnObservation {
            binding: target.clone(),
            upstream_model: None,
            response_id: None,
            usage: self.usage.clone(),
            final_usage,
        }
    }

    /// Retains dispatch evidence and the last valid snapshot on a failure.
    fn attach(&self, error: Error, target: &Provenance) -> Error {
        if error.observation.is_some() {
            return error;
        }
        let dispatch = if error.dispatch == Dispatch::NotSent {
            Dispatch::Accepted
        } else {
            error.dispatch
        };
        error
            .with_dispatch(dispatch)
            .with_observation(self.observation(target, false))
    }

    fn outcome(
        &self,
        items: Vec<Item>,
        request: &TurnRequest,
        target: &Provenance,
    ) -> Result<TurnOutcome, Error> {
        let outcome = TurnOutcome {
            stop_reason: stop_reason(self.stop_reason.as_deref())?,
            items,
            observation: self.observation(target, true),
        };
        outcome.validate_for(request, target)?;
        // A call returned to a caller that set `controls.return_unpublished_calls` must be one it
        // can answer here: a name outside `wire_name`'s bound would make the next turn unsendable.
        // Every published name passed that bound before the request was sent, so this reaches
        // only an unpublished call (story:unpublished-tool-call-reaches-an-opted-in-caller).
        if request.controls.return_unpublished_calls
            && outcome
                .tool_calls()
                .any(|call| wire_name(call.name.as_str()).is_err())
        {
            return Err(Error::protocol(
                "Bedrock Converse model named a tool outside the bound a next turn carries",
            ));
        }
        Ok(outcome)
    }
}

/// A reason this projection does not model (`stop_sequence`, `guardrail_intervened`,
/// `content_filtered`, `model_context_window_exceeded`, …) is carried under its own name, never
/// as completion.
fn stop_reason(reported: Option<&str>) -> Result<StopReason, Error> {
    match reported {
        None => Err(Error::protocol(
            "Bedrock Converse answer names no terminal reason",
        )),
        Some("end_turn") => Ok(StopReason::EndTurn),
        Some("tool_use") => Ok(StopReason::ToolCalls),
        Some("max_tokens") => Ok(StopReason::MaxOutputTokens),
        Some(other) => Ok(StopReason::Incomplete {
            reason: other.to_owned(),
        }),
    }
}

/// Converse's `TokenUsage` as neutral usage; `None` when it reports no counter at all.
///
/// `inputTokens` counts only the input read neither from nor into the prompt cache. The
/// recorded prompt-caching answers show it: `ddtrace-bedrock_converse_stream_prompt_caching-0`
/// reports `inputTokens` 11, `cacheWriteInputTokens` 1028, `outputTokens` 236 and `totalTokens`
/// 1275 = 11 + 1028 + 236; `-1` reports 12, `cacheReadInputTokens` 1028, 250 and 1290 =
/// 12 + 1028 + 250. Neutral `input_tokens` is the whole input, the cache counts its subsets, so
/// it is the sum of the three.
///
/// The body [`encode_request`] writes never carries a `cachePoint`, so Bedrock neither reads
/// nor writes the prompt cache on a turn of this crate: a cache counter the answer leaves out
/// is a known 0, and the whole input is known whenever `inputTokens` is. A cache counter the
/// answer reports is added as reported. This holds for Converse only; an Anthropic `InvokeModel`
/// body carries `cache_control`, so an absent counter there stays unknown. `totalTokens` is not
/// used to fill in a counter. Each cache counter may arrive under either of its two names; two
/// that disagree are refused.
fn read_usage(usage: &Value) -> Result<Option<Usage>, Error> {
    fields(usage, &USAGE_FIELDS)?;
    let input = counter(usage, "inputTokens")?;
    let output = counter(usage, "outputTokens")?;
    counter(usage, "totalTokens")?;
    let read = cache_counter(usage, "cacheReadInputTokens", "cacheReadInputTokenCount")?;
    let written = cache_counter(usage, "cacheWriteInputTokens", "cacheWriteInputTokenCount")?;
    if let Some(tools) = usage.get("serverToolUsage").filter(|v| !v.is_null()) {
        let tools = tools.as_object().ok_or_else(|| {
            Error::protocol("Bedrock Converse server tool usage is not an object")
        })?;
        if tools.values().any(|count| count.as_u64() != Some(0)) {
            return Err(Error::unsupported(
                "Bedrock Converse server-tool charges are outside the declared subset",
            ));
        }
    }
    if input.is_none() && output.is_none() && read.is_none() && written.is_none() {
        return Ok(None);
    }
    let read = read.unwrap_or(0);
    let written = written.unwrap_or(0);
    let total = input
        .map(|input| {
            input
                .checked_add(read)
                .and_then(|sum| sum.checked_add(written))
                .ok_or_else(|| Error::protocol("Bedrock Converse input usage overflows"))
        })
        .transpose()?;
    let usage = Usage {
        input_tokens: total,
        output_tokens: output,
        cached_input_tokens: Some(read),
        cache_creation_input_tokens: Some(written),
        reasoning_output_tokens: None,
    };
    usage.validate()?;
    Ok(Some(usage))
}

fn counter(value: &Value, name: &str) -> Result<Option<u64>, Error> {
    match value.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(count) => count.as_u64().map(Some).ok_or_else(|| {
            Error::protocol("Bedrock Converse usage counter is not an unsigned integer")
        }),
    }
}

fn cache_counter(value: &Value, name: &str, alias: &str) -> Result<Option<u64>, Error> {
    match (counter(value, name)?, counter(value, alias)?) {
        (Some(one), Some(other)) if one != other => Err(Error::protocol(
            "Bedrock Converse reports one cache counter twice with two values",
        )),
        (one, other) => Ok(one.or(other)),
    }
}

/// Refuses an object carrying a field outside `allowed`, and a value that is not an object.
fn fields(value: &Value, allowed: &[&str]) -> Result<(), Error> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::protocol("Bedrock Converse value is not an object"))?;
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(Error::unsupported(
            "Bedrock Converse carries a field outside the declared subset",
        ));
    }
    Ok(())
}

fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str, Error> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::protocol("Bedrock Converse field is missing or not a string"))
}

/// The one member of a union object, such as a delta or a content block.
fn member(value: &Value) -> Result<(&str, &Value), Error> {
    let object = value
        .as_object()
        .filter(|object| object.len() == 1)
        .ok_or_else(|| {
            Error::protocol("Bedrock Converse union does not hold exactly one member")
        })?;
    object
        .iter()
        .next()
        .map(|(kind, inner)| (kind.as_str(), inner))
        .ok_or_else(|| Error::protocol("Bedrock Converse union does not hold exactly one member"))
}

/// A reasoning block being read: its text, its signature and, for a redacted block, the
/// encrypted content in its place.
#[derive(Default)]
struct Reasoning {
    text: String,
    signature: Option<String>,
    redacted: Option<String>,
}

impl Reasoning {
    /// One `reasoningContent` delta. Text is shown as it arrives; a signature and redacted
    /// content are never shown: neither is reasoning.
    fn apply(&mut self, delta: &Value) -> Result<Option<StreamEvent>, Error> {
        let (kind, fragment) = member(delta)?;
        let fragment = fragment
            .as_str()
            .ok_or_else(|| Error::protocol("Bedrock Converse reasoning delta is not a string"))?;
        match kind {
            "text" => {
                self.text.push_str(fragment);
                Ok((!fragment.is_empty()).then(|| StreamEvent::ReasoningDelta {
                    text: fragment.to_owned(),
                }))
            }
            "signature" => {
                self.signature
                    .get_or_insert_with(String::new)
                    .push_str(fragment);
                Ok(None)
            }
            "redactedContent" => {
                self.redacted
                    .get_or_insert_with(String::new)
                    .push_str(fragment);
                Ok(None)
            }
            _ => Err(Error::unsupported(
                "Bedrock Converse reasoning delta is outside the declared subset",
            )),
        }
    }

    /// A whole answer's `reasoningContent` block.
    fn whole(content: &Value) -> Result<Self, Error> {
        let (kind, inner) = member(content)?;
        match kind {
            "reasoningText" => {
                fields(inner, &["text", "signature"])?;
                Ok(Self {
                    text: string(inner, "text")?.to_owned(),
                    signature: match inner.get("signature") {
                        None | Some(Value::Null) => None,
                        Some(_) => Some(string(inner, "signature")?.to_owned()),
                    },
                    redacted: None,
                })
            }
            "redactedContent" => Ok(Self {
                redacted: Some(
                    inner
                        .as_str()
                        .ok_or_else(|| {
                            Error::protocol("Bedrock Converse redacted reasoning is not a string")
                        })?
                        .to_owned(),
                ),
                ..Self::default()
            }),
            _ => Err(Error::unsupported(
                "Bedrock Converse reasoning block is outside the declared subset",
            )),
        }
    }

    /// The block as opaque state bound to the binding that produced it, in the shape
    /// [`reasoning_block`] accepts back.
    fn into_item(self, target: &Provenance) -> Result<Item, Error> {
        let payload = if let Some(redacted) = self.redacted {
            if !self.text.is_empty() || self.signature.is_some() {
                return Err(Error::protocol(
                    "Bedrock Converse reasoning block is both redacted and readable",
                ));
            }
            json!({"reasoningContent": {"redactedContent": redacted}})
        } else {
            let mut reasoning = json!({"text": self.text});
            if let Some(signature) = self.signature {
                reasoning["signature"] = json!(signature);
            }
            json!({"reasoningContent": {"reasoningText": reasoning}})
        };
        Ok(Item::Opaque {
            provenance: target.clone(),
            payload,
        })
    }
}

/// One content block being assembled from its deltas.
enum Block {
    Text(String),
    Tool {
        call: CallId,
        name: ToolName,
        /// Accumulated input fragments, parsed once when the block stops: half an argument
        /// object must never reach a caller as a whole one.
        arguments: String,
        /// Whether this block announced its call. A second block under an id already announced
        /// relays nothing; the finish refuses the duplicate.
        announced: bool,
    },
    Reasoning(Reasoning),
    /// A block that opened outside the declared subset: its first event, kept whole.
    Unknown(Value),
}

impl Block {
    /// One delta of `kind` into this block, and the event that shows it, if any. Text and
    /// reasoning text are shown as they arrive; tool input is relayed only by the block that
    /// announced its call.
    fn apply(&mut self, kind: &str, value: &Value) -> Result<Option<StreamEvent>, Error> {
        match (self, kind) {
            (Self::Text(text), "text") => {
                let fragment = value.as_str().ok_or_else(|| {
                    Error::protocol("Bedrock Converse text delta is not a string")
                })?;
                text.push_str(fragment);
                Ok((!fragment.is_empty()).then(|| StreamEvent::TextDelta {
                    text: fragment.to_owned(),
                }))
            }
            (
                Self::Tool {
                    call,
                    arguments,
                    announced,
                    ..
                },
                "toolUse",
            ) => {
                fields(value, &["input"])?;
                let fragment = string(value, "input")?;
                if arguments.len().saturating_add(fragment.len()) > MAX_TOOL_ARGUMENT_BYTES {
                    return Err(Error::too_large(
                        "Bedrock Converse streamed tool input exceeds its bound",
                    ));
                }
                arguments.push_str(fragment);
                Ok(
                    (*announced && !fragment.is_empty()).then(|| StreamEvent::ToolArgumentsDelta {
                        call_id: call.clone(),
                        delta: fragment.to_owned(),
                    }),
                )
            }
            (Self::Reasoning(reasoning), "reasoningContent") => reasoning.apply(value),
            _ => Err(Error::protocol(
                "Bedrock Converse delta contradicts its block",
            )),
        }
    }
}

/// Where the message is in its lifecycle.
#[derive(Clone, Copy)]
enum Phase {
    Opening,
    Started,
    Stopped,
}

/// Accumulates one `/converse-stream` answer into a bound neutral outcome.
///
/// The stream is `messageStart`, content blocks (`contentBlockStart` for a tool use only; a text
/// or reasoning block begins with its first `contentBlockDelta`), each ended by
/// `contentBlockStop`, then `messageStop` with the stop reason and `metadata` with the usage.
/// Both terminal events are required: without `metadata` the turn's usage is not known to be
/// final. Text is shown as it arrives and the turn carries exactly that text. An event or a
/// block outside the subset is kept whole as opaque state, bound to the serving binding, and the
/// caller is told it was not interpreted.
pub(crate) struct StreamDecoder {
    target: Provenance,
    deadline: Option<Instant>,
    header: Header,
    phase: Phase,
    /// Whether the `metadata` event was read.
    metadata: bool,
    open: BTreeMap<u64, Block>,
    /// Keyed by block index: the order the route numbered its content is the order the caller
    /// was streamed it.
    items: BTreeMap<u64, Item>,
    /// The index after the highest that opened. A block index below it opened before.
    next_index: u64,
    announced: Vec<CallId>,
    /// Events outside the declared subset, each with the block index that would have opened
    /// next when it arrived.
    preserved: Vec<(u64, Item)>,
    /// Whether any event other than `messageStart` was read, shown to the caller or not.
    answered: bool,
}

impl StreamDecoder {
    pub(crate) fn new(target: Provenance) -> Self {
        Self {
            target,
            deadline: None,
            header: Header::default(),
            phase: Phase::Opening,
            metadata: false,
            open: BTreeMap::new(),
            items: BTreeMap::new(),
            next_index: 0,
            announced: Vec::new(),
            preserved: Vec::new(),
            answered: false,
        }
    }

    /// Bound every remaining wait in this decode by one absolute instant.
    #[must_use]
    pub(crate) fn with_deadline(mut self, deadline: Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }

    pub(crate) const fn is_complete(&self) -> bool {
        matches!(self.phase, Phase::Stopped) && self.metadata
    }

    /// Whether any event other than `messageStart` was read, shown to the caller or not. A
    /// failure after it is final: another attempt would answer, and bill, again.
    pub(crate) const fn has_answered(&self) -> bool {
        self.answered
    }

    /// Retains dispatch evidence and the last valid usage snapshot on a failure.
    pub(crate) fn attach(&self, error: Error) -> Error {
        self.header.attach(error, &self.target)
    }

    /// Apply one event-stream frame, emitting the neutral deltas a caller watches.
    ///
    /// An exception frame ends the turn with the class [`errors::exception`] gives it,
    /// retriable only while nothing has answered.
    pub(crate) async fn apply(
        &mut self,
        message: &Message,
        sink: &mut dyn StreamSink,
        cancel: &Cancel,
    ) -> Result<(), Error> {
        match message.string_header(":message-type") {
            Some("event") => {}
            Some("exception" | "error") => {
                return Err(errors::exception(message, self.answered));
            }
            _ => {
                return Err(Error::protocol(
                    "Bedrock frame carries no known message type",
                ));
            }
        }
        if self.is_complete() {
            return Err(Error::protocol(
                "Bedrock Converse event follows the terminal events",
            ));
        }
        let kind = message
            .string_header(":event-type")
            .ok_or_else(|| Error::protocol("Bedrock Converse event names no type"))?;
        let data: Value = serde_json::from_slice(&message.payload)
            .map_err(|_| Error::protocol("Bedrock Converse event payload is not JSON"))?;
        if !data.is_object() {
            return Err(Error::protocol(
                "Bedrock Converse event payload is not an object",
            ));
        }
        let applied = match kind {
            "messageStart" => self.start_message(&data),
            "contentBlockStart" => self.start_block(&data, sink, cancel).await,
            "contentBlockDelta" => self.apply_delta(&data, sink, cancel).await,
            "contentBlockStop" => self.stop_block(&data),
            "messageStop" => self.stop_message(&data),
            "metadata" => self.read_metadata(&data),
            // An event type added later is not a broken stream: kept whole, and said so.
            _ => {
                self.preserve(
                    &data,
                    "a stream event outside the pinned subset was preserved, not interpreted",
                    sink,
                    cancel,
                )
                .await
            }
        };
        self.answered |= kind != "messageStart";
        applied
    }

    fn started(&self) -> Result<(), Error> {
        match self.phase {
            Phase::Started => Ok(()),
            Phase::Opening => Err(Error::protocol(
                "Bedrock Converse event precedes its message start",
            )),
            Phase::Stopped => Err(Error::protocol(
                "Bedrock Converse content follows its message stop",
            )),
        }
    }

    /// Refuses one more item past the content bound, counting what is open and what was kept.
    fn bound(&self) -> Result<(), Error> {
        if self.items.len() + self.open.len() + self.preserved.len() >= MAX_ITEMS {
            return Err(Error::too_large(
                "Bedrock Converse content exceeds its bound",
            ));
        }
        Ok(())
    }

    /// Refuses a block at an index that is open or below one that already opened.
    fn ordered(&self, index: u64) -> Result<(), Error> {
        if self.open.contains_key(&index) || index < self.next_index {
            return Err(Error::protocol(
                "Bedrock Converse content block is out of order",
            ));
        }
        Ok(())
    }

    /// Refuses a new block out of order, or one past the content bound.
    fn fresh(&self, index: u64) -> Result<(), Error> {
        self.ordered(index)?;
        self.bound()
    }

    fn insert(&mut self, index: u64, block: Block) {
        self.next_index = index.saturating_add(1);
        self.open.insert(index, block);
    }

    /// Keeps one event outside the declared subset, bound to the serving binding, and says so.
    async fn preserve(
        &mut self,
        data: &Value,
        message: &str,
        sink: &mut dyn StreamSink,
        cancel: &Cancel,
    ) -> Result<(), Error> {
        self.bound()?;
        self.preserved.push((
            self.next_index,
            Item::Opaque {
                provenance: self.target.clone(),
                payload: data.clone(),
            },
        ));
        emit(sink, warning(UNKNOWN_EVENT, message), cancel, self.deadline).await
    }

    fn start_message(&mut self, data: &Value) -> Result<(), Error> {
        fields(data, &["role", "p"])?;
        if !matches!(self.phase, Phase::Opening) {
            return Err(Error::protocol("Bedrock Converse stream started twice"));
        }
        if data.get("role").and_then(Value::as_str) != Some("assistant") {
            return Err(Error::protocol(
                "Bedrock Converse message is not the assistant's",
            ));
        }
        self.phase = Phase::Started;
        Ok(())
    }

    async fn start_block(
        &mut self,
        data: &Value,
        sink: &mut dyn StreamSink,
        cancel: &Cancel,
    ) -> Result<(), Error> {
        self.started()?;
        fields(data, &["contentBlockIndex", "start", "p"])?;
        let index = index(data)?;
        self.fresh(index)?;
        let start = data
            .get("start")
            .ok_or_else(|| Error::protocol("Bedrock Converse block start carries no start"))?;
        let (kind, tool) = member(start)?;
        if kind != "toolUse" {
            // Kept whole at its index and never interpreted; the caller is told when it opens.
            self.insert(index, Block::Unknown(data.clone()));
            return emit(
                sink,
                warning(
                    UNKNOWN_BLOCK,
                    "a content block outside the pinned subset was preserved, not interpreted",
                ),
                cancel,
                self.deadline,
            )
            .await;
        }
        fields(tool, &["toolUseId", "name"])?;
        let call = CallId::new(string(tool, "toolUseId")?)
            .map_err(|_| Error::protocol("invalid Bedrock Converse tool use id"))?;
        let name = ToolName::new(string(tool, "name")?)
            .map_err(|_| Error::protocol("invalid Bedrock Converse tool name"))?;
        let announced = !self.announced.contains(&call);
        if announced {
            self.announced.push(call.clone());
        }
        self.insert(
            index,
            Block::Tool {
                call: call.clone(),
                name: name.clone(),
                arguments: String::new(),
                announced,
            },
        );
        if !announced {
            return Ok(());
        }
        emit(
            sink,
            StreamEvent::ToolCallStarted {
                call_id: call,
                name,
            },
            cancel,
            self.deadline,
        )
        .await
    }

    async fn apply_delta(
        &mut self,
        data: &Value,
        sink: &mut dyn StreamSink,
        cancel: &Cancel,
    ) -> Result<(), Error> {
        self.started()?;
        fields(data, &["contentBlockIndex", "delta", "p"])?;
        let index = index(data)?;
        let delta = data
            .get("delta")
            .ok_or_else(|| Error::protocol("Bedrock Converse delta carries no delta"))?;
        let (kind, value) = member(delta)?;
        if !self.open.contains_key(&index) {
            // A text or reasoning block opens with its first delta. A tool use block opens with
            // its start, so input for one that never started is out of order.
            let block = match kind {
                "text" => Block::Text(String::new()),
                "reasoningContent" => Block::Reasoning(Reasoning::default()),
                "toolUse" => {
                    return Err(Error::protocol(
                        "Bedrock Converse tool input names a block that never started",
                    ));
                }
                _ => Block::Unknown(data.clone()),
            };
            self.fresh(index)?;
            let unknown = matches!(block, Block::Unknown(_));
            self.insert(index, block);
            if unknown {
                return emit(
                    sink,
                    warning(
                        UNKNOWN_BLOCK,
                        "a content block outside the pinned subset was preserved, not interpreted",
                    ),
                    cancel,
                    self.deadline,
                )
                .await;
            }
        } else if matches!(self.open.get(&index), Some(Block::Unknown(_)))
            || !matches!(kind, "text" | "toolUse" | "reasoningContent")
        {
            // Nothing inside a block this subset does not model is interpreted, and a delta
            // kind it does not model is kept as its whole event.
            return self
                .preserve(
                    data,
                    "a content block delta outside the pinned subset was preserved, not interpreted",
                    sink,
                    cancel,
                )
                .await;
        }
        let event = self
            .open
            .get_mut(&index)
            .ok_or_else(|| Error::protocol("Bedrock Converse delta names no open block"))?
            .apply(kind, value)?;
        match event {
            Some(event) => emit(sink, event, cancel, self.deadline).await,
            None => Ok(()),
        }
    }

    fn stop_block(&mut self, data: &Value) -> Result<(), Error> {
        self.started()?;
        fields(data, &["contentBlockIndex", "p"])?;
        let index = index(data)?;
        let Some(block) = self.open.remove(&index) else {
            // A block that stops without having opened carried nothing, so it adds no item and
            // is not counted against the bound; one that already stopped cannot stop again.
            self.ordered(index)?;
            self.next_index = index.saturating_add(1);
            return Ok(());
        };
        let item = match block {
            Block::Text(text) => (!text.is_empty()).then_some(Item::AssistantText { text }),
            Block::Tool {
                call,
                name,
                arguments,
                ..
            } => Some(Item::ToolCall(ToolCall {
                call_id: call,
                name,
                arguments: tool_input(&arguments)?,
            })),
            Block::Reasoning(reasoning) => Some(reasoning.into_item(&self.target)?),
            Block::Unknown(value) => Some(Item::Opaque {
                provenance: self.target.clone(),
                payload: value,
            }),
        };
        if let Some(item) = item {
            self.items.insert(index, item);
        }
        Ok(())
    }

    fn stop_message(&mut self, data: &Value) -> Result<(), Error> {
        self.started()?;
        fields(data, &["stopReason", "additionalModelResponseFields", "p"])?;
        unrequested_fields(data)?;
        if !self.open.is_empty() {
            return Err(Error::protocol(
                "Bedrock Converse message stop leaves a content block open",
            ));
        }
        self.header.reason(data.get("stopReason"))?;
        self.phase = Phase::Stopped;
        Ok(())
    }

    fn read_metadata(&mut self, data: &Value) -> Result<(), Error> {
        fields(data, &METADATA_FIELDS)?;
        if matches!(self.phase, Phase::Opening) {
            return Err(Error::protocol(
                "Bedrock Converse event precedes its message start",
            ));
        }
        if self.metadata {
            return Err(Error::protocol(
                "Bedrock Converse reported its metadata twice",
            ));
        }
        if let Some(usage) = data.get("usage").filter(|usage| !usage.is_null()) {
            self.header.usage = read_usage(usage)?;
        }
        self.metadata = true;
        Ok(())
    }

    /// The terminal outcome.
    ///
    /// # Errors
    /// Refuses a stream that never reached both terminal events, retaining the last snapshot.
    pub(crate) fn finish(self, request: &TurnRequest) -> Result<TurnOutcome, Error> {
        let complete = self.is_complete();
        let Self {
            target,
            header,
            items,
            preserved,
            ..
        } = self;
        let decoded = || -> Result<TurnOutcome, Error> {
            if !complete {
                return Err(Error::protocol(
                    "the Bedrock Converse stream ended before its message stop and metadata",
                ));
            }
            header.outcome(merge(items, preserved), request, &target)
        };
        decoded().map_err(|error| header.attach(error, &target))
    }
}

/// `additionalModelResponseFields` answers `additionalModelResponseFieldPaths`, which this wire
/// never sends; one that arrives anyway is outside the subset.
fn unrequested_fields(data: &Value) -> Result<(), Error> {
    if data
        .get("additionalModelResponseFields")
        .is_some_and(|fields| !fields.is_null())
    {
        return Err(Error::unsupported(
            "Bedrock Converse additional model response fields are outside the declared subset",
        ));
    }
    Ok(())
}

/// A tool's whole input. A tool use that streamed no input took none.
fn tool_input(arguments: &str) -> Result<Value, Error> {
    if arguments.is_empty() {
        return Ok(json!({}));
    }
    let input: Value = serde_json::from_str(arguments)
        .map_err(|_| Error::protocol("Bedrock Converse streamed tool input is not JSON"))?;
    if !input.is_object() {
        return Err(Error::protocol(
            "Bedrock Converse tool input is not an object",
        ));
    }
    Ok(input)
}

/// Content in index order, each kept event after every block that had opened when it arrived.
fn merge(items: BTreeMap<u64, Item>, preserved: Vec<(u64, Item)>) -> Vec<Item> {
    let mut merged = Vec::with_capacity(items.len() + preserved.len());
    let mut preserved = preserved.into_iter().peekable();
    for (index, item) in items {
        while let Some((_, kept)) = preserved.next_if(|(next, _)| *next <= index) {
            merged.push(kept);
        }
        merged.push(item);
    }
    merged.extend(preserved.map(|(_, kept)| kept));
    merged
}

fn index(data: &Value) -> Result<u64, Error> {
    data.get("contentBlockIndex")
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::protocol("Bedrock Converse block index is missing or invalid"))
}

/// A fixed warning: the code and this crate's own text, never the producer's.
fn warning(code: &str, message: &str) -> StreamEvent {
    StreamEvent::Warning {
        code: code.to_owned(),
        message: message.to_owned(),
    }
}

/// Decodes a whole `/converse` answer, then shows the caller what it carries.
///
/// Nothing is shown for an answer that is refused: the events are emitted only once the answer
/// has decoded, in item order, so the turn's text is exactly the text its deltas showed.
///
/// # Errors
/// Refuses an answer outside the declared subset, after dispatch, keeping the usage it read;
/// and a sink or cancellation failure while the caller is told.
pub(crate) async fn decode_response(
    bytes: &[u8],
    request: &TurnRequest,
    provenance: &Provenance,
    sink: &mut dyn StreamSink,
    cancel: &Cancel,
) -> Result<TurnOutcome, Error> {
    let mut header = Header::default();
    let (outcome, shown) = read_answer(bytes, request, provenance, &mut header)
        .map_err(|error| header.attach(error, provenance))?;
    for event in shown {
        emit(sink, event, cancel, None)
            .await
            .map_err(|error| header.attach(error, provenance))?;
    }
    Ok(outcome)
}

fn read_answer(
    bytes: &[u8],
    request: &TurnRequest,
    target: &Provenance,
    header: &mut Header,
) -> Result<(TurnOutcome, Vec<StreamEvent>), Error> {
    let answer: Value = serde_json::from_slice(bytes)
        .map_err(|_| Error::protocol("Bedrock Converse answer is not JSON"))?;
    fields(&answer, &ANSWER_FIELDS)?;
    unrequested_fields(&answer)?;
    if let Some(usage) = answer.get("usage").filter(|usage| !usage.is_null()) {
        header.usage = read_usage(usage)?;
    }
    header.reason(answer.get("stopReason"))?;
    let output = answer
        .get("output")
        .ok_or_else(|| Error::protocol("Bedrock Converse answer carries no output"))?;
    fields(output, &["message"])?;
    let message = output
        .get("message")
        .ok_or_else(|| Error::protocol("Bedrock Converse output carries no message"))?;
    fields(message, &["role", "content"])?;
    if message.get("role").and_then(Value::as_str) != Some("assistant") {
        return Err(Error::protocol(
            "Bedrock Converse message is not the assistant's",
        ));
    }
    let content = message
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::protocol("Bedrock Converse content is not an array"))?;
    if content.len() > MAX_ITEMS {
        return Err(Error::too_large(
            "Bedrock Converse content exceeds its bound",
        ));
    }
    let mut items = Vec::with_capacity(content.len());
    let mut shown = Vec::new();
    for block in content {
        if let Some(item) = whole_block(block, target, &mut shown)? {
            items.push(item);
        }
    }
    Ok((header.outcome(items, request, target)?, shown))
}

/// One content block of a whole answer, and the events that show it as the stream would have.
fn whole_block(
    block: &Value,
    target: &Provenance,
    shown: &mut Vec<StreamEvent>,
) -> Result<Option<Item>, Error> {
    let (kind, inner) = member(block)?;
    match kind {
        "text" => {
            let text = inner
                .as_str()
                .ok_or_else(|| Error::protocol("Bedrock Converse text is not a string"))?;
            if text.is_empty() {
                return Ok(None);
            }
            shown.push(StreamEvent::TextDelta {
                text: text.to_owned(),
            });
            Ok(Some(Item::assistant(text)))
        }
        "toolUse" => {
            fields(inner, &["toolUseId", "name", "input"])?;
            let call = ToolCall {
                call_id: CallId::new(string(inner, "toolUseId")?)
                    .map_err(|_| Error::protocol("invalid Bedrock Converse tool use id"))?,
                name: ToolName::new(string(inner, "name")?)
                    .map_err(|_| Error::protocol("invalid Bedrock Converse tool name"))?,
                arguments: inner
                    .get("input")
                    .filter(|input| input.is_object())
                    .cloned()
                    .ok_or_else(|| {
                        Error::protocol("Bedrock Converse tool input is not an object")
                    })?,
            };
            shown.push(StreamEvent::ToolCallStarted {
                call_id: call.call_id.clone(),
                name: call.name.clone(),
            });
            shown.push(StreamEvent::ToolArgumentsDelta {
                call_id: call.call_id.clone(),
                delta: call.arguments.to_string(),
            });
            Ok(Some(Item::ToolCall(call)))
        }
        "reasoningContent" => {
            let reasoning = Reasoning::whole(inner)?;
            if !reasoning.text.is_empty() {
                shown.push(StreamEvent::ReasoningDelta {
                    text: reasoning.text.clone(),
                });
            }
            reasoning.into_item(target).map(Some)
        }
        _ => {
            shown.push(warning(
                UNKNOWN_BLOCK,
                "a content block outside the pinned subset was preserved, not interpreted",
            ));
            Ok(Some(Item::Opaque {
                provenance: target.clone(),
                payload: block.clone(),
            }))
        }
    }
}

/// Awaits the caller, bounded by both of the things that end a turn from outside it.
///
/// Backpressure is still awaited: this is a bound on the wait, not a reason to drop an event.
/// With a deadline each poll asks for the Tokio runtime first, so a wait polled outside one is
/// refused, after dispatch, before the timer is created.
pub(crate) async fn emit(
    sink: &mut dyn StreamSink,
    event: StreamEvent,
    cancel: &Cancel,
    deadline: Option<Instant>,
) -> Result<(), Error> {
    let wait = async move {
        tokio::select! {
            biased;
            () = cancel.cancelled() => Err(Error::cancelled().with_dispatch(Dispatch::Accepted)),
            () = expiry(deadline) => Err(Error::new(ErrorCode::Deadline, "the Bedrock turn deadline expired")
                .with_dispatch(Dispatch::Accepted)),
            result = sink.emit(event) => result,
        }
    };
    if deadline.is_none() {
        return wait.await;
    }
    within_tokio_runtime(wait)
        .await
        .map_err(|refusal| refusal.with_dispatch(Dispatch::Accepted))?
}

/// Completes at the turn's deadline, or never when the caller configured none.
async fn expiry(deadline: Option<Instant>) {
    match deadline {
        None => std::future::pending().await,
        Some(at) => tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await,
    }
}
