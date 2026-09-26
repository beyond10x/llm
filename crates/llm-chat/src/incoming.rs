//! Chat Completions responses projected onto the neutral outcome and stream vocabulary.
//!
//! Response fields this subset does not read are ignored, because a server may add fields to a
//! response at any time. Every field it does read is refused when it contradicts the wire.
//! Nothing here fills an absent counter, identifier or model name with a configured value.
use llm_core::{
    CallId, Dispatch, Error, Id, Item, Provenance, StopReason, StreamEvent, ToolCall, ToolName,
    TurnObservation, TurnOutcome, Usage,
};
use llm_http::{Framing, SseDecoder, SseEvent};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// One tool call under construction from the deltas that carry it.
#[derive(Debug, Default)]
struct PendingCall {
    call_id: Option<String>,
    name: Option<String>,
    arguments: String,
    /// The caller has been sent this call's [`StreamEvent::ToolCallStarted`].
    announced: bool,
    /// A fragment after the announcement named the call differently.
    renamed: bool,
}

impl PendingCall {
    /// The identity to announce, once both halves have arrived and both are usable. One that
    /// is not usable is never announced and is refused, with its evidence, when the stream
    /// finishes.
    fn announcement(&self) -> Option<(CallId, ToolName)> {
        let call_id = CallId::new(self.call_id.as_deref()?).ok()?;
        let name = ToolName::new(self.name.as_deref()?).ok()?;
        Some((call_id, name))
    }
}

/// What one response reported about itself, independently of its content.
#[derive(Debug, Default)]
struct Reported {
    response_id: Option<Id>,
    upstream_model: Option<Id>,
    usage: Option<Usage>,
}

/// Incremental projection of one Chat Completions stream.
///
/// Evidence observed so far is readable at any point through [`StreamProjection::observation`],
/// so a failure after partial output keeps the snapshot the attempt had already earned.
#[derive(Debug)]
pub struct StreamProjection {
    target: Provenance,
    text: String,
    calls: BTreeMap<u64, PendingCall>,
    /// Every call identifier already announced, whichever index opened it. A second index
    /// opened under one of them is not announced again and its fragments are not relayed:
    /// they would join another call's arguments. `finish` refuses the duplicate.
    announced: BTreeSet<CallId>,
    stop_reason: Option<StopReason>,
    reported: Reported,
    terminated: bool,
}

impl StreamProjection {
    pub fn new(target: Provenance) -> Self {
        Self {
            target,
            text: String::new(),
            calls: BTreeMap::new(),
            announced: BTreeSet::new(),
            stop_reason: None,
            reported: Reported::default(),
            terminated: false,
        }
    }

    /// Accept one framed server-sent event and return the neutral deltas it completed.
    ///
    /// # Errors
    /// Refuses a second choice, an unreadable counter, an unsupported finish reason and a
    /// tool-call delta whose identifier or name never arrived.
    pub fn accept(&mut self, event: &SseEvent) -> Result<Vec<StreamEvent>, Error> {
        self.framed(event).map_err(after_dispatch)
    }

    fn framed(&mut self, event: &SseEvent) -> Result<Vec<StreamEvent>, Error> {
        match event {
            SseEvent::Done => {
                self.terminated = true;
                Ok(Vec::new())
            }
            SseEvent::Payload { data, .. } => self.chunk(data),
        }
    }

    fn chunk(&mut self, data: &Value) -> Result<Vec<StreamEvent>, Error> {
        read_reported(data, &mut self.reported)?;
        let mut events = Vec::new();
        let Some(choices) = data.get("choices").and_then(Value::as_array) else {
            return Ok(events);
        };
        // Counted, not just numbered: an index this wire did not write as a whole count
        // cannot distinguish two completions, and joining them would hand the caller one
        // assistant turn assembled from two.
        if choices.len() > 1 {
            return Err(too_many_choices());
        }
        for choice in choices {
            only_the_first_choice(choice)?;
            if let Some(reason) = choice.get("finish_reason")
                && !reason.is_null()
            {
                self.stop_reason = Some(stop_reason(reason)?);
            }
            if let Some(delta) = choice.get("delta") {
                events.extend(self.delta(delta)?);
            }
        }
        Ok(events)
    }

    fn delta(&mut self, delta: &Value) -> Result<Vec<StreamEvent>, Error> {
        let mut events = Vec::new();
        if let Some(text) = delta.get("content").and_then(Value::as_str)
            && !text.is_empty()
        {
            self.text.push_str(text);
            events.push(StreamEvent::TextDelta {
                text: text.to_owned(),
            });
        }
        // vLLM's reasoning parsers put separated reasoning here. It reaches the caller as a
        // reasoning delta and never joins the assistant text.
        if let Some(text) = delta.get("reasoning_content").and_then(Value::as_str)
            && !text.is_empty()
        {
            events.push(StreamEvent::ReasoningDelta {
                text: text.to_owned(),
            });
        }
        let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) else {
            return Ok(events);
        };
        for call in calls {
            // The wire correlates the fragments of one call by index; the identifier and the
            // name arrive once, in the fragment that opens it.
            let index = call
                .get("index")
                .and_then(Value::as_u64)
                .ok_or_else(|| Error::protocol("chat tool call delta carries no index"))?;
            let pending = self.calls.entry(index).or_default();
            let function = call.get("function");
            for (slot, value) in [
                (&mut pending.call_id, nonempty(call.get("id"))),
                (
                    &mut pending.name,
                    nonempty(function.and_then(|value| value.get("name"))),
                ),
            ] {
                let Some(value) = value else { continue };
                // Announced once. A later fragment naming the call differently is refused
                // when the stream finishes, so the refusal still carries the counters the
                // endpoint reports after it.
                if pending.announced {
                    pending.renamed |= slot.as_deref() != Some(value);
                } else {
                    *slot = Some(value.to_owned());
                }
            }
            let fragment = nonempty(function.and_then(|value| value.get("arguments")));
            if let Some(fragment) = fragment {
                if pending.call_id.is_none() {
                    return Err(Error::protocol(
                        "chat tool call arguments arrived before their identifier",
                    ));
                }
                pending.arguments.push_str(fragment);
            }
            if pending.announced {
                if let (Some(fragment), Some(call_id)) = (fragment, &pending.call_id) {
                    events.push(StreamEvent::ToolArgumentsDelta {
                        call_id: call_identifier(call_id)?,
                        delta: fragment.to_owned(),
                    });
                }
            } else if let Some((call_id, name)) = pending.announcement()
                && !self.announced.contains(&call_id)
            {
                // A fragment is relayed only under a call the caller has been told about. Any
                // that arrived before the name are relayed here, once, right behind it.
                pending.announced = true;
                self.announced.insert(call_id.clone());
                events.push(StreamEvent::ToolCallStarted {
                    call_id: call_id.clone(),
                    name,
                });
                if !pending.arguments.is_empty() {
                    events.push(StreamEvent::ToolArgumentsDelta {
                        call_id,
                        delta: pending.arguments.clone(),
                    });
                }
            }
        }
        Ok(events)
    }

    /// Evidence reported so far. `final_usage` stays false until the stream terminates,
    /// so an interrupted attempt exposes a partial snapshot and never a final cost.
    pub fn observation(&self) -> TurnObservation {
        let mut observation = TurnObservation::new(self.target.clone());
        observation
            .upstream_model
            .clone_from(&self.reported.upstream_model);
        observation
            .response_id
            .clone_from(&self.reported.response_id);
        observation.usage.clone_from(&self.reported.usage);
        observation.final_usage = self.terminated;
        observation
    }

    /// The neutral outcome of a terminated stream.
    ///
    /// # Errors
    /// Refuses a stream that stopped before its terminal sentinel or without a finish reason,
    /// an incomplete tool call, unparsable arguments and contradictory reported counters.
    /// Every one of those carries dispatch evidence that the request was served.
    pub fn finish(&self) -> Result<TurnOutcome, Error> {
        self.outcome().map_err(after_dispatch)
    }

    fn outcome(&self) -> Result<TurnOutcome, Error> {
        if !self.terminated {
            return Err(Error::protocol(
                "chat completion stream ended before its terminal sentinel",
            ));
        }
        let Some(stop_reason) = self.stop_reason.clone() else {
            return Err(Error::protocol(
                "chat completion stream carried no finish reason",
            ));
        };
        let mut items = Vec::new();
        if !self.text.is_empty() {
            items.push(Item::assistant(self.text.clone()));
        }
        for pending in self.calls.values() {
            if pending.renamed {
                return Err(Error::protocol(
                    "chat tool call fragment renames a call already announced",
                ));
            }
            let call_id = pending
                .call_id
                .as_deref()
                .ok_or_else(|| Error::protocol("chat tool call carries no identifier"))?;
            let name = pending
                .name
                .as_deref()
                .ok_or_else(|| Error::protocol("chat tool call carries no name"))?;
            items.push(Item::ToolCall(ToolCall {
                call_id: call_identifier(call_id)?,
                name: tool_name(name)?,
                arguments: parse_arguments(&pending.arguments)?,
            }));
        }
        let observation = self.observation();
        observation.validate_for(&self.target)?;
        Ok(TurnOutcome {
            stop_reason,
            items,
            observation,
        })
    }
}

/// Project a complete Chat Completions response body through the shared SSE framing.
///
/// The deltas accepted before a failure are returned beside it: an attempt that fails after
/// exposing output keeps that output observable.
pub fn project_response_bytes(
    bytes: &[u8],
    target: &Provenance,
) -> (Vec<StreamEvent>, Result<TurnOutcome, Error>) {
    let mut projection = StreamProjection::new(target.clone());
    let mut decoder = SseDecoder::new(Framing::DoneSentinel);
    let mut events = Vec::new();
    for framed in decoder.push(bytes) {
        match framed.and_then(|event| projection.accept(&event)) {
            Ok(produced) => events.extend(produced),
            Err(error) => return (events, Err(after_dispatch(error))),
        }
    }
    if let Err(error) = decoder.finish() {
        return (events, Err(after_dispatch(error)));
    }
    (events, projection.finish())
}

/// Project one non-streamed Chat Completions response body.
///
/// # Errors
/// Refuses a body with other than one choice, a missing or unsupported finish reason, an
/// incomplete tool call, unparsable arguments and contradictory reported counters. Every one
/// of those carries dispatch evidence that the request was served.
pub fn decode_completion(body: &Value, target: &Provenance) -> Result<TurnOutcome, Error> {
    completion(body, target).map_err(after_dispatch)
}

fn completion(body: &Value, target: &Provenance) -> Result<TurnOutcome, Error> {
    let mut reported = Reported::default();
    read_reported(body, &mut reported)?;
    let choices = body
        .get("choices")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::protocol("chat completion carries no choices"))?;
    let [choice] = choices.as_slice() else {
        return Err(Error::unsupported(
            "chat completion carries other than one choice",
        ));
    };
    only_the_first_choice(choice)?;
    let stop_reason = stop_reason(
        choice
            .get("finish_reason")
            .ok_or_else(|| Error::protocol("chat completion carries no finish reason"))?,
    )?;
    let message = choice
        .get("message")
        .ok_or_else(|| Error::protocol("chat completion choice carries no message"))?;
    let mut items = Vec::new();
    if let Some(text) = nonempty(message.get("content")) {
        items.push(Item::assistant(text));
    }
    if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
        for call in calls {
            let call_id = nonempty(call.get("id"))
                .ok_or_else(|| Error::protocol("chat tool call carries no identifier"))?;
            let function = call.get("function");
            let name = nonempty(function.and_then(|value| value.get("name")))
                .ok_or_else(|| Error::protocol("chat tool call carries no name"))?;
            let arguments = function
                .and_then(|value| value.get("arguments"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            items.push(Item::ToolCall(ToolCall {
                call_id: call_identifier(call_id)?,
                name: tool_name(name)?,
                arguments: parse_arguments(arguments)?,
            }));
        }
    }
    let mut observation = TurnObservation::new(target.clone());
    observation.upstream_model = reported.upstream_model;
    observation.response_id = reported.response_id;
    observation.usage = reported.usage;
    // A complete non-streamed body is the whole report this attempt will ever make.
    observation.final_usage = true;
    observation.validate_for(target)?;
    Ok(TurnOutcome {
        stop_reason,
        items,
        observation,
    })
}

fn too_many_choices() -> Error {
    Error::unsupported("chat completion returned more than one choice")
}

/// A single choice is the only one this projection asked for. An absent index is that one;
/// a present index has to say so, and one that is not a whole count says nothing at all.
fn only_the_first_choice(choice: &Value) -> Result<(), Error> {
    match choice.get("index") {
        None | Some(Value::Null) => Ok(()),
        Some(index) if index.as_u64() == Some(0) => Ok(()),
        Some(index) if index.as_u64().is_some() => Err(too_many_choices()),
        Some(_) => Err(Error::protocol(
            "chat completion choice index is not a whole count",
        )),
    }
}

/// A nonempty string field, or `None` for absent, null, empty and non-string values.
fn nonempty(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
}

fn read_reported(value: &Value, into: &mut Reported) -> Result<(), Error> {
    if let Some(text) = nonempty(value.get("id")) {
        into.response_id = Some(identifier(text)?);
    }
    // An unreported model stays unreported. The configured alias never fills this in.
    if let Some(text) = nonempty(value.get("model")) {
        into.upstream_model = Some(identifier(text)?);
    }
    if let Some(usage) = value.get("usage")
        && !usage.is_null()
    {
        into.usage = Some(usage_of(usage)?);
    }
    Ok(())
}

/// Nothing in this module is reachable before the endpoint has served bytes: a response is
/// the only input it has. So no refusal it produces may report that the request was never
/// sent — dispatch is the retry signal, and `not-sent` here would invite a resend of a turn
/// the provider already served and may already have billed.
///
/// Every public entry point applies this, so a refusal added inside cannot escape it. There
/// are three — `StreamProjection::accept`, `StreamProjection::finish` and
/// `decode_completion`, with `project_response_bytes` composing the first two — and a
/// previous revision of this comment said two, which is how `accept` came to be the one
/// without it. `every_exported_decode_entry_point_applies_the_dispatch_correction` now reads
/// that count out of this file rather than out of a sentence anybody has to keep true.
/// Evidence a lower layer already qualified, such as the transport's `unknown`, is kept.
fn after_dispatch(error: Error) -> Error {
    if error.dispatch == Dispatch::NotSent {
        return error.with_dispatch(Dispatch::Accepted);
    }
    error
}

fn usage_of(value: &Value) -> Result<Usage, Error> {
    Ok(Usage {
        input_tokens: count(value, "prompt_tokens")?,
        output_tokens: count(value, "completion_tokens")?,
        cached_input_tokens: detail(value, "prompt_tokens_details", "cached_tokens")?,
        // No provider serving this wire reports a cache-write counter, so against one this
        // stays unknown. It is read because this repository's own gateway writes it here
        // (`ingress.rs`), and a counter deleted on read-back is a counter this wire lost.
        cache_creation_input_tokens: detail(
            value,
            "prompt_tokens_details",
            "cache_creation_input_tokens",
        )?,
        reasoning_output_tokens: detail(value, "completion_tokens_details", "reasoning_tokens")?,
    })
}

fn count(value: &Value, field: &str) -> Result<Option<u64>, Error> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(found) => found.as_u64().map(Some).ok_or_else(|| {
            Error::protocol("chat completion reported a counter that is not a whole count")
        }),
    }
}

fn detail(value: &Value, object: &str, field: &str) -> Result<Option<u64>, Error> {
    match value.get(object) {
        None | Some(Value::Null) => Ok(None),
        Some(details) => count(details, field),
    }
}

/// The finish reasons this subset implements. An unknown one is refused, and the refusal
/// carries no byte of the upstream response.
fn stop_reason(value: &Value) -> Result<StopReason, Error> {
    match value.as_str() {
        Some("stop") => Ok(StopReason::EndTurn),
        Some("length") => Ok(StopReason::MaxOutputTokens),
        Some("tool_calls") => Ok(StopReason::ToolCalls),
        Some("content_filter") => Ok(StopReason::Incomplete {
            reason: "content-filter".to_owned(),
        }),
        _ => Err(Error::protocol(
            "chat completion reported a finish reason outside the supported subset",
        )),
    }
}

/// The accumulated arguments of one proposed call. A call the model made with no arguments
/// reaches the caller as an empty object, which is what the wire's empty string means.
pub(crate) fn parse_arguments(raw: &str) -> Result<Value, Error> {
    if raw.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(raw)
        .map_err(|_| Error::protocol("chat tool call arguments are not valid JSON"))
}

fn identifier(value: &str) -> Result<Id, Error> {
    Id::new(value).map_err(|_| Error::protocol("chat completion reported an unusable identifier"))
}

fn call_identifier(value: &str) -> Result<CallId, Error> {
    CallId::new(value)
        .map_err(|_| Error::protocol("chat completion reported an unusable tool call identifier"))
}

fn tool_name(value: &str) -> Result<ToolName, Error> {
    ToolName::new(value)
        .map_err(|_| Error::protocol("chat completion reported an unusable tool name"))
}
