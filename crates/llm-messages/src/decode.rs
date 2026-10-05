use crate::{codec::decode_block, fields, object, path, string, usage::Snapshot};
use llm_core::{
    CallId, Cancel, Dispatch, Error, ErrorCode, Id, Item, MAX_ITEMS, MAX_TOOL_ARGUMENT_BYTES,
    Provenance, StopReason, StreamEvent, StreamSink, TurnObservation, TurnOutcome, TurnRequest,
};
use llm_http::{Framing, SseDecoder, SseEvent};
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Instant};

/// Every field one complete message may carry. `stop_sequence` is retained producer metadata:
/// the stop reason already names a sequence stop, so the sequence itself changes no decision.
const MESSAGE_FIELDS: [&str; 8] = [
    "id",
    "type",
    "role",
    "model",
    "content",
    "stop_reason",
    "stop_sequence",
    "usage",
];
const MAX_STOP_REASON_BYTES: usize = 64;
// Where a streamed object sits, as a refusal names it: the event type, then the field path.
const START: &str = "message_start.message";
const BLOCK: &str = "content_block_start.content_block";
const DELTA: &str = "content_block_delta.delta";

/// The invariants that travel with [`MESSAGE_FIELDS`].
///
/// Both halves of this codec read the same object — a complete response, and the one inside
/// `message_start` — so both ask the same question of it. A field list shared without the checks
/// that go with it is two decoders that agree about spelling and disagree about meaning.
fn assistant_message(message: &Value, at: &str) -> Result<(), Error> {
    fields(message, at, &MESSAGE_FIELDS)?;
    if text(message, "type")? != Some("message") || text(message, "role")? != Some("assistant") {
        return Err(Error::protocol(
            "Messages response is not an assistant message",
        ));
    }
    Ok(())
}

fn text<'a>(value: &'a Value, field: &str) -> Result<Option<&'a str>, Error> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(found)) => Ok(Some(found)),
        Some(_) => Err(Error::protocol("Messages field is not a string")),
    }
}

/// What one message reported about itself, independently of its content.
#[derive(Default)]
struct Header {
    upstream_model: Option<Id>,
    response_id: Option<Id>,
    usage: Snapshot,
    stop_reason: Option<String>,
}

impl Header {
    fn read(&mut self, message: &Value, at: &str) -> Result<(), Error> {
        if let Some(model) = text(message, "model")? {
            // The route's own name for what served the turn; a configured alias never fills it.
            self.upstream_model =
                Some(Id::new(model).map_err(|_| Error::protocol("invalid Messages model name"))?);
        }
        if let Some(id) = text(message, "id")? {
            self.response_id =
                Some(Id::new(id).map_err(|_| Error::protocol("invalid Messages response id"))?);
        }
        if let Some(usage) = message.get("usage").filter(|value| !value.is_null()) {
            self.usage = self.usage.update(usage, &path(at, "usage"))?;
        }
        self.reason(text(message, "stop_reason")?)
    }

    fn reason(&mut self, reported: Option<&str>) -> Result<(), Error> {
        let Some(reported) = reported else {
            return Ok(());
        };
        if self.stop_reason.is_some() {
            return Err(Error::protocol("Messages reported two terminal reasons"));
        }
        if reported.is_empty()
            || reported.len() > MAX_STOP_REASON_BYTES
            || !reported.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(Error::protocol(
                "Messages terminal reason is not a bounded name",
            ));
        }
        self.stop_reason = Some(reported.to_owned());
        Ok(())
    }

    /// The last valid bound snapshot. An unreported counter stays unknown, never zero.
    fn observation(&self, target: &Provenance, final_usage: bool) -> TurnObservation {
        TurnObservation {
            binding: target.clone(),
            upstream_model: self.upstream_model.clone(),
            response_id: self.response_id.clone(),
            usage: if self.usage.is_known() {
                self.usage.normalized().ok()
            } else {
                None
            },
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
        Ok(outcome)
    }
}

/// A reason this projection does not model is carried under its own name, never as completion.
fn stop_reason(reported: Option<&str>) -> Result<StopReason, Error> {
    match reported {
        // Absence is not `end_turn`: a terminal message that names no reason is unknown.
        None => Err(Error::protocol("Messages terminal message names no reason")),
        Some("end_turn") => Ok(StopReason::EndTurn),
        Some("tool_use") => Ok(StopReason::ToolCalls),
        Some("max_tokens") => Ok(StopReason::MaxOutputTokens),
        Some(other) => Ok(StopReason::Incomplete {
            reason: other.to_owned(),
        }),
    }
}

/// Decode one complete Messages response into a bound neutral outcome.
///
/// # Errors
/// Refuses content, metadata, terminal reasons and usage outside the declared subset, retaining
/// the reported counters on failure.
pub fn decode_message(
    message: &Value,
    request: &TurnRequest,
    target: &Provenance,
) -> Result<TurnOutcome, Error> {
    // A refusal here describes a response that arrived, so it says so and keeps what it read.
    let mut header = Header::default();
    read_message(message, request, target, &mut header)
        .map_err(|error| header.attach(error, target))
}

fn read_message(
    message: &Value,
    request: &TurnRequest,
    target: &Provenance,
    header: &mut Header,
) -> Result<TurnOutcome, Error> {
    // A complete response is its own root: its fields are named from there.
    assistant_message(message, "")?;
    header.read(message, "")?;
    let content = message
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::protocol("Messages content must be an array"))?;
    let mut items = Vec::new();
    for block in content {
        items.push(decode_block(block, false, Some(target), "content[]")?);
    }
    header.outcome(items, request, target)
}

/// One content block being assembled from its deltas.
struct Block {
    value: Value,
    /// Accumulated `input_json_delta` fragments, parsed once when the block closes: half an
    /// argument object must never reach a caller as a whole one.
    arguments: String,
    call: Option<CallId>,
    /// A block type outside the declared subset: kept whole, never interpreted.
    unknown: bool,
}

const UNKNOWN_EVENT: &str = "unknown-stream-event";
const UNKNOWN_BLOCK: &str = "unknown-output-item";

/// Accumulates one Messages event stream into a bound neutral outcome.
///
/// A terminal `message_stop` payload is required: this route has no `[DONE]` sentinel, so
/// end of input is the absence of an answer rather than the end of one.
pub struct StreamDecoder {
    target: Provenance,
    /// The turn's absolute deadline, when the caller gave the client one. A blocked sink is
    /// bounded by it exactly as the HTTP exchange is.
    deadline: Option<Instant>,
    header: Header,
    blocks: BTreeMap<u64, Block>,
    /// Keyed by content-block index: the order the route numbered its content is the order the
    /// caller was streamed it, and a stop that arrives early does not reorder the turn.
    items: BTreeMap<u64, Item>,
    next_index: u64,
    /// Every `tool_use` id already announced. A second block under one of them is not
    /// announced again and its fragments are not relayed; the finish refuses the duplicate.
    announced: Vec<CallId>,
    /// Events outside the declared subset, kept whole, each with the block index that would
    /// have started next when it arrived: it sits after every block that had already started.
    preserved: Vec<(u64, Item)>,
    started: bool,
    complete: bool,
}

impl StreamDecoder {
    pub fn new(target: Provenance) -> Self {
        Self {
            target,
            deadline: None,
            header: Header::default(),
            blocks: BTreeMap::new(),
            items: BTreeMap::new(),
            next_index: 0,
            announced: Vec::new(),
            preserved: Vec::new(),
            started: false,
            complete: false,
        }
    }

    /// Bound every remaining wait in this decode by one absolute instant.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }

    pub const fn is_complete(&self) -> bool {
        self.complete
    }

    /// Retains dispatch evidence and the last valid usage snapshot on a failure.
    pub fn attach(&self, error: Error) -> Error {
        self.header.attach(error, &self.target)
    }

    /// Apply one framed event, emitting the neutral deltas a caller watches.
    ///
    /// # Errors
    /// Refuses unsupported, contradictory, duplicate and out-of-order events, and a sink or
    /// cancellation failure raised while the caller was being told.
    pub async fn apply(
        &mut self,
        event: &SseEvent,
        sink: &mut dyn StreamSink,
        cancel: &Cancel,
    ) -> Result<(), Error> {
        let SseEvent::Payload { event: name, data } = event else {
            return Err(Error::protocol("Messages framing has no terminal sentinel"));
        };
        if self.complete {
            return Err(Error::protocol(
                "Messages payload follows the terminal event",
            ));
        }
        let kind = string(data, "type")?;
        if name.as_ref().is_some_and(|name| name != kind) {
            return Err(Error::protocol(
                "Messages event name contradicts its payload type",
            ));
        }
        match kind {
            // A keep-alive. Its irrelevance to this subset is the route's own documentation.
            "ping" => fields(data, "ping", &["type"]),
            "message_start" => self.start_message(data),
            "content_block_start" => self.start_block(data, sink, cancel).await,
            "content_block_delta" => self.apply_delta(data, sink, cancel).await,
            "content_block_stop" => self.stop_block(data),
            "message_delta" => self.apply_message_delta(data),
            "message_stop" => self.stop_message(data),
            "error" => Err(stream_error(data.get("error"))),
            // A route that adds an event type is not a broken stream: the event is kept whole
            // and the caller told it was not interpreted. Dropping it would hide it; refusing it
            // would end a turn the route is still answering.
            _ => {
                self.preserve(
                    data,
                    "a stream event outside the pinned subset was preserved, not interpreted",
                    sink,
                    cancel,
                )
                .await
            }
        }
    }

    /// Refuses one more item past the content bound, counting what is open and what was kept.
    fn bound(&self) -> Result<(), Error> {
        if self.items.len() + self.blocks.len() + self.preserved.len() >= MAX_ITEMS {
            return Err(Error::too_large("Messages content exceeds its bound"));
        }
        Ok(())
    }

    async fn preserve_delta(
        &mut self,
        data: &Value,
        sink: &mut dyn StreamSink,
        cancel: &Cancel,
    ) -> Result<(), Error> {
        self.preserve(
            data,
            "a content block delta outside the pinned subset was preserved, not interpreted",
            sink,
            cancel,
        )
        .await
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

    fn started(&self) -> Result<(), Error> {
        if self.started {
            return Ok(());
        }
        Err(Error::protocol("Messages event precedes its message start"))
    }

    fn start_message(&mut self, data: &Value) -> Result<(), Error> {
        fields(data, "message_start", &["type", "message"])?;
        if self.started {
            return Err(Error::protocol("Messages stream started twice"));
        }
        let message = data
            .get("message")
            .ok_or_else(|| Error::protocol("Messages start carries no message"))?;
        assistant_message(message, START)?;
        if message
            .get("content")
            .is_some_and(|content| content.as_array().is_none_or(|blocks| !blocks.is_empty()))
        {
            return Err(Error::protocol("Messages start carries content"));
        }
        self.header.read(message, START)?;
        self.started = true;
        Ok(())
    }

    async fn start_block(
        &mut self,
        data: &Value,
        sink: &mut dyn StreamSink,
        cancel: &Cancel,
    ) -> Result<(), Error> {
        self.started()?;
        fields(
            data,
            "content_block_start",
            &["type", "index", "content_block"],
        )?;
        let index = index(data)?;
        if self.blocks.contains_key(&index) || index < self.next_index {
            return Err(Error::protocol("Messages content block is out of order"));
        }
        self.bound()?;
        let value = data
            .get("content_block")
            .ok_or_else(|| Error::protocol("Messages block start carries no block"))?
            .clone();
        let announcement = match string(&value, "type")? {
            "text" | "thinking" | "redacted_thinking" => None,
            "tool_use" => {
                let call_id = CallId::new(string(&value, "id")?)
                    .map_err(|_| Error::protocol("invalid Messages tool call id"))?;
                // Named by the rule the finished block is decoded by, so a name the route could
                // not be answered under is refused here, before anyone is told about the call,
                // rather than after its arguments were already relayed.
                let opening = json!({"type": "tool_use", "id": call_id.as_str(),
                    "name": string(&value, "name")?, "input": {}});
                let Item::ToolCall(call) =
                    decode_block(&opening, false, Some(&self.target), BLOCK)?
                else {
                    return Err(Error::unsupported(
                        "Messages content is outside the declared subset",
                    ));
                };
                Some(StreamEvent::ToolCallStarted {
                    call_id,
                    name: call.name,
                })
            }
            // Kept whole at its index and never interpreted: a dropped block is a hole in the
            // conversation the next turn cannot see. The caller is told when it opens.
            _ => Some(warning(
                UNKNOWN_BLOCK,
                "a content block outside the pinned subset was preserved, not interpreted",
            )),
        };
        let unknown = matches!(announcement, Some(StreamEvent::Warning { .. }));
        let announcement = announcement.filter(|event| match event {
            StreamEvent::ToolCallStarted { call_id, .. } => !self.announced.contains(call_id),
            _ => true,
        });
        let call = match &announcement {
            Some(StreamEvent::ToolCallStarted { call_id, .. }) => {
                self.announced.push(call_id.clone());
                Some(call_id.clone())
            }
            _ => None,
        };
        self.next_index = index.saturating_add(1);
        self.blocks.insert(
            index,
            Block {
                value,
                arguments: String::new(),
                call,
                unknown,
            },
        );
        match announcement {
            Some(event) => emit(sink, event, cancel, self.deadline).await,
            None => Ok(()),
        }
    }

    async fn apply_delta(
        &mut self,
        data: &Value,
        sink: &mut dyn StreamSink,
        cancel: &Cancel,
    ) -> Result<(), Error> {
        self.started()?;
        fields(data, "content_block_delta", &["type", "index", "delta"])?;
        let index = index(data)?;
        let delta = data
            .get("delta")
            .ok_or_else(|| Error::protocol("Messages delta carries no delta"))?;
        let block = self.blocks.get_mut(&index).ok_or_else(|| {
            Error::protocol("Messages delta names a content block that never started")
        })?;
        // Nothing inside a block this subset does not model is interpreted, whatever its type.
        if block.unknown {
            return self.preserve_delta(data, sink, cancel).await;
        }
        let event = match string(delta, "type")? {
            "text_delta" => {
                fields(delta, DELTA, &["type", "text"])?;
                let text = string(delta, "text")?;
                append(&mut block.value, "text", text)?;
                (!text.is_empty()).then(|| StreamEvent::TextDelta {
                    text: text.to_owned(),
                })
            }
            // Shown as it arrives and also folded into the block, which stays opaque and is
            // replayed with its signature. Opaque means never reinterpreted, not never seen.
            "thinking_delta" => {
                fields(delta, DELTA, &["type", "thinking"])?;
                let text = string(delta, "thinking")?;
                append(&mut block.value, "thinking", text)?;
                (!text.is_empty()).then(|| StreamEvent::ReasoningDelta {
                    text: text.to_owned(),
                })
            }
            // Never shown: a signature is not reasoning, it is what the route verifies against.
            // A thinking block may open with no `signature` field at all; its first delta begins
            // it. The finished block is still refused unless it carries a signature.
            "signature_delta" => {
                fields(delta, DELTA, &["type", "signature"])?;
                if block.value.get("type").and_then(Value::as_str) == Some("thinking")
                    && let Some(opened) = block.value.as_object_mut()
                {
                    opened
                        .entry("signature")
                        .or_insert_with(|| Value::String(String::new()));
                }
                append(&mut block.value, "signature", string(delta, "signature")?)?;
                None
            }
            "input_json_delta" => {
                fields(delta, DELTA, &["type", "partial_json"])?;
                let fragment = string(delta, "partial_json")?;
                if block.arguments.len().saturating_add(fragment.len()) > MAX_TOOL_ARGUMENT_BYTES {
                    return Err(Error::too_large(
                        "Messages streamed tool arguments exceed their bound",
                    ));
                }
                block.arguments.push_str(fragment);
                block
                    .call
                    .clone()
                    .filter(|_| !fragment.is_empty())
                    .map(|call_id| StreamEvent::ToolArgumentsDelta {
                        call_id,
                        delta: fragment.to_owned(),
                    })
            }
            // A delta type this subset does not model is kept as its whole event; the block it
            // names still assembles from the deltas around it.
            _ => return self.preserve_delta(data, sink, cancel).await,
        };
        match event {
            Some(event) => emit(sink, event, cancel, self.deadline).await,
            None => Ok(()),
        }
    }

    fn stop_block(&mut self, data: &Value) -> Result<(), Error> {
        self.started()?;
        fields(data, "content_block_stop", &["type", "index"])?;
        let index = index(data)?;
        let mut block = self.blocks.remove(&index).ok_or_else(|| {
            Error::protocol("Messages block stop names a block that never started")
        })?;
        if block.unknown {
            // Its deltas were kept as events of their own; the block is kept as it opened.
            self.items.insert(
                index,
                Item::Opaque {
                    provenance: self.target.clone(),
                    payload: block.value,
                },
            );
            return Ok(());
        }
        if !block.arguments.is_empty() {
            let arguments: Value = serde_json::from_str(&block.arguments)
                .map_err(|_| Error::protocol("Messages streamed tool arguments are not JSON"))?;
            object(&block.value)?;
            if let Some(fields) = block.value.as_object_mut() {
                fields.insert("input".to_owned(), arguments);
            }
        }
        self.items.insert(
            index,
            decode_block(&block.value, false, Some(&self.target), BLOCK)?,
        );
        Ok(())
    }

    fn apply_message_delta(&mut self, data: &Value) -> Result<(), Error> {
        self.started()?;
        fields(data, "message_delta", &["type", "delta", "usage"])?;
        let delta = data
            .get("delta")
            .ok_or_else(|| Error::protocol("Messages message delta carries no delta"))?;
        fields(
            delta,
            "message_delta.delta",
            &["stop_reason", "stop_sequence"],
        )?;
        // Cumulative, not incremental: the reported counters replace the previous ones.
        if let Some(usage) = data.get("usage").filter(|value| !value.is_null()) {
            self.header.usage = self.header.usage.update(usage, "message_delta.usage")?;
        }
        self.header.reason(text(delta, "stop_reason")?)
    }

    fn stop_message(&mut self, data: &Value) -> Result<(), Error> {
        self.started()?;
        fields(data, "message_stop", &["type"])?;
        if !self.blocks.is_empty() {
            return Err(Error::protocol(
                "Messages terminal event leaves a content block open",
            ));
        }
        self.complete = true;
        Ok(())
    }

    /// The terminal outcome.
    ///
    /// # Errors
    /// Refuses a stream that never reached its terminal event, retaining the last snapshot.
    pub fn finish(self, request: &TurnRequest) -> Result<TurnOutcome, Error> {
        let Self {
            target,
            header,
            items,
            preserved,
            complete,
            ..
        } = self;
        let decoded = || -> Result<TurnOutcome, Error> {
            if !complete {
                return Err(Error::protocol(
                    "the Messages stream ended before its terminal event",
                ));
            }
            header.outcome(merge(items, preserved), request, &target)
        };
        decoded().map_err(|error| header.attach(error, &target))
    }
}

/// Content in index order, each kept event after every block that had started when it arrived.
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

/// Decode a complete Messages event stream through the production framing and decoder.
///
/// Reads every byte it was given: a payload after the terminal event is a stream this subset
/// does not describe, and is refused rather than discarded.
///
/// # Errors
/// Refuses framing failures, unsupported events and a stream without its terminal event.
pub async fn decode_stream(
    bytes: &[u8],
    request: &TurnRequest,
    target: &Provenance,
    sink: &mut dyn StreamSink,
    cancel: &Cancel,
) -> Result<TurnOutcome, Error> {
    let mut framing = SseDecoder::new(Framing::PayloadsOnly);
    let mut decoder = StreamDecoder::new(target.clone());
    // Chunked so that the framing is exercised across arbitrary network splits.
    for chunk in bytes.chunks(64) {
        for event in framing.push(chunk) {
            let event = event.map_err(|error| decoder.attach(error))?;
            decoder
                .apply(&event, sink, cancel)
                .await
                .map_err(|error| decoder.attach(error))?;
        }
    }
    framing.finish().map_err(|error| decoder.attach(error))?;
    decoder.finish(request)
}

fn index(data: &Value) -> Result<u64, Error> {
    data.get("index")
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::protocol("Messages content block index is missing or invalid"))
}

/// A fixed warning: the code and this crate's own text, never the producer's.
fn warning(code: &str, message: &str) -> StreamEvent {
    StreamEvent::Warning {
        code: code.to_owned(),
        message: message.to_owned(),
    }
}

fn append(block: &mut Value, field: &str, text: &str) -> Result<(), Error> {
    let existing = block
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::protocol("Messages delta does not extend its block"))?;
    let joined = format!("{existing}{text}");
    object(block)?;
    if let Some(fields) = block.as_object_mut() {
        fields.insert(field.to_owned(), json!(joined));
    }
    Ok(())
}

/// Fixed diagnostics for a reported upstream failure. No response text is copied into a message.
fn stream_error(error: Option<&Value>) -> Error {
    let kind = error
        .and_then(|error| error.get("type"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let (code, message) = match kind {
        "overloaded_error" => (
            ErrorCode::Unavailable,
            "the Messages route reported that it is overloaded",
        ),
        "api_error" => (
            ErrorCode::Transport,
            "the Messages route reported an internal failure",
        ),
        "timeout_error" => (ErrorCode::Deadline, "the Messages route reported a timeout"),
        "rate_limit_error" => (
            ErrorCode::RateLimited,
            "the Messages route reported a rate limit",
        ),
        "authentication_error" => (
            ErrorCode::Unauthorized,
            "the Messages route refused the presented credential",
        ),
        "permission_error" => (
            ErrorCode::Unauthorized,
            "the Messages route refused this account's access",
        ),
        "invalid_request_error" => (
            ErrorCode::InvalidRequest,
            "the Messages route refused the request as invalid",
        ),
        "request_too_large" => (
            ErrorCode::TooLarge,
            "the Messages route refused the request as too large",
        ),
        "not_found_error" => (
            ErrorCode::Refused,
            "the Messages route did not find the requested resource",
        ),
        _ => (ErrorCode::Refused, "the Messages route failed the message"),
    };
    Error::new(code, message).with_dispatch(Dispatch::Accepted)
}

/// Awaits the caller, bounded by both of the things that end a turn from outside it.
///
/// Backpressure is still awaited: this is a bound on the wait, not a reason to drop an event.
async fn emit(
    sink: &mut dyn StreamSink,
    event: StreamEvent,
    cancel: &Cancel,
    deadline: Option<Instant>,
) -> Result<(), Error> {
    tokio::select! {
        biased;
        () = cancel.cancelled() => Err(Error::cancelled().with_dispatch(Dispatch::Accepted)),
        () = expiry(deadline) => Err(Error::new(ErrorCode::Deadline, "the Messages turn deadline expired")
            .with_dispatch(Dispatch::Accepted)),
        result = sink.emit(event) => result,
    }
}

/// Completes at the turn's deadline, or never when the caller configured none.
async fn expiry(deadline: Option<Instant>) {
    match deadline {
        None => std::future::pending().await,
        Some(at) => tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await,
    }
}
