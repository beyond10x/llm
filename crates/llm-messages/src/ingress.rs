//! Gateway answers: a neutral outcome, and the deltas that led to it, encoded back onto the
//! Messages wire.
//!
//! The decoder in this crate is the contract. Every block, terminal reason and usage report is
//! checked against the same reader `decode_message` and `StreamDecoder` use before it is sent,
//! so an answer this encoder emits is one this crate reads back as the same outcome, and one it
//! would read back as anything else is refused rather than approximated.
//!
//! Two limits of the streamed answer, each because `StreamEvent` carries less than this wire:
//!
//! - **The model travels only in `message_start`**, which precedes the outcome. A stream carries
//!   the model passed to `IngressStream::new`; an outcome model the stream never announced is not
//!   sent, and reads back unknown. `encode_ingress_message` carries the outcome's own model.
//! - **Content with no neutral event cannot sit between streamed blocks.** A `redacted_thinking`
//!   block emits no `StreamEvent`, so a relayed answer of thinking, redacted thinking and then
//!   text has already shown the text at the index the redacted block holds in the outcome.
//!   `close` refuses it as a contradiction and sends nothing; such content is carried only after
//!   every streamed block. Relaying it in place needs block boundaries in `StreamEvent`.

use crate::{codec::decode_block, decode::read_reason, usage::Snapshot};
use llm_core::{
    CallId, Error, ErrorCode, Id, Item, MAX_ITEMS, MAX_TOOL_ARGUMENT_BYTES, StopReason,
    StreamEvent, ToolName, TurnOutcome, Usage,
};
use serde_json::{Map, Value, json};

const CALLER_OWNED: &str = "Messages answer carries caller-owned content";
const UNREADABLE: &str = "Messages answer content would not read back as itself";
const CONTRADICTION: &str = "Messages answer contradicts what was already streamed";
const UNREPORTABLE: &str = "Messages cannot report this terminal reason";
const ENDED: &str = "Messages answer has already ended";
// Where an encoded block sits, as a refusal from the shared block reader names it.
const CONTENT: &str = "content[]";
const BLOCK: &str = "content_block_start.content_block";

/// Encode a neutral outcome as one non-streamed Messages response.
///
/// `id` comes from the caller: this library mints no identity. The upstream model is reported
/// only when the outcome observed one; a configured alias never stands in for it.
///
/// # Errors
/// Refuses an identifier the decoder cannot read, caller-owned items, unattributed or foreign
/// opaque state, content the shared block reader would read back as anything else, a terminal
/// reason it would read as another, and usage it would refuse.
pub fn encode_ingress_message(outcome: &TurnOutcome, id: &str) -> Result<Value, Error> {
    let id = response_id(id)?;
    let content = content(outcome)?;
    let reason = reason(&outcome.stop_reason)?;
    let usage = usage(outcome.observation.usage.as_ref())?;
    let mut message = header(&id, outcome.observation.upstream_model.as_ref());
    message.insert("content".to_owned(), Value::Array(content));
    message.insert("stop_reason".to_owned(), json!(reason));
    if let Some(usage) = usage {
        message.insert("usage".to_owned(), usage);
    }
    Ok(Value::Object(message))
}

/// What one streamed block has shown the caller so far.
#[derive(Debug, Clone)]
enum Shown {
    Text(String),
    /// Signed only at close: the signature is the outcome's, not a delta's.
    Thinking(String),
    Tool {
        call_id: CallId,
        name: ToolName,
        /// The argument text relayed under this block.
        arguments: String,
        /// A fragment arrived that could not be relayed: after the block stopped, or past the
        /// argument bound. What the caller assembled is then not the call.
        withheld: bool,
    },
}

#[derive(Debug, Clone)]
struct Block {
    shown: Shown,
    open: bool,
}

/// Encoder for one streamed answer.
///
/// Each value it returns is one event payload carrying its `type`; the SSE event name equals it
/// and the framing is the server's. `message_start` precedes the first event a chunk emits, or
/// is sent by `close` or `fail` when nothing was.
///
/// A new block stops the open one, except a `thinking` block: its signature is carried by the
/// outcome's opaque state, so it stays open and is signed and stopped by `close`. The decoder
/// assembles content by block index, so a thinking block stopped after the blocks that followed
/// it still reads back in its place.
#[derive(Debug, Clone)]
pub struct IngressStream {
    id: Id,
    /// The model announced in `message_start`, which precedes the outcome.
    model: Option<Id>,
    blocks: Vec<Block>,
    /// Calls whose fragments arrived with no announcement, or whose name the decoder would
    /// refuse when the block opens. Nothing of them was relayed; `close` sends them complete, and
    /// refuses an outcome that omits one.
    held: Vec<CallId>,
    /// Content arrived past the bound the decoder holds a stream to; `close` refuses.
    overflowed: bool,
    started: bool,
    ended: bool,
}

impl IngressStream {
    /// `model` is what `message_start` announces. That event precedes the outcome, so the
    /// upstream model is announced only when the caller already knows it; none is invented.
    ///
    /// The model travels only in `message_start`: a server passes here the model it will
    /// announce, for example the binding's upstream model name. With `None`, an outcome that
    /// does report a model still closes, and the caller reads the model back as unknown. A model
    /// announced here and contradicted by the outcome is refused by `close`.
    ///
    /// # Errors
    /// Refuses an identifier or model name the decoder cannot read, before anything is sent.
    pub fn new(id: &str, model: Option<&str>) -> Result<Self, Error> {
        Ok(Self {
            id: response_id(id)?,
            model: model.map(model_name).transpose()?,
            blocks: Vec::new(),
            held: Vec::new(),
            overflowed: false,
            started: false,
            ended: false,
        })
    }

    /// The events for one neutral delta; empty when this wire carries nothing for it.
    ///
    /// An empty delta and a warning carry nothing. A call announced twice is announced once. A
    /// fragment for a call never announced is held and its call sent complete by `close`.
    pub fn chunk(&mut self, event: &StreamEvent) -> Vec<Value> {
        if self.ended {
            return Vec::new();
        }
        let mut out = Vec::new();
        match event {
            StreamEvent::TextDelta { text } if !text.is_empty() => self.text(text, &mut out),
            StreamEvent::ReasoningDelta { text } if !text.is_empty() => {
                self.reasoning(text, &mut out);
            }
            StreamEvent::ToolCallStarted { call_id, name } => {
                self.announce(call_id, name, &mut out);
            }
            StreamEvent::ToolArgumentsDelta { call_id, delta } if !delta.is_empty() => {
                self.fragment(call_id, delta, &mut out);
            }
            _ => {}
        }
        if !out.is_empty() && !self.started {
            self.started = true;
            out.insert(0, self.start());
        }
        out
    }

    /// The terminal events: the signature and stop of each open block, the content the stream
    /// never showed, the terminal reason and the reported counters, and `message_stop`.
    ///
    /// The outcome's items must begin with exactly the blocks streamed, in order: the same text,
    /// the same reasoning, the same calls with the arguments relayed. Content the stream never
    /// showed may only follow them, because the decoder reads content in block order. Every call
    /// the stream reported is in the outcome: an announced one at its block, a held one among
    /// the content that follows. A refusal sends nothing and leaves the stream open for `fail`.
    ///
    /// # Errors
    /// Refuses everything `encode_ingress_message` refuses, an outcome that contradicts what was
    /// streamed or omits a call the stream reported, a model other than the one announced, and a
    /// stream that already ended.
    pub fn close(&mut self, outcome: &TurnOutcome) -> Result<Vec<Value>, Error> {
        if self.ended {
            return Err(Error::protocol(ENDED));
        }
        let content = content(outcome)?;
        let reason = reason(&outcome.stop_reason)?;
        let usage = usage(outcome.observation.usage.as_ref())?;
        if self.overflowed
            || outcome.items.len() < self.blocks.len()
            || (self.model.is_some() && self.model != outcome.observation.upstream_model)
            || self
                .held
                .iter()
                .any(|held| !outcome.tool_calls().any(|call| call.call_id == *held))
        {
            return Err(Error::protocol(CONTRADICTION));
        }
        let mut out = Vec::new();
        if !self.started {
            out.push(self.start());
        }
        for (index, (block, item)) in self.blocks.iter().zip(&outcome.items).enumerate() {
            out.extend(settle(index, block, item)?);
        }
        for (index, (item, value)) in outcome
            .items
            .iter()
            .zip(&content)
            .enumerate()
            .skip(self.blocks.len())
        {
            out.extend(complete(index, item, value));
        }
        let mut terminal = json!({"type":"message_delta","delta":{"stop_reason":reason}});
        if let Some(usage) = usage {
            terminal["usage"] = usage;
        }
        out.push(terminal);
        out.push(json!({"type":"message_stop"}));
        self.started = true;
        self.ended = true;
        Ok(out)
    }

    /// The `error` event that ends the stream where it stands, preceded by `message_start`
    /// when nothing was sent. Empty once the stream has ended.
    ///
    /// The error type is the one the decoder reads back as the same code. Three codes have no
    /// type of their own on this route and take the nearest the decoder reads: `protocol` and
    /// `cancelled` are this side's own failure (`api_error`, read as `transport`), and
    /// `unsupported` is a request outside the subset (`invalid_request_error`). The message is
    /// the error's own fixed diagnostic.
    pub fn fail(&mut self, error: &Error) -> Vec<Value> {
        if self.ended {
            return Vec::new();
        }
        let mut out = Vec::new();
        if !self.started {
            out.push(self.start());
        }
        out.push(
            json!({"type":"error","error":{"type":error_type(error.code),
            "message":error.message}}),
        );
        self.started = true;
        self.ended = true;
        out
    }

    fn start(&self) -> Value {
        let mut message = header(&self.id, self.model.as_ref());
        message.insert("content".to_owned(), json!([]));
        json!({"type":"message_start","message":Value::Object(message)})
    }

    /// Extends the open text block, or opens one.
    fn text(&mut self, text: &str, out: &mut Vec<Value>) {
        let index = match self.blocks.last() {
            Some(Block {
                shown: Shown::Text(_),
                open: true,
            }) => Some(self.blocks.len() - 1),
            _ => self.open(
                Shown::Text(String::new()),
                json!({"type":"text","text":""}),
                out,
            ),
        };
        if let Some(index) = index {
            if let Shown::Text(shown) = &mut self.blocks[index].shown {
                shown.push_str(text);
            }
            out.push(delta(index, json!({"type":"text_delta","text":text})));
        }
    }

    /// Extends the latest block when it is thinking, or opens one. It opens with an empty
    /// signature, which the decoder accepts and `close` signs.
    fn reasoning(&mut self, text: &str, out: &mut Vec<Value>) {
        let index = match self.blocks.last() {
            Some(Block {
                shown: Shown::Thinking(_),
                ..
            }) => Some(self.blocks.len() - 1),
            _ => self.open(
                Shown::Thinking(String::new()),
                json!({"type":"thinking","thinking":"","signature":""}),
                out,
            ),
        };
        if let Some(index) = index {
            if let Shown::Thinking(shown) = &mut self.blocks[index].shown {
                shown.push_str(text);
            }
            out.push(delta(
                index,
                json!({"type":"thinking_delta","thinking":text}),
            ));
        }
    }

    fn announce(&mut self, call_id: &CallId, name: &ToolName, out: &mut Vec<Value>) {
        if self.position(call_id).is_some() || self.held.contains(call_id) {
            return;
        }
        let opening = json!({"type":"tool_use","id":call_id.as_str(),
            "name":name.as_str(),"input":{}});
        // The decoder refuses a name it cannot answer under when the block opens, which would
        // end the caller's stream; such a call is held for `close` to refuse.
        if decode_block(&opening, false, None, BLOCK).is_err() {
            self.held.push(call_id.clone());
            return;
        }
        self.open(
            Shown::Tool {
                call_id: call_id.clone(),
                name: name.clone(),
                arguments: String::new(),
                withheld: false,
            },
            opening,
            out,
        );
    }

    /// Relays a fragment into its call's open block, within the decoder's argument bound.
    fn fragment(&mut self, call_id: &CallId, text: &str, out: &mut Vec<Value>) {
        let Some(index) = self.position(call_id) else {
            if !self.held.contains(call_id) {
                self.held.push(call_id.clone());
            }
            return;
        };
        let block = &mut self.blocks[index];
        if let Shown::Tool {
            arguments,
            withheld,
            ..
        } = &mut block.shown
        {
            if block.open
                && !*withheld
                && arguments.len().saturating_add(text.len()) <= MAX_TOOL_ARGUMENT_BYTES
            {
                arguments.push_str(text);
                out.push(delta(
                    index,
                    json!({"type":"input_json_delta","partial_json":text}),
                ));
            } else {
                *withheld = true;
            }
        }
    }

    fn position(&self, call_id: &CallId) -> Option<usize> {
        self.blocks.iter().position(|block| {
            matches!(&block.shown, Shown::Tool { call_id: announced, .. } if announced == call_id)
        })
    }

    /// Stops the open block unless it awaits its signature, then opens the next index. `None`
    /// when the decoder's content bound leaves no index to open.
    fn open(&mut self, shown: Shown, block: Value, out: &mut Vec<Value>) -> Option<usize> {
        if self.blocks.len() >= MAX_ITEMS {
            self.overflowed = true;
            return None;
        }
        for (index, open) in self.blocks.iter_mut().enumerate() {
            if open.open && !matches!(open.shown, Shown::Thinking(_)) {
                open.open = false;
                out.push(stop(index));
            }
        }
        let index = self.blocks.len();
        out.push(start(index, block));
        self.blocks.push(Block { shown, open: true });
        Some(index)
    }
}

/// The events that finish one streamed block against the outcome item at its index.
fn settle(index: usize, block: &Block, item: &Item) -> Result<Vec<Value>, Error> {
    let mut out = Vec::new();
    match (&block.shown, item) {
        (Shown::Text(shown), Item::AssistantText { text }) if shown == text => {}
        (Shown::Thinking(shown), Item::Opaque { payload, .. })
            if payload.get("thinking").and_then(Value::as_str) == Some(shown) =>
        {
            // `content` already read the payload back as a signed thinking block.
            let signature = payload.get("signature").cloned().unwrap_or_default();
            out.push(delta(
                index,
                json!({"type":"signature_delta","signature":signature}),
            ));
        }
        (
            Shown::Tool {
                call_id,
                name,
                arguments,
                withheld: false,
            },
            Item::ToolCall(call),
        ) if *call_id == call.call_id && *name == call.name => {
            if arguments.is_empty() && block.open {
                out.push(delta(
                    index,
                    json!({"type":"input_json_delta","partial_json":call.arguments.to_string()}),
                ));
            } else if relayed(arguments) != call.arguments {
                return Err(Error::protocol(CONTRADICTION));
            }
        }
        _ => return Err(Error::protocol(CONTRADICTION)),
    }
    if block.open {
        out.push(stop(index));
    }
    Ok(out)
}

/// What the decoder assembles from relayed argument text: none leaves the opening `{}`.
fn relayed(arguments: &str) -> Value {
    if arguments.is_empty() {
        return json!({});
    }
    serde_json::from_str(arguments).unwrap_or(Value::Null)
}

/// One block the stream never showed, sent whole at `index`.
fn complete(index: usize, item: &Item, value: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    match item {
        Item::AssistantText { text } => {
            out.push(start(index, json!({"type":"text","text":""})));
            if !text.is_empty() {
                out.push(delta(index, json!({"type":"text_delta","text":text})));
            }
        }
        Item::ToolCall(call) => {
            out.push(start(
                index,
                json!({"type":"tool_use","id":call.call_id.as_str(),
                    "name":call.name.as_str(),"input":{}}),
            ));
            out.push(delta(
                index,
                json!({"type":"input_json_delta","partial_json":call.arguments.to_string()}),
            ));
        }
        _ if value.get("type").and_then(Value::as_str) == Some("thinking") => {
            out.push(start(
                index,
                json!({"type":"thinking","thinking":"","signature":""}),
            ));
            if let Some(thinking) = value
                .get("thinking")
                .and_then(Value::as_str)
                .filter(|thinking| !thinking.is_empty())
            {
                out.push(delta(
                    index,
                    json!({"type":"thinking_delta","thinking":thinking}),
                ));
            }
            out.push(delta(
                index,
                json!({"type":"signature_delta",
                    "signature":value.get("signature").cloned().unwrap_or_default()}),
            ));
        }
        // Redacted thinking has no deltas: it opens whole.
        _ => out.push(start(index, value.clone())),
    }
    out.push(stop(index));
    out
}

fn start(index: usize, block: Value) -> Value {
    let mut event = json!({"type":"content_block_start","index":index});
    event["content_block"] = block;
    event
}

fn delta(index: usize, delta: Value) -> Value {
    let mut event = json!({"type":"content_block_delta","index":index});
    event["delta"] = delta;
    event
}

fn stop(index: usize) -> Value {
    json!({"type":"content_block_stop","index":index})
}

/// The fields a message carries about itself, as `message_start` and a complete response share.
fn header(id: &Id, model: Option<&Id>) -> Map<String, Value> {
    let mut message = Map::new();
    message.insert("id".to_owned(), json!(id.as_str()));
    message.insert("type".to_owned(), json!("message"));
    message.insert("role".to_owned(), json!("assistant"));
    if let Some(model) = model {
        message.insert("model".to_owned(), json!(model.as_str()));
    }
    message
}

fn response_id(id: &str) -> Result<Id, Error> {
    Id::new(id).map_err(|_| Error::invalid("invalid Messages response id"))
}

fn model_name(model: &str) -> Result<Id, Error> {
    Id::new(model).map_err(|_| Error::invalid("invalid Messages model name"))
}

/// Every item as the block it is sent as, each read back by the shared block reader under the
/// outcome's own binding and required to be itself.
fn content(outcome: &TurnOutcome) -> Result<Vec<Value>, Error> {
    if outcome.items.len() > MAX_ITEMS {
        return Err(Error::too_large("Messages content exceeds its bound"));
    }
    let binding = &outcome.observation.binding;
    let mut calls: Vec<&CallId> = Vec::new();
    let mut blocks = Vec::with_capacity(outcome.items.len());
    for item in &outcome.items {
        let value = match item {
            Item::AssistantText { text } => json!({"type":"text","text":text}),
            Item::ToolCall(call) => {
                if calls.contains(&&call.call_id) {
                    return Err(Error::protocol(CONTRADICTION));
                }
                calls.push(&call.call_id);
                json!({"type":"tool_use","id":call.call_id.as_str(),
                    "name":call.name.as_str(),"input":call.arguments})
            }
            Item::Opaque { payload, .. } => payload.clone(),
            Item::UnattributedOpaque { .. } => {
                return Err(Error::unsupported(Item::UNATTRIBUTED_REFUSAL));
            }
            Item::UserText { .. } | Item::ToolResult { .. } => {
                return Err(Error::protocol(CALLER_OWNED));
            }
        };
        if decode_block(&value, false, Some(binding), CONTENT)? != *item {
            return Err(Error::unsupported(UNREADABLE));
        }
        blocks.push(value);
    }
    Ok(blocks)
}

/// The terminal reason under the name the decoder reads back as the same reason.
fn reason(stop_reason: &StopReason) -> Result<&str, Error> {
    let name = match stop_reason {
        StopReason::EndTurn => "end_turn",
        StopReason::ToolCalls => "tool_use",
        StopReason::MaxOutputTokens => "max_tokens",
        StopReason::Incomplete { reason } => reason.as_str(),
    };
    if read_reason(name).ok().as_ref() != Some(stop_reason) {
        return Err(Error::unsupported(UNREPORTABLE));
    }
    Ok(name)
}

/// Only reported counters, in the route's disjoint shape, read back by the decoder's own usage
/// reader before they are sent.
///
/// The route reports fresh input apart from both cache counters, so the fresh count is sent only
/// when the inclusive total and both cache counters are known; otherwise it is left out, never
/// derived from a guess. An unknown counter is absent, never zero; no counter at all sends no
/// usage.
fn usage(usage: Option<&Usage>) -> Result<Option<Value>, Error> {
    let Some(usage) = usage else {
        return Ok(None);
    };
    usage.validate()?;
    let mut wire = Map::new();
    if let (Some(total), Some(read), Some(created)) = (
        usage.input_tokens,
        usage.cached_input_tokens,
        usage.cache_creation_input_tokens,
    ) {
        // `validate` bounds the two cache counters by the total, so this is exact.
        wire.insert(
            "input_tokens".to_owned(),
            json!(total.saturating_sub(read).saturating_sub(created)),
        );
    }
    for (name, count) in [
        ("output_tokens", usage.output_tokens),
        ("cache_read_input_tokens", usage.cached_input_tokens),
        (
            "cache_creation_input_tokens",
            usage.cache_creation_input_tokens,
        ),
    ] {
        if let Some(count) = count {
            wire.insert(name.to_owned(), json!(count));
        }
    }
    if let Some(reasoning) = usage.reasoning_output_tokens {
        wire.insert(
            "output_tokens_details".to_owned(),
            json!({"thinking_tokens":reasoning}),
        );
    }
    if wire.is_empty() {
        return Ok(None);
    }
    let wire = Value::Object(wire);
    Snapshot::default().update(&wire, "usage")?;
    Ok(Some(wire))
}

/// The error type the decoder reads back as `code`, or the nearest one it reads.
const fn error_type(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::InvalidRequest | ErrorCode::Unsupported => "invalid_request_error",
        ErrorCode::Transport | ErrorCode::Protocol | ErrorCode::Cancelled => "api_error",
        ErrorCode::Unauthorized => "authentication_error",
        ErrorCode::RateLimited => "rate_limit_error",
        ErrorCode::Refused => "not_found_error",
        ErrorCode::TooLarge => "request_too_large",
        ErrorCode::Deadline => "timeout_error",
        ErrorCode::Unavailable => "overloaded_error",
    }
}
