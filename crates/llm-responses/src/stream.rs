//! One streamed response, decoded into neutral items, counters and terminal truth.
//!
//! The input is the ordered sequence of decoded server-sent-event payloads. Framing them out of
//! a byte stream is `llm-http`'s `SseDecoder`; this module never sees a byte boundary, which is
//! why a route that ends without the `data: [DONE]` sentinel — vLLM v0.27.1 sends none — is
//! decided here by the terminal response object and not by the sentinel.

use std::collections::BTreeMap;

use llm_core::{
    CallId, Dispatch, Error, ErrorCode, Id, Item, MAX_ITEMS, MAX_TOOL_ARGUMENT_BYTES, RefusalCause,
    SHOWN_TEXT_KEPT, SHOWN_TEXT_UNPAIRED, StopReason, StreamEvent, ToolCall, ToolName,
    TurnObservation, TurnOutcome, Usage, exceeds,
};
use serde_json::Value;

use crate::Binding;

/// Payloads that have not answered the turn. Every other payload, including a silent opening
/// item, ends retry eligibility for subsequent failures in both the decoder and live client.
const LIFECYCLE_EVENTS: &[&str] = &[
    "keepalive",
    "response.created",
    "response.in_progress",
    "response.queued",
];

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
    "response.metadata",
    "response.output_item.added",
    "response.output_item.done",
    "response.output_text.delta",
    "response.output_text.done",
    "response.queued",
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
///
/// This is the incremental decoder the client drives payload by payload, fed the whole
/// sequence at once: the events and the result are the ones a live caller would get. Like the
/// client, it reads nothing after the first terminal object.
///
/// It checks the turn against no request, so every turn it returns is one the client would hand
/// over unless that check refused it. A turn whose shown text replaced a server's own text, or
/// could not be paired with one message, ends its events with the `shown-text-kept` or
/// `shown-text-unpaired` warning, where the client emits it once that check has passed.
pub fn decode_stream(binding: &Binding, payloads: &[Value]) -> StreamDecoding {
    let mut decoder = Decoder::new(binding);
    for payload in payloads {
        if let Err(error) = decoder.apply(payload) {
            return StreamDecoding {
                events: decoder.events,
                result: Err(error),
            };
        }
        if decoder.is_terminal() {
            break;
        }
    }
    let (mut decoding, notices) = decoder.finish();
    decoding.events.extend(notices);
    decoding
}

/// The warning that a returned turn carries the text the caller was shown where the server's own
/// final text for that message differed. Fixed: it quotes neither text.
fn shown_text_kept() -> StreamEvent {
    StreamEvent::Warning {
        code: SHOWN_TEXT_KEPT.to_owned(),
        message: "a message's final text differed from the text shown for it; the shown text was \
                  kept"
            .to_owned(),
    }
}

/// The warning that text the caller was shown could be more than one of the turn's messages, so
/// it is carried in none and the server's messages are as sent. Fixed: it quotes no text.
fn shown_text_unpaired() -> StreamEvent {
    StreamEvent::Warning {
        code: SHOWN_TEXT_UNPAIRED.to_owned(),
        message: "text was shown that more than one message could be; the server's messages were \
                  kept as sent"
            .to_owned(),
    }
}

/// One stream decoded payload by payload.
///
/// [`Decoder::apply`] reads one payload and [`Decoder::take_events`] hands over the events it
/// produced, so a caller can be shown each one before the next payload is read. A refusal from
/// `apply` ends the stream; [`Decoder::finish`] decides the turn from what was read.
pub(crate) struct Decoder<'a> {
    binding: &'a Binding,
    answered: bool,
    events: Vec<StreamEvent>,
    /// Text the caller was shown through `response.output_text.delta`, one entry per output item
    /// the deltas named, in order of first delta. Each is the text of the message the turn
    /// carries for that item, or a message of its own when the turn carries none (row R36).
    shown: Vec<ShownText>,
    /// Where every message among `streamed` sits, in order.
    streamed_messages: Vec<Placed>,
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

/// The text deltas of one output item, as the caller was shown them.
struct ShownText {
    item_id: Option<String>,
    /// The `output_index` its first delta named.
    output_index: Option<u64>,
    /// For deltas that named neither an item nor an index: the streamed message whose
    /// `response.output_item.done` ended them, by its place among the streamed messages.
    ended_by: Option<usize>,
    text: String,
}

/// Where one message the turn carries sits, by every coordinate the wire gave it.
struct Placed {
    /// The `id` the message names.
    id: Option<String>,
    /// Which streamed message it is, in `response.output_item.done` order, when it is one.
    streamed: Option<usize>,
    /// The `output_index` its `response.output_item.done` named.
    stream_index: Option<u64>,
    /// Its position in the terminal `output`, kept only when no streamed message is this one. A
    /// terminal `output` can hold items the stream never showed (Harness's emulator completes
    /// with a reasoning item ahead of a message it streamed at index 0), so a position is a
    /// weaker coordinate than an index the stream named.
    position: Option<u64>,
}

/// One message the turn carries, as pairing sees it.
struct Message<'p> {
    /// Where it sits in the turn's items.
    at: usize,
    placed: &'p Placed,
    text: String,
}

/// One way shown text and a message are found to be the same output item.
type Pairing = fn(&ShownText, &Message<'_>) -> bool;

impl ShownText {
    /// The pairings, strongest first: each is tried for every shown text before the next. What
    /// none of them pairs is decided by elimination ([`ShownText::could_be`]).
    const PAIRINGS: [Pairing; 5] = [
        Self::same_id,
        Self::ended_by_it,
        Self::same_stream_index,
        Self::same_position,
        Self::same_text,
    ];

    /// Both name the same `id`.
    fn same_id(&self, message: &Message<'_>) -> bool {
        self.item_id.is_some() && self.item_id == message.placed.id
    }

    /// Deltas naming no coordinate, ended by this message's `response.output_item.done`.
    fn ended_by_it(&self, message: &Message<'_>) -> bool {
        self.ended_by.is_some()
            && self.ended_by == message.placed.streamed
            && !self.ids_differ(message)
    }

    /// The deltas and the message's `response.output_item.done` named the same `output_index`.
    fn same_stream_index(&self, message: &Message<'_>) -> bool {
        self.output_index.is_some()
            && self.output_index == message.placed.stream_index
            && !self.ids_differ(message)
    }

    /// The deltas named the `output_index` the message sits at in the terminal `output`.
    fn same_position(&self, message: &Message<'_>) -> bool {
        self.output_index.is_some()
            && self.output_index == message.placed.position
            && !self.ids_differ(message)
    }

    /// The message already carries exactly the shown text, and no coordinate the stream named
    /// tells them apart. Pairing them changes nothing, and not pairing them would carry the text
    /// twice.
    fn same_text(&self, message: &Message<'_>) -> bool {
        self.text == message.text && !self.ids_differ(message) && !self.streams_differ(message)
    }

    /// No coordinate both sides name tells them apart.
    fn could_be(&self, message: &Message<'_>) -> bool {
        let positions = matches!(
            (self.output_index, message.placed.position),
            (Some(shown), Some(position)) if shown != position
        );
        !self.ids_differ(message) && !self.streams_differ(message) && !positions
    }

    /// Two different ids are two items.
    fn ids_differ(&self, message: &Message<'_>) -> bool {
        matches!((&self.item_id, &message.placed.id), (Some(shown), Some(id)) if shown != id)
    }

    /// Two different indexes the stream named, or two different streamed messages, are two
    /// items.
    fn streams_differ(&self, message: &Message<'_>) -> bool {
        let indexes = matches!(
            (self.output_index, message.placed.stream_index),
            (Some(shown), Some(index)) if shown != index
        );
        let ended = matches!(
            (self.ended_by, message.placed.streamed),
            (Some(shown), Some(streamed)) if shown != streamed
        );
        indexes || ended
    }
}

/// Gives each terminal message the coordinates of the streamed message it is: the one under the
/// same `id`, and otherwise, only when as many terminal messages as streamed messages are left,
/// the next one in order whose `id` does not contradict it. A message that is a streamed one
/// drops its position: the index the stream named outranks where the terminal `output` put it.
fn adopt_streamed(carried: &mut [Placed], streamed: &[Placed]) {
    let mut used = vec![false; streamed.len()];
    let adopt = |message: &mut Placed, index: usize, used: &mut [bool]| {
        used[index] = true;
        message.streamed = streamed[index].streamed;
        message.stream_index = streamed[index].stream_index;
        message.position = None;
    };
    for message in carried.iter_mut() {
        let same = streamed.iter().enumerate().position(|(index, other)| {
            !used[index] && message.id.is_some() && other.id == message.id
        });
        if let Some(index) = same {
            adopt(message, index, &mut used);
        }
    }
    let left = carried.iter().filter(|m| m.streamed.is_none()).count();
    if left == 0 || left != used.iter().filter(|used| !**used).count() {
        return;
    }
    let mut next = 0;
    for message in carried.iter_mut().filter(|m| m.streamed.is_none()) {
        while next < streamed.len()
            && (used[next]
                || matches!((&message.id, &streamed[next].id), (Some(a), Some(b)) if a != b))
        {
            next += 1;
        }
        if next < streamed.len() {
            adopt(message, next, &mut used);
            next += 1;
        }
    }
}

impl<'a> Decoder<'a> {
    pub(crate) fn new(binding: &'a Binding) -> Self {
        Self {
            binding,
            answered: false,
            events: Vec::new(),
            shown: Vec::new(),
            calls: BTreeMap::new(),
            announced: Vec::new(),
            streamed: Vec::new(),
            streamed_messages: Vec::new(),
            unmodelled: Vec::new(),
            terminal: None,
            deferred: None,
        }
    }

    /// Reads one payload. A refusal ends the stream: nothing after it is read.
    pub(crate) fn apply(&mut self, event: &Value) -> Result<(), Error> {
        let kind = event.get("type").and_then(Value::as_str);
        match kind {
            Some("response.output_text.delta") => {
                if let Some(text) = string(event.get("delta")) {
                    self.show(event, text);
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
                            if matches!(decoded, Item::AssistantText { .. }) {
                                self.end_message(event, item);
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
            // warned about because each repeats text a delta already carried. `response.metadata`
            // (the Codex backend, Harness wave D200) describes the response and is not output:
            // kept as an opaque item it was replayed as an input entry the endpoint refuses.
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
        self.answered |= !kind.is_some_and(|kind| LIFECYCLE_EVENTS.contains(&kind));
        Ok(())
    }

    /// Whether any prior payload produced output, including output with no sink event.
    pub(crate) const fn has_answered(&self) -> bool {
        self.answered
    }

    /// Whether a terminal object has been read; nothing after it belongs to the turn.
    pub(crate) const fn is_terminal(&self) -> bool {
        self.terminal.is_some()
    }

    /// The events produced since the last call, in the order they were produced.
    pub(crate) fn take_events(&mut self) -> Vec<StreamEvent> {
        std::mem::take(&mut self.events)
    }

    /// Decides the turn from everything read. The events are the ones produced since the last
    /// [`Decoder::take_events`], deciding the turn included.
    ///
    /// The notices are what the caller is to be told about the returned turn's text
    /// ([`Decoder::keep_shown_text`]). They are not events here: a caller is told about a turn
    /// only once it is handed it, so the client emits them after checking the turn against the
    /// request, as it emits the advisory warning, and a refused turn has none.
    pub(crate) fn finish(mut self) -> (StreamDecoding, Vec<StreamEvent>) {
        let (result, notices) = match self.terminate() {
            Ok((outcome, notices)) => (Ok(outcome), notices),
            Err(error) => (Err(error), Vec::new()),
        };
        let decoding = StreamDecoding {
            events: self.events,
            result,
        };
        (decoding, notices)
    }

    /// Records text the caller is being shown, under the output item the delta names: its
    /// `item_id`, or, for a delta that names no item, its `output_index`. Deltas at two indexes
    /// are two items whether or not they name one. Deltas that name neither belong to the
    /// message whose `response.output_item.done` comes next ([`Decoder::end_message`]).
    fn show(&mut self, event: &Value, text: &str) {
        let item_id = string(event.get("item_id"));
        let output_index = event.get("output_index").and_then(Value::as_u64);
        if let Some(shown) = self
            .shown
            .iter_mut()
            .find(|shown| match (item_id, output_index) {
                (Some(_), _) => shown.item_id.as_deref() == item_id,
                (None, Some(_)) => shown.item_id.is_none() && shown.output_index == output_index,
                (None, None) => {
                    shown.item_id.is_none()
                        && shown.output_index.is_none()
                        && shown.ended_by.is_none()
                }
            })
        {
            shown.text.push_str(text);
            return;
        }
        self.shown.push(ShownText {
            item_id: item_id.map(str::to_owned),
            output_index,
            ended_by: None,
            text: text.to_owned(),
        });
    }

    /// Records a message `response.output_item.done` finished, and ends the shown text that
    /// named neither an item nor an index: those deltas were this message's.
    fn end_message(&mut self, event: &Value, item: &Value) {
        let streamed = self.streamed_messages.len();
        for shown in &mut self.shown {
            if shown.item_id.is_none() && shown.output_index.is_none() && shown.ended_by.is_none() {
                shown.ended_by = Some(streamed);
            }
        }
        self.streamed_messages.push(Placed {
            id: message_id(item),
            streamed: Some(streamed),
            stream_index: event.get("output_index").and_then(Value::as_u64),
            position: None,
        });
    }

    /// Gives each output item in the turn the text the caller was shown of it, and returns what
    /// the caller is to be told about the turn's text once it is handed the turn.
    ///
    /// A shown delta is final (`StreamEvent`): a caller may already have spoken it, so the turn
    /// it gets back carries what it was shown, not what the server finished with. This one pass
    /// covers both ways a server's message reaches `items`: in the terminal `output`, or streamed
    /// through `response.output_item.done` before an empty one. `carried` places every message
    /// in `items`, in order.
    ///
    /// Shown text is paired with exactly one message, by [`ShownText::PAIRINGS`] in order: the
    /// same `id`; deltas naming nothing, ended by the message's `response.output_item.done`; the
    /// `output_index` that `done` named; the message's position in the terminal `output`; then a
    /// message already carrying exactly the shown text. What none of them pairs is paired by
    /// elimination: with the one message no coordinate both sides name tells apart from it. A
    /// paired message takes the shown text, and one that carried other text is reported with
    /// `shown-text-kept`. A message no text was paired with keeps the server's text.
    ///
    /// The decoder never carries a message's text twice and never overwrites a message it cannot
    /// tell is the one shown. Shown text that more than one message could be is carried in none:
    /// the server's messages stay as they are and `shown-text-unpaired` says so. Shown text no
    /// message could be is kept as a message of its own (R36): a server that streams a message
    /// only as deltas and then completes with an empty `output`, as the Codex backend can, would
    /// otherwise hand the caller a turn without the answer it just watched arrive. Kept messages
    /// are placed by `output_index`, ascending and stable among equal indexes, whatever order
    /// their first deltas arrived in; one without an index goes after every other item.
    fn keep_shown_text(&mut self, items: &mut Vec<Item>, carried: &[Placed]) -> Vec<StreamEvent> {
        let shown: Vec<ShownText> = std::mem::take(&mut self.shown)
            .into_iter()
            .filter(|shown| !shown.text.is_empty())
            .collect();
        let messages: Vec<Message<'_>> = items
            .iter()
            .enumerate()
            .filter_map(|(at, item)| match item {
                Item::AssistantText { text } => Some((at, text.clone())),
                _ => None,
            })
            .zip(carried)
            .map(|((at, text), placed)| Message { at, placed, text })
            .collect();
        let mut taken = vec![false; messages.len()];
        let mut pairs: Vec<Option<usize>> = vec![None; shown.len()];
        for pairing in ShownText::PAIRINGS {
            for (pair, text) in pairs.iter_mut().zip(&shown) {
                if pair.is_none() {
                    *pair = (0..messages.len())
                        .find(|&index| !taken[index] && pairing(text, &messages[index]));
                    if let Some(index) = *pair {
                        taken[index] = true;
                    }
                }
            }
        }
        let mut unpaired = false;
        let mut kept = Vec::new();
        let mut paired = Vec::new();
        for (text, pair) in shown.into_iter().zip(pairs) {
            if let Some(index) = pair {
                paired.push((index, text));
                continue;
            }
            let could_be: Vec<usize> = (0..messages.len())
                .filter(|&index| !taken[index] && text.could_be(&messages[index]))
                .collect();
            match could_be.as_slice() {
                [] => kept.push(text),
                [index] => {
                    taken[*index] = true;
                    paired.push((*index, text));
                }
                _ => unpaired = true,
            }
        }
        let mut replaced = false;
        for (index, text) in paired {
            if let Some(Item::AssistantText { text: carried }) = items.get_mut(messages[index].at)
                && *carried != text.text
            {
                *carried = text.text;
                replaced = true;
            }
        }
        kept.sort_by_key(|shown| shown.output_index.unwrap_or(u64::MAX));
        for shown in kept {
            let at = shown
                .output_index
                .and_then(|index| usize::try_from(index).ok())
                .map_or(items.len(), |index| index.min(items.len()));
            items.insert(at, Item::assistant(shown.text));
        }
        let mut notices = Vec::new();
        if replaced {
            notices.push(shown_text_kept());
        }
        if unpaired {
            notices.push(shown_text_unpaired());
        }
        notices
    }

    /// Holds the first streamed-item refusal. A later one does not displace it: the first is the
    /// one that says why the turn stopped being decodable.
    fn defer(&mut self, error: Error) {
        if self.deferred.is_none() {
            self.deferred = Some(error);
        }
    }

    /// The turn, and what the caller is to be told about its text once it is handed the turn.
    fn terminate(&mut self) -> Result<(TurnOutcome, Vec<StreamEvent>), Error> {
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
        let (mut items, carried) = match terminal.response.get("output").and_then(Value::as_array) {
            Some(output) if !output.is_empty() || self.streamed.is_empty() => {
                let mut decoded = Vec::with_capacity(output.len());
                let mut carried = Vec::new();
                for (position, value) in output.iter().enumerate() {
                    let item = self.output_item(value).map_err(retain)?;
                    if matches!(item, Item::AssistantText { .. }) {
                        carried.push(Placed {
                            id: message_id(value),
                            streamed: None,
                            stream_index: None,
                            position: u64::try_from(position).ok(),
                        });
                    }
                    decoded.push(item);
                }
                adopt_streamed(&mut carried, &self.streamed_messages);
                (decoded, carried)
            }
            _ => (
                std::mem::take(&mut self.streamed),
                std::mem::take(&mut self.streamed_messages),
            ),
        };
        for item in std::mem::take(&mut self.unmodelled) {
            if !items.contains(&item) {
                items.push(item);
            }
        }
        let notices = self.keep_shown_text(&mut items, &carried);
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
        let outcome = TurnOutcome {
            stop_reason: stop_reason(&terminal, has_tool_calls),
            items,
            observation,
        };
        Ok((outcome, notices))
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
    /// Only the machine-readable code is read, to choose between three classes and to name a
    /// refusal's cause ([`refusal_cause`]). The message is this crate's own: an upstream string
    /// relayed into a diagnostic is how a prompt or a credential eventually reaches a log.
    ///
    /// The three codes the far side reports as its own temporary state are retriable before
    /// any output and final after it (`docs/design.md`, provider failures retried before
    /// output). Every other code is final, an exhausted quota (`insufficient_quota`) included:
    /// another identical attempt meets it again. A context-length refusal is final too, and
    /// names its cause.
    fn provider_failure(&self, response: Option<&Value>, event: &Value) -> Error {
        let source = response.unwrap_or(event);
        let code = source
            .get("error")
            .and_then(|error| string(error.get("code")))
            .or_else(|| string(source.get("code")));
        let (class, message, transient) = match code {
            Some("server_is_overloaded") => (
                ErrorCode::Unavailable,
                "the provider is temporarily overloaded",
                true,
            ),
            Some("server_error") => (
                ErrorCode::Unavailable,
                "the provider failed this response on its own account",
                true,
            ),
            Some("rate_limit_exceeded") => (
                ErrorCode::RateLimited,
                "the provider asked for less traffic",
                true,
            ),
            _ => (
                ErrorCode::Refused,
                "the provider refused this request",
                false,
            ),
        };
        let mut error = Error::new(class, message)
            .with_dispatch(Dispatch::Accepted)
            .with_retriable(transient && !self.answered);
        if let Some(cause) = refusal_cause(code) {
            error.message = format!("{message}: {}", cause.describe());
            error = error.with_refusal(cause);
        }
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

/// The `id` an output message names, which its text deltas name as `item_id`.
fn message_id(value: &Value) -> Option<String> {
    string(value.get("id")).map(str::to_owned)
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

pub(crate) fn is_safe_reason(value: &str) -> bool {
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

/// The cause a refusal's exact `error.code` names on this wire, before the stream as a 400 and
/// inside it: `context_length_exceeded` is a request over the model's context window, as the
/// official Codex CLI reads it (`openai/codex` 3d2ee51c,
/// `codex-rs/codex-api/src/sse/responses.rs:703`-`705`). Every other code, and that one in
/// another spelling, names none. The provider's prose is never read.
pub(crate) fn refusal_cause(code: Option<&str>) -> Option<RefusalCause> {
    match code {
        Some("context_length_exceeded") => Some(RefusalCause::ContextLengthExceeded),
        _ => None,
    }
}
