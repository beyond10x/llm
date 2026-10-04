//! One streamed response, decoded into neutral items, counters and terminal truth.
//!
//! The input is the ordered sequence of decoded server-sent-event payloads. Framing them out of
//! a byte stream is `llm-http`'s `SseDecoder`; this module never sees a byte boundary, which is
//! why a route that ends without the `data: [DONE]` sentinel — vLLM v0.27.1 sends none — is
//! decided here by the terminal response object and not by the sentinel.

use std::collections::BTreeMap;

use llm_core::{
    CallId, Dispatch, Error, ErrorCode, Id, Item, MAX_ITEMS, MAX_TOOL_ARGUMENT_BYTES, StopReason,
    StreamEvent, ToolCall, ToolName, TurnObservation, TurnOutcome, Usage, exceeds,
};
use serde_json::Value;

use crate::Binding;

/// Every stream event discriminator this decoder interprets rather than preserving as unknown.
///
/// The two reasoning families are both here because two servers speaking this protocol disagree.
/// `OpenAI` streams `response.reasoning_summary_*`; vLLM v0.27.1 streams `response.reasoning_*`,
/// measured at 301 of 307 warning lines in a two-turn run recorded as
/// `verification-report:openai-responses-on-vllm` in `beyond10x/harness`. Neither is wrong, and a
/// pin that knows one of them makes a visibly working run look silent on the other.
pub const ACCEPTED_STREAM_EVENTS: &[&str] = &[
    "error",
    "keepalive",
    "response.completed",
    "response.content_part.added",
    "response.content_part.done",
    "response.created",
    "response.failed",
    "response.function_call_arguments.delta",
    "response.function_call_arguments.done",
    "response.in_progress",
    "response.incomplete",
    "response.output_item.added",
    "response.output_item.done",
    "response.output_text.delta",
    "response.output_text.done",
    "response.reasoning_part.added",
    "response.reasoning_part.done",
    "response.reasoning_summary_part.added",
    "response.reasoning_summary_part.done",
    "response.reasoning_summary_text.delta",
    "response.reasoning_summary_text.done",
    "response.reasoning_text.delta",
    "response.reasoning_text.done",
];

/// The discriminators that produce a [`StreamEvent::ReasoningDelta`], one per server family.
pub const REASONING_DELTA_EVENTS: &[&str] = &[
    "response.reasoning_summary_text.delta",
    "response.reasoning_text.delta",
];

/// What one decoded stream produced.
///
/// The visible events are separate from the result because a failure after partial output must
/// not erase the output the caller already saw. A `Result` alone cannot carry both.
#[derive(Debug)]
pub struct StreamDecoding {
    /// Ordered events a caller may show, in the order the wire produced them.
    pub events: Vec<StreamEvent>,
    /// The turn, or the typed refusal, with the last bound evidence attached either way.
    pub result: Result<TurnOutcome, Error>,
}

/// Decodes one complete stream of Responses payloads into a neutral outcome.
///
/// Terminal truth comes from a `response.completed` or `response.incomplete` object and from
/// nothing else: not from end of stream, not from a sentinel, not from the last delta seen.
pub fn decode_stream(binding: &Binding, payloads: &[Value]) -> StreamDecoding {
    let mut decoder = Decoder::new(binding);
    for payload in payloads {
        if let Err(error) = decoder.apply(payload) {
            return StreamDecoding {
                events: decoder.events,
                result: Err(error),
            };
        }
    }
    decoder.finish()
}

struct Decoder<'a> {
    binding: &'a Binding,
    events: Vec<StreamEvent>,
    /// `item_id` -> `call_id`, so an arguments delta can name the call a reader is watching.
    calls: BTreeMap<String, CallId>,
    /// Every call the caller was told about, in announcement order. The outcome must carry each
    /// under the identifier and the name it was announced with.
    announced: Vec<(CallId, ToolName)>,
    streamed: Vec<Item>,
    /// Opaque state that must survive even when the terminal object replaces streamed output.
    unmodelled: Vec<Item>,
    terminal: Option<Terminal>,
    /// A refusal raised while decoding a streamed item, held until the stream is drained.
    ///
    /// Returning it where it is raised is what made the first version of the retained-evidence
    /// rule unreachable: a server announces a finished item with `response.output_item.done`
    /// **before** `response.completed`, so returning on the spot discards the terminal object
    /// sitting later in the same stream and with it the counters the attempt was billed for.
    /// Waiting does not soften the refusal — it is still returned, and `decode_stream` still
    /// reports every event the caller already saw.
    deferred: Option<Error>,
}

/// One terminal response object and the event that delivered it.
///
/// The event name is kept because it is the authoritative half. `response.incomplete` says the
/// turn did not finish whether or not the object inside it repeats `status`, and reading only
/// the object decoded such a response as a turn that ended normally.
struct Terminal {
    incomplete: bool,
    response: Value,
}

impl<'a> Decoder<'a> {
    fn new(binding: &'a Binding) -> Self {
        Self {
            binding,
            events: Vec::new(),
            calls: BTreeMap::new(),
            announced: Vec::new(),
            streamed: Vec::new(),
            unmodelled: Vec::new(),
            terminal: None,
            deferred: None,
        }
    }

    fn apply(&mut self, event: &Value) -> Result<(), Error> {
        match event.get("type").and_then(Value::as_str) {
            Some("response.output_text.delta") => {
                if let Some(text) = string(event.get("delta")) {
                    self.events.push(StreamEvent::TextDelta {
                        text: text.to_owned(),
                    });
                }
            }
            Some(name) if REASONING_DELTA_EVENTS.contains(&name) => {
                if let Some(text) = string(event.get("delta")) {
                    self.events.push(StreamEvent::ReasoningDelta {
                        text: text.to_owned(),
                    });
                }
            }
            Some("response.output_item.added") => self.remember_call(event),
            Some("response.function_call_arguments.delta") => {
                if let (Some(delta), Some(call_id)) =
                    (string(event.get("delta")), self.call_for(event))
                {
                    self.events.push(StreamEvent::ToolArgumentsDelta {
                        call_id,
                        delta: delta.to_owned(),
                    });
                }
            }
            Some("response.output_item.done") => {
                if let Some(item) = event.get("item") {
                    match self.output_item(item) {
                        Ok(decoded) => {
                            if matches!(decoded, Item::Opaque { .. }) {
                                self.unmodelled.push(decoded.clone());
                            }
                            if let Err(error) = self.push_streamed(decoded) {
                                self.defer(error);
                            }
                        }
                        Err(error) => self.defer(error),
                    }
                }
            }
            Some(name @ ("response.completed" | "response.incomplete")) => {
                self.terminal = Some(Terminal {
                    incomplete: name == "response.incomplete",
                    response: event.get("response").cloned().unwrap_or(Value::Null),
                });
            }
            Some("response.failed") => {
                return Err(self.provider_failure(event.get("response"), event));
            }
            Some("error") => return Err(self.provider_failure(None, event)),
            // Progress markers the decoder does not act on; the terminal object is authoritative.
            // The reasoning `.done` and `part` markers of both families are here rather than
            // warned about because each repeats text a delta already carried.
            Some(name) if ACCEPTED_STREAM_EVENTS.contains(&name) => {}
            _ => {
                // Preserved, not dropped, and reported. A dropped event is a hole in the
                // conversation that the next turn cannot see.
                self.events.push(StreamEvent::Warning {
                    code: "unknown-stream-event".to_owned(),
                    message:
                        "a stream event outside the pinned subset was preserved, not interpreted"
                            .to_owned(),
                });
                let item = self.opaque(event);
                self.unmodelled.push(item.clone());
                if let Err(error) = self.push_streamed(item) {
                    self.defer(error);
                }
            }
        }
        Ok(())
    }

    fn finish(mut self) -> StreamDecoding {
        let result = self.terminate();
        StreamDecoding {
            events: self.events,
            result,
        }
    }

    /// Holds the first streamed-item refusal. A later one does not displace it: the first is the
    /// one that says why the turn stopped being decodable.
    fn defer(&mut self, error: Error) {
        if self.deferred.is_none() {
            self.deferred = Some(error);
        }
    }

    fn terminate(&mut self) -> Result<TurnOutcome, Error> {
        let deferred = self.deferred.take();
        let Some(terminal) = self.terminal.take() else {
            // Not a sentinel and not end of stream: this wire's terminal truth is an object, and
            // its absence is a stream that stopped, with dispatch left genuinely unknown. A
            // deferred refusal is the root cause and outranks the missing terminal object, which
            // is its consequence; either way only the binding is known.
            return Err(self.bound(deferred.unwrap_or_else(|| {
                Error::protocol("the stream ended before the response reached a terminal state")
                    .with_dispatch(Dispatch::Unknown)
            })));
        };
        // **Read the evidence before anything can refuse it away.** Every refusal below belongs
        // to a response whose counters the provider already reported, and one that returned
        // without them would throw away the only record of what the attempt cost. The single
        // exception is the line above this one: when the counters are themselves contradictory
        // there is no valid evidence to attach, and attaching invalid evidence is worse than none.
        let observation = self.observation(&terminal.response, true)?;
        let retain = |error: Error| error.with_observation(observation.clone());
        // Raised earlier in the stream, answered here, carrying the evidence the terminal object
        // reported. This is the whole reason it waited.
        if let Some(error) = deferred {
            return Err(retain(error));
        }
        // The terminal object is authoritative when it carries output; the streamed items are the
        // fallback for a server that reports completion without repeating them, whether it omits
        // `output` or, as the Codex backend does, sends it empty after streaming the items.
        let mut items = match terminal.response.get("output").and_then(Value::as_array) {
            Some(output) if !output.is_empty() || self.streamed.is_empty() => {
                let mut decoded = Vec::with_capacity(output.len());
                for value in output {
                    decoded.push(self.output_item(value).map_err(&retain)?);
                }
                decoded
            }
            _ => std::mem::take(&mut self.streamed),
        };
        for item in std::mem::take(&mut self.unmodelled) {
            if !items.contains(&item) {
                items.push(item);
            }
        }
        if items.len() > MAX_ITEMS {
            return Err(retain(
                Error::too_large("model output exceeds its bound")
                    .with_dispatch(Dispatch::Accepted),
            ));
        }
        // The caller already saw each announcement. An outcome that carries the call under
        // another name, or under another identifier than the one its arguments streamed under,
        // would hand it a call it was never shown; the refusal keeps the terminal counters.
        let contradicted = self.announced.iter().any(|(call_id, name)| {
            !items
                .iter()
                .filter_map(Item::as_tool_call)
                .any(|call| &call.call_id == call_id && &call.name == name)
        });
        if contradicted {
            return Err(retain(protocol(
                "a finished function call contradicts its announcement",
            )));
        }
        let has_tool_calls = items.iter().any(|item| item.as_tool_call().is_some());
        Ok(TurnOutcome {
            stop_reason: stop_reason(&terminal, has_tool_calls),
            items,
            observation,
        })
    }

    /// Attaches the only evidence there is when nothing terminal has arrived: the binding itself.
    ///
    /// A refusal with no observation says nothing about which serving model it came from, and a
    /// caller deciding whether the attempt may be retried needs that even when no counter is
    /// known.
    fn bound(&self, error: Error) -> Error {
        error.with_observation(TurnObservation::new(self.binding.provenance().clone()))
    }

    /// The evidence one terminal or failed response object reports.
    ///
    /// Every counter and identifier is independently optional and an absent one stays absent. The
    /// configured model never fills in a missing reported one: the caller already knows what it
    /// asked for, and repeating it here would turn a request into an observation.
    fn observation(&self, response: &Value, final_usage: bool) -> Result<TurnObservation, Error> {
        let observation = self.evidence(response, final_usage);
        observation
            .validate_for(self.binding.provenance())
            .map_err(|error| error.with_dispatch(Dispatch::Accepted))?;
        Ok(observation)
    }

    /// The evidence one response object reports, before it is judged.
    fn evidence(&self, response: &Value, final_usage: bool) -> TurnObservation {
        let mut observation = TurnObservation::new(self.binding.provenance().clone());
        observation.upstream_model = identifier(response.get("model"));
        observation.response_id = identifier(response.get("id"));
        observation.usage = usage(response);
        observation.final_usage = final_usage;
        observation
    }

    /// A typed refusal from the provider's own failure, carrying no text the provider wrote.
    ///
    /// Only the machine-readable code is read, and only to choose between three classes. The
    /// message is this crate's own: an upstream string relayed into a diagnostic is how a prompt
    /// or a credential eventually reaches a log.
    fn provider_failure(&self, response: Option<&Value>, event: &Value) -> Error {
        let source = response.unwrap_or(event);
        let code = source
            .get("error")
            .and_then(|error| string(error.get("code")))
            .or_else(|| string(source.get("code")));
        let (class, message) = match code {
            Some("server_error") => (
                ErrorCode::Unavailable,
                "the provider failed this response on its own account",
            ),
            Some("rate_limit_exceeded") => (
                ErrorCode::RateLimited,
                "the provider asked for less traffic",
            ),
            _ => (ErrorCode::Refused, "the provider refused this request"),
        };
        let error = Error::new(class, message).with_dispatch(Dispatch::Accepted);
        // **Classified first, evidence attached second.** The provider said what kind of failure
        // this was, and that answer is kept; counters that disagree with themselves narrow what
        // the refusal can carry rather than overwriting it. Reporting a rate limit as a
        // non-retriable protocol error, because its usage block happened to be inconsistent,
        // ends a run on somebody else's temporary state.
        //
        // A failed response is terminal for the attempt, so whatever counters it reported are
        // terminal too. A top-level `error` event reports none and says so.
        let mut observation = self.evidence(source, response.is_some());
        if observation
            .usage
            .as_ref()
            .is_some_and(|usage| usage.validate().is_err())
        {
            // Only the invalid part is dropped: the binding, the upstream model and the response
            // id are still what was observed.
            observation.usage = None;
        }
        error.with_observation(observation)
    }

    fn output_item(&mut self, value: &Value) -> Result<Item, Error> {
        match value.get("type").and_then(Value::as_str) {
            Some("message") => message_item(value),
            Some("function_call") => Self::function_call(value),
            // Reasoning is modelled as opaque on purpose: it belongs to this binding and returns
            // to it unchanged, whether the provider encrypted it or sent it in the clear.
            Some("reasoning") => Ok(self.opaque(value)),
            _ => {
                self.events.push(StreamEvent::Warning {
                    code: "unknown-output-item".to_owned(),
                    message:
                        "an output item outside the pinned subset was preserved, not interpreted"
                            .to_owned(),
                });
                Ok(self.opaque(value))
            }
        }
    }

    fn function_call(value: &Value) -> Result<Item, Error> {
        let call_id = CallId::new(
            string(value.get("call_id"))
                .ok_or_else(|| protocol("a function call arrived without a call id"))?,
        )
        .map_err(|_| protocol("a function call carries an invalid call id"))?;
        let name = ToolName::new(
            string(value.get("name"))
                .ok_or_else(|| protocol("a function call arrived without a name"))?,
        )
        .map_err(|_| protocol("a function call carries an invalid tool name"))?;
        let raw = string(value.get("arguments"))
            .ok_or_else(|| protocol("a function call arrived without arguments"))?;
        // A half-parsed argument blob must never reach a caller: it would act on a value the
        // model did not send.
        let arguments: Value = serde_json::from_str(raw)
            .map_err(|_| protocol("function call arguments are not JSON"))?;
        if !arguments.is_object() {
            return Err(protocol("function call arguments are not a JSON object"));
        }
        if exceeds(&arguments, MAX_TOOL_ARGUMENT_BYTES) {
            return Err(Error::too_large("model tool arguments exceed their bound")
                .with_dispatch(Dispatch::Accepted));
        }
        Ok(Item::ToolCall(ToolCall {
            call_id,
            name,
            arguments,
        }))
    }

    fn push_streamed(&mut self, item: Item) -> Result<(), Error> {
        if self.streamed.len() >= MAX_ITEMS {
            return Err(Error::too_large("model output exceeds its bound")
                .with_dispatch(Dispatch::Accepted));
        }
        self.streamed.push(item);
        Ok(())
    }

    fn opaque(&self, payload: &Value) -> Item {
        Item::Opaque {
            provenance: self.binding.provenance().clone(),
            payload: payload.clone(),
        }
    }

    fn remember_call(&mut self, event: &Value) {
        let Some(item) = event.get("item") else {
            return;
        };
        if string(item.get("type")) != Some("function_call") {
            return;
        }
        let (Some(item_id), Some(call_id), Some(name)) = (
            string(item.get("id")),
            string(item.get("call_id")),
            string(item.get("name")),
        ) else {
            return;
        };
        // The opening item is where this wire names the call. One it cannot name is not
        // announced, and its argument deltas are then not relayed either: a fragment for a
        // call the caller was never told about has no call to belong to. The finished item
        // or the terminal object still decides, and refuses, the call itself.
        let (Ok(call_id), Ok(name)) = (CallId::new(call_id), ToolName::new(name)) else {
            return;
        };
        // Once per call: a repeated opening item, or a second item under a call id already
        // announced, announces nothing and relays nothing. The terminal check still decides.
        if self.calls.contains_key(item_id)
            || self
                .announced
                .iter()
                .any(|(announced, _)| announced == &call_id)
        {
            return;
        }
        self.calls.insert(item_id.to_owned(), call_id.clone());
        self.announced.push((call_id.clone(), name.clone()));
        self.events
            .push(StreamEvent::ToolCallStarted { call_id, name });
    }

    fn call_for(&self, event: &Value) -> Option<CallId> {
        string(event.get("item_id"))
            .and_then(|item_id| self.calls.get(item_id))
            .cloned()
    }
}

/// A protocol refusal observed after the request was accepted, not before it was sent.
fn protocol(message: &'static str) -> Error {
    Error::protocol(message).with_dispatch(Dispatch::Accepted)
}

/// One assistant message, or a refusal when any of its content is outside this version.
///
/// Filtering the parts it understands and keeping the rest was the earlier shape, and it turned a
/// model `refusal` part into an assistant message with no text and an `EndTurn` beside it: the
/// caller is told the model answered and shown nothing. `docs/responses.md` promises `Unsupported`
/// for content this version does not carry **on either side**, and this is the other side. The
/// refusal carries the terminal object's own counters, so nothing about the attempt is lost with
/// it.
fn message_item(value: &Value) -> Result<Item, Error> {
    let parts = value
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| protocol("an output message has no content array"))?;
    let mut text = String::new();
    for part in parts {
        match string(part.get("type")) {
            Some("output_text" | "text") => text.push_str(
                string(part.get("text")).ok_or_else(|| protocol("a content part has no text"))?,
            ),
            _ => {
                return Err(
                    Error::unsupported("message content is outside the pinned subset")
                        .with_dispatch(Dispatch::Accepted),
                );
            }
        }
    }
    Ok(Item::assistant(text))
}

/// Why the turn stopped, read from the terminal event and the object it delivered.
fn stop_reason(terminal: &Terminal, has_tool_calls: bool) -> StopReason {
    let response = &terminal.response;
    if terminal.incomplete || string(response.get("status")) == Some("incomplete") {
        let reason = response
            .get("incomplete_details")
            .and_then(|details| string(details.get("reason")));
        return match reason {
            Some("max_output_tokens") => StopReason::MaxOutputTokens,
            Some(other) if is_safe_reason(other) => StopReason::Incomplete {
                reason: other.to_owned(),
            },
            // The reason is a provider code, not prose. One that is not shaped like a code is
            // named as unrecognized rather than relayed into a value callers compare on.
            Some(_) => StopReason::Incomplete {
                reason: "unrecognized".to_owned(),
            },
            None => StopReason::Incomplete {
                reason: "unspecified".to_owned(),
            },
        };
    }
    if has_tool_calls {
        StopReason::ToolCalls
    } else {
        StopReason::EndTurn
    }
}

fn is_safe_reason(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// Reported token counts, each independently optional.
///
/// This wire reports no cache-write figure at all, so `cache_creation_input_tokens` stays absent
/// rather than zero: a zero would be this crate claiming, on the provider's behalf, that nothing
/// was written to its cache.
fn usage(response: &Value) -> Option<Usage> {
    let reported = response.get("usage")?.as_object()?;
    Some(Usage {
        input_tokens: count(reported.get("input_tokens")),
        output_tokens: count(reported.get("output_tokens")),
        cached_input_tokens: count(
            reported
                .get("input_tokens_details")
                .and_then(|details| details.get("cached_tokens")),
        ),
        cache_creation_input_tokens: None,
        reasoning_output_tokens: count(
            reported
                .get("output_tokens_details")
                .and_then(|details| details.get("reasoning_tokens")),
        ),
    })
}

fn count(value: Option<&Value>) -> Option<u64> {
    value.and_then(Value::as_u64)
}

fn identifier(value: Option<&Value>) -> Option<Id> {
    string(value).and_then(|value| Id::new(value).ok())
}

fn string(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}
