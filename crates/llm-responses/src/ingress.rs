//! The answer half of gateway ingress: a neutral turn back onto this wire.
//!
//! [`crate::ingest_request`] reads a caller's request onto the neutral turn; this module turns the
//! neutral [`StreamEvent`]s and [`TurnOutcome`] that answer it into the payloads a Responses caller
//! reads. It emits only what this crate's own decoder ([`crate::decode_stream`]) interprets, in the
//! order that decoder requires, so a caller reading the answer with it gets back the events and
//! the turn that were encoded. Where that is impossible the encoder refuses: a stop reason the
//! decoder would read back as another one, opaque state this wire cannot carry, content the
//! caller owns, or an outcome that contradicts what was already streamed.
//!
//! Each returned value is one server-sent-event payload carrying its `type`. Framing (`event:`
//! and `data:` lines) is the server's. The caller supplies the response identifier and the clock.

use llm_core::{
    CallId, Error, ErrorCode, Item, Protocol, StopReason, StreamEvent, ToolCall, ToolName,
    TurnObservation, TurnOutcome, Usage,
};
use serde_json::{Map, Value, json};

use crate::stream::is_safe_reason;

/// Encodes a neutral outcome as one non-streamed Responses response object.
///
/// The object is what a `response.completed` or `response.incomplete` payload carries as its
/// `response`; `status` alone already says which, so a reader needs no event name.
///
/// `id` and `created_at` come from the caller: this library owns no clock and mints no identity.
/// The object's `id` is the response identifier the outcome reports, or `id` when it reports none.
///
/// # Errors
/// Refuses what this wire cannot express rather than approximating it: caller-owned items,
/// unattributed opaque state ([`Item::UNATTRIBUTED_REFUSAL`]), opaque state of another binding or
/// other than reasoning, a stop reason the decoder would read back as another one, contradictory
/// counters, and a successful outcome without terminal evidence.
pub fn encode_ingress_response(
    outcome: &TurnOutcome,
    id: &str,
    created_at: u64,
) -> Result<Value, Error> {
    let terminal = check_outcome(outcome)?;
    let output = outcome
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| wire_item(item, &item_id(item, id, index as u64)))
        .collect();
    Ok(response_object(id, created_at, outcome, &terminal, output))
}

/// Encoder for one streamed answer.
///
/// It holds the output items it has opened, so a later delta continues the same item and the
/// close can finish each one under the identifier it was opened with. Every item it opens stays
/// open until [`IngressStream::close`]: the finished item is the outcome's, and the outcome is
/// not known before then.
#[derive(Debug, Clone)]
pub struct IngressStream {
    id: String,
    created_at: u64,
    /// The `sequence_number` of the next payload.
    sequence: u64,
    /// Whether `response.created` and `response.in_progress` have been emitted.
    opened: bool,
    /// Whether a terminal payload has been emitted. Nothing follows it.
    finished: bool,
    /// The `output_index` the next opened item takes.
    next_index: u64,
    /// The one assistant message every text delta continues.
    message: Option<OpenText>,
    /// The one reasoning item every reasoning delta continues.
    reasoning: Option<OpenText>,
    /// Announced calls, in announcement order, with the argument text relayed for each.
    calls: Vec<OpenCall>,
    /// Calls whose arguments arrived before any announcement. Their fragments were not relayed,
    /// because an item that opens a call must name it, and they are emitted whole at close.
    unannounced: Vec<CallId>,
}

#[derive(Debug, Clone)]
struct OpenText {
    output_index: u64,
    item_id: String,
    text: String,
}

#[derive(Debug, Clone)]
struct OpenCall {
    output_index: u64,
    item_id: String,
    call_id: CallId,
    name: ToolName,
    arguments: String,
}

impl IngressStream {
    pub fn new(id: &str, created_at: u64) -> Self {
        Self {
            id: id.to_owned(),
            created_at,
            sequence: 0,
            opened: false,
            finished: false,
            next_index: 0,
            message: None,
            reasoning: None,
            calls: Vec::new(),
            unannounced: Vec::new(),
        }
    }

    /// The payloads for one neutral delta, preceded by the opening payloads on the first one.
    ///
    /// One delta can need several payloads: the first text delta opens a message item and its
    /// content part before the text itself. A delta this wire has no payload for returns nothing:
    /// a warning, a repeated announcement, a fragment of a call never announced (it is emitted
    /// whole by [`IngressStream::close`]), and anything after the stream ended.
    pub fn chunk(&mut self, event: &StreamEvent) -> Vec<Value> {
        if self.finished {
            return Vec::new();
        }
        let mut payloads = Vec::new();
        match event {
            StreamEvent::TextDelta { text } => {
                let open = match &mut self.message {
                    Some(open) => open,
                    slot => {
                        let output_index = self.next_index;
                        self.next_index += 1;
                        let item_id = format!("msg_{}_{output_index}", self.id);
                        payloads.extend(open_message(output_index, &item_id));
                        slot.insert(OpenText {
                            output_index,
                            item_id,
                            text: String::new(),
                        })
                    }
                };
                open.text.push_str(text);
                payloads.push(text_delta(open.output_index, &open.item_id, text));
            }
            StreamEvent::ReasoningDelta { text } => {
                let open = match &mut self.reasoning {
                    Some(open) => open,
                    slot => {
                        let output_index = self.next_index;
                        self.next_index += 1;
                        let item_id = format!("rs_{}_{output_index}", self.id);
                        payloads.extend(open_reasoning(output_index, &item_id));
                        slot.insert(OpenText {
                            output_index,
                            item_id,
                            text: String::new(),
                        })
                    }
                };
                open.text.push_str(text);
                payloads.push(json!({
                    "type": "response.reasoning_summary_text.delta",
                    "item_id": open.item_id,
                    "output_index": open.output_index,
                    "summary_index": 0,
                    "delta": text,
                }));
            }
            StreamEvent::ToolCallStarted { call_id, name } => {
                if self.position(call_id).is_some() || self.unannounced.contains(call_id) {
                    return Vec::new();
                }
                let output_index = self.next_index;
                self.next_index += 1;
                let item_id = format!("fc_{}_{output_index}", self.id);
                payloads.push(json!({
                    "type": "response.output_item.added",
                    "output_index": output_index,
                    "item": call_item(&item_id, call_id, name, "", "in_progress"),
                }));
                self.calls.push(OpenCall {
                    output_index,
                    item_id,
                    call_id: call_id.clone(),
                    name: name.clone(),
                    arguments: String::new(),
                });
            }
            StreamEvent::ToolArgumentsDelta { call_id, delta } => {
                let Some(index) = self.position(call_id) else {
                    if !self.unannounced.contains(call_id) {
                        self.unannounced.push(call_id.clone());
                    }
                    return Vec::new();
                };
                let open = &mut self.calls[index];
                open.arguments.push_str(delta);
                payloads.push(arguments_delta(open.output_index, &open.item_id, delta));
            }
            StreamEvent::Warning { .. } => return Vec::new(),
        }
        self.number(payloads)
    }

    /// The payloads that finish the answer: each output item's closing payloads, in the
    /// outcome's order, then exactly one `response.completed` or `response.incomplete` carrying
    /// the whole response object. When nothing was streamed, the opening payloads come first.
    ///
    /// The streamed message is finished as the outcome's first assistant message, the streamed
    /// reasoning as its first reasoning item, and each announced call as the outcome's call under
    /// the same identifier. What the outcome carries beyond that is emitted whole here: an item
    /// opened, filled and finished, so a reader sees it as it would any other.
    ///
    /// A refusal emits nothing and leaves the stream open, so the server can still end it with
    /// [`IngressStream::fail`].
    ///
    /// # Errors
    /// Refuses everything [`encode_ingress_response`] refuses; an outcome that contradicts what
    /// was already streamed (an announced call it does not carry, carries under another name, or
    /// with arguments other than the relayed ones; shown text it carries no message for); and a
    /// stream that has already ended.
    pub fn close(&mut self, outcome: &TurnOutcome) -> Result<Vec<Value>, Error> {
        if self.finished {
            return Err(Error::protocol("the streamed response has already ended"));
        }
        let terminal = check_outcome(outcome)?;
        self.check_streamed(outcome)?;

        let mut payloads = Vec::new();
        let mut output = Vec::with_capacity(outcome.items.len());
        let mut next_index = self.next_index;
        let mut fresh = |kind: &str| {
            let output_index = next_index;
            next_index += 1;
            (output_index, format!("{kind}_{}_{output_index}", self.id))
        };
        let mut message = self.message.as_ref();
        let mut reasoning = self.reasoning.as_ref();
        let mut finished_calls = vec![false; self.calls.len()];
        for item in &outcome.items {
            match item {
                Item::AssistantText { text } => {
                    let (output_index, item_id) = if let Some(open) = message.take() {
                        (open.output_index, open.item_id.clone())
                    } else {
                        let (output_index, item_id) = fresh("msg");
                        payloads.extend(open_message(output_index, &item_id));
                        if !text.is_empty() {
                            payloads.push(text_delta(output_index, &item_id, text));
                        }
                        (output_index, item_id)
                    };
                    let item = message_item(&item_id, text);
                    payloads.extend(finish_message(output_index, &item_id, text, &item));
                    output.push(item);
                }
                Item::Opaque { payload, .. } => {
                    let output_index = if let Some(open) = reasoning.take() {
                        payloads.extend(finish_reasoning_text(open));
                        open.output_index
                    } else {
                        let (output_index, _) = fresh("rs");
                        payloads.push(json!({
                            "type": "response.output_item.added",
                            "output_index": output_index,
                            "item": payload,
                        }));
                        output_index
                    };
                    // Verbatim: the decoder binds exactly this item, and a caller replays it.
                    payloads.push(item_done(output_index, payload));
                    output.push(payload.clone());
                }
                Item::ToolCall(call) => {
                    let announced = self
                        .position(&call.call_id)
                        .filter(|&index| !finished_calls[index]);
                    let (output_index, item_id, relayed) = if let Some(index) = announced {
                        finished_calls[index] = true;
                        let open = &self.calls[index];
                        (
                            open.output_index,
                            open.item_id.clone(),
                            open.arguments.clone(),
                        )
                    } else {
                        let (output_index, item_id) = fresh("fc");
                        payloads.push(json!({
                            "type": "response.output_item.added",
                            "output_index": output_index,
                            "item": call_item(&item_id, &call.call_id, &call.name, "", "in_progress"),
                        }));
                        (output_index, item_id, String::new())
                    };
                    output.push(finish_call(
                        &mut payloads,
                        output_index,
                        &item_id,
                        relayed,
                        call,
                    ));
                }
                // `check_outcome` refused every other item.
                Item::UserText { .. }
                | Item::ToolResult { .. }
                | Item::UnattributedOpaque { .. } => {}
            }
        }
        // Reasoning the caller was shown with no state to carry it ends its part; the item itself
        // is not finished, because a finished item is state the decoder would add to the turn.
        if let Some(open) = reasoning {
            payloads.splice(0..0, finish_reasoning_text(open));
        }
        let body = response_object(&self.id, self.created_at, outcome, &terminal, output);
        payloads.push(json!({"type": terminal.event, "response": body}));

        let mut opening = self.open();
        opening.extend(payloads);
        self.next_index = next_index;
        self.finished = true;
        Ok(self.number(opening))
    }

    /// The payloads that end the answer as failed: one `response.failed`, preceded by the opening
    /// payloads when nothing was streamed. Nothing after it, and nothing after the stream ended.
    ///
    /// The failure's code is the one this crate's decoder classifies back into the error's own
    /// code where the wire has one: `server_is_overloaded` for a retriable `unavailable`,
    /// `server_error` for any other, `rate_limit_exceeded` for `rate-limited`. Every other code is
    /// written as its own name, which the decoder reads as `refused`. The diagnostic is a fixed
    /// string: the error's message is never relayed. The counters, model and response identifier
    /// the error's observation reports are carried, an invalid usage report excepted.
    pub fn fail(&mut self, error: &Error) -> Vec<Value> {
        if self.finished {
            return Vec::new();
        }
        let (code, message) = failure(error);
        let mut body = Map::new();
        let observation = error.observation.as_deref();
        body.insert(
            "id".to_owned(),
            json!(
                observation
                    .and_then(|observation| observation.response_id.as_ref())
                    .map_or(self.id.as_str(), |id| id.as_str())
            ),
        );
        body.insert("object".to_owned(), json!("response"));
        body.insert("created_at".to_owned(), json!(self.created_at));
        body.insert("status".to_owned(), json!("failed"));
        if let Some(model) = observation.and_then(|observation| observation.upstream_model.as_ref())
        {
            body.insert("model".to_owned(), json!(model.as_str()));
        }
        body.insert("output".to_owned(), json!([]));
        body.insert(
            "error".to_owned(),
            json!({"code": code, "message": message}),
        );
        if let Some(usage) = observation
            .and_then(|observation| observation.usage.as_ref())
            .filter(|usage| usage.validate().is_ok())
        {
            body.insert("usage".to_owned(), encode_usage(usage));
        }
        let mut payloads = self.open();
        payloads.push(json!({"type": "response.failed", "response": Value::Object(body)}));
        self.finished = true;
        self.number(payloads)
    }

    /// The opening payloads, once.
    fn open(&mut self) -> Vec<Value> {
        if self.opened {
            return Vec::new();
        }
        self.opened = true;
        let response = json!({
            "id": self.id,
            "object": "response",
            "created_at": self.created_at,
            "status": "in_progress",
            "output": [],
        });
        vec![
            json!({"type": "response.created", "response": response}),
            json!({"type": "response.in_progress", "response": response}),
        ]
    }

    /// Opens the response ahead of a delta's payloads and numbers them in wire order.
    fn number(&mut self, payloads: Vec<Value>) -> Vec<Value> {
        if payloads.is_empty() {
            return payloads;
        }
        let mut numbered = self.open();
        numbered.extend(payloads);
        for payload in &mut numbered {
            if let Some(fields) = payload.as_object_mut() {
                fields.insert("sequence_number".to_owned(), json!(self.sequence));
                self.sequence += 1;
            }
        }
        numbered
    }

    fn position(&self, call_id: &CallId) -> Option<usize> {
        self.calls.iter().position(|open| &open.call_id == call_id)
    }

    /// What a reader already assembled must survive into the turn unchanged.
    fn check_streamed(&self, outcome: &TurnOutcome) -> Result<(), Error> {
        for open in &self.calls {
            let Some(call) = outcome
                .tool_calls()
                .find(|call| call.call_id == open.call_id)
            else {
                return Err(streamed_contradiction());
            };
            if call.name != open.name || !relayed_matches(&open.arguments, call) {
                return Err(streamed_contradiction());
            }
        }
        // The decoder keeps text a caller was shown when the turn carries no message for it, so
        // an outcome without one would be read back as a different turn. Shown text that is
        // empty is not kept, and its message is left unfinished rather than finished empty.
        if self
            .message
            .as_ref()
            .is_some_and(|open| !open.text.is_empty())
            && !outcome
                .items
                .iter()
                .any(|item| matches!(item, Item::AssistantText { .. }))
        {
            return Err(streamed_contradiction());
        }
        Ok(())
    }
}

/// How the turn ends on this wire.
struct Terminal {
    event: &'static str,
    status: &'static str,
    incomplete_reason: Option<String>,
}

/// Refuses an outcome this wire cannot carry, and says how it ends.
fn check_outcome(outcome: &TurnOutcome) -> Result<Terminal, Error> {
    let observation = &outcome.observation;
    for item in &outcome.items {
        match item {
            Item::AssistantText { .. } | Item::ToolCall(_) => {}
            Item::Opaque {
                provenance,
                payload,
            } => {
                // Only the binding that produced the state may answer with it: a caller replays
                // it to this endpoint, which forwards it to the binding the turn names.
                if provenance != &observation.binding || provenance.protocol != Protocol::Responses
                {
                    return Err(Error::unsupported(
                        "opaque state belongs to a different serving binding",
                    ));
                }
                if payload.get("type").and_then(Value::as_str) != Some("reasoning") {
                    return Err(Error::unsupported(
                        "the Responses wire carries no such opaque state",
                    ));
                }
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
    if !observation.final_usage {
        return Err(Error::protocol(
            "successful outcome lacks terminal evidence",
        ));
    }
    if let Some(usage) = &observation.usage {
        usage.validate()?;
    }
    terminal(&outcome.stop_reason, outcome.tool_calls().next().is_some())
}

/// The terminal event for a stop reason, refusing one the decoder would read back as another.
fn terminal(stop_reason: &StopReason, has_tool_calls: bool) -> Result<Terminal, Error> {
    let incomplete = |reason: &str| Terminal {
        event: "response.incomplete",
        status: "incomplete",
        incomplete_reason: Some(reason.to_owned()),
    };
    match stop_reason {
        // The decoder derives these two from whether the turn carries a call.
        StopReason::EndTurn if !has_tool_calls => Ok(COMPLETED),
        StopReason::ToolCalls if has_tool_calls => Ok(COMPLETED),
        StopReason::MaxOutputTokens => Ok(incomplete("max_output_tokens")),
        // `max_output_tokens` reads back as `MaxOutputTokens`, and a reason that is not shaped
        // like a code reads back as `unrecognized`.
        StopReason::Incomplete { reason }
            if reason != "max_output_tokens" && is_safe_reason(reason) =>
        {
            Ok(incomplete(reason))
        }
        StopReason::EndTurn | StopReason::ToolCalls | StopReason::Incomplete { .. } => Err(
            Error::unsupported("the Responses wire cannot report this terminal reason"),
        ),
    }
}

const COMPLETED: Terminal = Terminal {
    event: "response.completed",
    status: "completed",
    incomplete_reason: None,
};

fn response_object(
    id: &str,
    created_at: u64,
    outcome: &TurnOutcome,
    terminal: &Terminal,
    output: Vec<Value>,
) -> Value {
    let observation: &TurnObservation = &outcome.observation;
    let mut body = Map::new();
    body.insert(
        "id".to_owned(),
        json!(
            observation
                .response_id
                .as_ref()
                .map_or(id, |reported| reported.as_str())
        ),
    );
    body.insert("object".to_owned(), json!("response"));
    body.insert("created_at".to_owned(), json!(created_at));
    body.insert("status".to_owned(), json!(terminal.status));
    if let Some(reason) = &terminal.incomplete_reason {
        body.insert("incomplete_details".to_owned(), json!({"reason": reason}));
    }
    // An unreported upstream model leaves the field out. Substituting the configured one would
    // turn an absent observation into a reported one.
    if let Some(model) = &observation.upstream_model {
        body.insert("model".to_owned(), json!(model.as_str()));
    }
    body.insert("output".to_owned(), Value::Array(output));
    if let Some(usage) = &observation.usage {
        body.insert("usage".to_owned(), encode_usage(usage));
    }
    Value::Object(body)
}

/// Only counters the upstream reported. An absent one is left out rather than reported as zero,
/// and a total is derived only from two known parts.
///
/// A usage report with no known counter is still a report, and stays one: `{}`, which the decoder
/// reads back as a report whose every counter is unknown.
fn encode_usage(usage: &Usage) -> Value {
    let mut out = Map::new();
    if let Some(value) = usage.input_tokens {
        out.insert("input_tokens".to_owned(), json!(value));
    }
    if let Some(value) = usage.output_tokens {
        out.insert("output_tokens".to_owned(), json!(value));
    }
    if let Some(total) = usage
        .input_tokens
        .zip(usage.output_tokens)
        .and_then(|(input, output)| input.checked_add(output))
    {
        out.insert("total_tokens".to_owned(), json!(total));
    }
    let mut input_details = Map::new();
    if let Some(value) = usage.cached_input_tokens {
        input_details.insert("cached_tokens".to_owned(), json!(value));
    }
    if let Some(value) = usage.cache_creation_input_tokens {
        // This wire has no cache-write counter. A named extension keeps the reported count
        // readable; dropping it would delete an observation the upstream actually made.
        input_details.insert("cache_creation_input_tokens".to_owned(), json!(value));
    }
    if !input_details.is_empty() {
        out.insert(
            "input_tokens_details".to_owned(),
            Value::Object(input_details),
        );
    }
    if let Some(value) = usage.reasoning_output_tokens {
        out.insert(
            "output_tokens_details".to_owned(),
            json!({"reasoning_tokens": value}),
        );
    }
    Value::Object(out)
}

/// The wire code and the fixed diagnostic for a failure.
fn failure(error: &Error) -> (&'static str, &'static str) {
    match error.code {
        ErrorCode::Unavailable if error.retriable => (
            "server_is_overloaded",
            "the upstream is temporarily overloaded",
        ),
        ErrorCode::Unavailable => ("server_error", "the upstream failed this response"),
        ErrorCode::RateLimited => ("rate_limit_exceeded", "the upstream asked for less traffic"),
        ErrorCode::InvalidRequest => ("invalid_request", "the request could not be answered"),
        ErrorCode::Transport => ("transport", "the request could not be answered"),
        ErrorCode::Protocol => ("protocol", "the request could not be answered"),
        ErrorCode::Unauthorized => ("unauthorized", "the request could not be answered"),
        ErrorCode::Refused => ("refused", "the request could not be answered"),
        ErrorCode::TooLarge => ("too_large", "the request could not be answered"),
        ErrorCode::Unsupported => ("unsupported", "the request could not be answered"),
        ErrorCode::Cancelled => ("cancelled", "the request could not be answered"),
        ErrorCode::Deadline => ("deadline", "the request could not be answered"),
    }
}

/// The identifier a non-streamed output item is given, by its position.
fn item_id(item: &Item, id: &str, index: u64) -> String {
    let kind = match item {
        Item::ToolCall(_) => "fc",
        Item::Opaque { .. } => "rs",
        _ => "msg",
    };
    format!("{kind}_{id}_{index}")
}

/// One item of the response's `output`. Only items `check_outcome` accepted reach here.
fn wire_item(item: &Item, item_id: &str) -> Value {
    match item {
        Item::ToolCall(call) => call_item(
            item_id,
            &call.call_id,
            &call.name,
            &call.arguments.to_string(),
            "completed",
        ),
        Item::Opaque { payload, .. } => payload.clone(),
        Item::AssistantText { text } => message_item(item_id, text),
        Item::UserText { .. } | Item::ToolResult { .. } | Item::UnattributedOpaque { .. } => {
            Value::Null
        }
    }
}

fn message_item(item_id: &str, text: &str) -> Value {
    json!({
        "type": "message",
        "id": item_id,
        "status": "completed",
        "role": "assistant",
        "content": [{"type": "output_text", "text": text, "annotations": []}],
    })
}

fn call_item(
    item_id: &str,
    call_id: &CallId,
    name: &ToolName,
    arguments: &str,
    status: &str,
) -> Value {
    json!({
        "type": "function_call",
        "id": item_id,
        "call_id": call_id.as_str(),
        "name": name.as_str(),
        "arguments": arguments,
        "status": status,
    })
}

fn open_message(output_index: u64, item_id: &str) -> [Value; 2] {
    [
        json!({
            "type": "response.output_item.added",
            "output_index": output_index,
            "item": {"type": "message", "id": item_id, "status": "in_progress",
                     "role": "assistant", "content": []},
        }),
        json!({
            "type": "response.content_part.added",
            "item_id": item_id,
            "output_index": output_index,
            "content_index": 0,
            "part": {"type": "output_text", "text": "", "annotations": []},
        }),
    ]
}

fn text_delta(output_index: u64, item_id: &str, text: &str) -> Value {
    json!({
        "type": "response.output_text.delta",
        "item_id": item_id,
        "output_index": output_index,
        "content_index": 0,
        "delta": text,
    })
}

fn finish_message(output_index: u64, item_id: &str, text: &str, item: &Value) -> [Value; 3] {
    [
        json!({
            "type": "response.output_text.done",
            "item_id": item_id,
            "output_index": output_index,
            "content_index": 0,
            "text": text,
        }),
        json!({
            "type": "response.content_part.done",
            "item_id": item_id,
            "output_index": output_index,
            "content_index": 0,
            "part": {"type": "output_text", "text": text, "annotations": []},
        }),
        item_done(output_index, item),
    ]
}

fn open_reasoning(output_index: u64, item_id: &str) -> [Value; 2] {
    [
        json!({
            "type": "response.output_item.added",
            "output_index": output_index,
            "item": {"type": "reasoning", "id": item_id, "summary": []},
        }),
        json!({
            "type": "response.reasoning_summary_part.added",
            "item_id": item_id,
            "output_index": output_index,
            "summary_index": 0,
            "part": {"type": "summary_text", "text": ""},
        }),
    ]
}

fn finish_reasoning_text(open: &OpenText) -> [Value; 2] {
    [
        json!({
            "type": "response.reasoning_summary_text.done",
            "item_id": open.item_id,
            "output_index": open.output_index,
            "summary_index": 0,
            "text": open.text,
        }),
        json!({
            "type": "response.reasoning_summary_part.done",
            "item_id": open.item_id,
            "output_index": open.output_index,
            "summary_index": 0,
            "part": {"type": "summary_text", "text": open.text},
        }),
    ]
}

/// The closing payloads of one call, and its finished item.
///
/// When nothing a reader could assemble was relayed, the arguments are sent here, and the text the
/// reader then holds, which the finished item repeats, is what was relayed followed by them.
fn finish_call(
    payloads: &mut Vec<Value>,
    output_index: u64,
    item_id: &str,
    relayed: String,
    call: &ToolCall,
) -> Value {
    let arguments = if relayed.trim().is_empty() {
        let whole = call.arguments.to_string();
        payloads.push(arguments_delta(output_index, item_id, &whole));
        relayed + &whole
    } else {
        relayed
    };
    payloads.push(json!({
        "type": "response.function_call_arguments.done",
        "item_id": item_id,
        "output_index": output_index,
        "arguments": arguments,
    }));
    let item = call_item(item_id, &call.call_id, &call.name, &arguments, "completed");
    payloads.push(item_done(output_index, &item));
    item
}

fn arguments_delta(output_index: u64, item_id: &str, delta: &str) -> Value {
    json!({
        "type": "response.function_call_arguments.delta",
        "item_id": item_id,
        "output_index": output_index,
        "delta": delta,
    })
}

fn item_done(output_index: u64, item: &Value) -> Value {
    json!({
        "type": "response.output_item.done",
        "output_index": output_index,
        "item": item,
    })
}

/// Whether the argument text a reader assembled is the call's arguments, by the rule the decoder
/// reads them by. Text that is blank was not relayed in any form a reader could act on, and the
/// close completes it.
fn relayed_matches(relayed: &str, call: &ToolCall) -> bool {
    relayed.trim().is_empty()
        || serde_json::from_str::<Value>(relayed).is_ok_and(|parsed| parsed == call.arguments)
}

/// The reader already assembled what was streamed; an outcome that disagrees with it cannot be
/// reconciled by a closing payload.
fn streamed_contradiction() -> Error {
    Error::protocol("model outcome contradicts what was already streamed")
}
