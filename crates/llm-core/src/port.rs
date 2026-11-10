use crate::{
    CallId, Cancel, Capabilities, Error, Id, Provenance, ToolName, TurnOutcome, TurnRequest, Usage,
    encoded_len,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{future::Future, pin::Pin, time::Duration};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The code of the warning a decoder emits when a server's own final text for an assistant
/// message differed from the text deltas it streamed for it, and the shown text was kept.
pub const SHOWN_TEXT_KEPT: &str = "shown-text-kept";

/// The code of the warning a decoder emits when text the caller was shown could be more than one
/// of the turn's messages: it is carried in none, and the server's messages are as it sent them.
pub const SHOWN_TEXT_UNPAIRED: &str = "shown-text-unpaired";

/// What a turn shows its caller while it runs, in the order the wire produced it.
///
/// **Every event is final.** No later event retracts, replaces or reorders one already emitted,
/// and no variant can: a caller may act on each event as it arrives, speak a text delta or commit
/// it, and never has to take it back. On a successful turn each assistant message in the
/// [`TurnOutcome`] carries exactly the concatenation of the [`StreamEvent::TextDelta`]s shown for
/// it; a message no delta was shown for carries the text the server sent. Where a server's own
/// final text for a message differed from its deltas, the shown text is kept and a
/// [`StreamEvent::Warning`] with the code [`SHOWN_TEXT_KEPT`] says so. A decoder never carries a
/// message's text twice and never overwrites a message it cannot tell is the one shown: shown
/// text it cannot pair with exactly one message is carried in none, the server's messages stay as
/// sent, and [`SHOWN_TEXT_UNPAIRED`] says so. Either warning comes once the turn has passed its
/// checks and before the outcome is returned; a refused turn carries neither. Their messages are
/// fixed and quote no text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum StreamEvent {
    TextDelta {
        text: String,
    },
    /// A proposed call becomes visible before its arguments do. Every protocol in scope
    /// announces the tool name once, in the fragment that opens the call, and a consumer
    /// re-emitting the stream cannot name the call without it. A producer emits it once per
    /// call, before any [`StreamEvent::ToolArgumentsDelta`] for the same `call_id`.
    ToolCallStarted {
        call_id: CallId,
        name: ToolName,
    },
    ToolArgumentsDelta {
        call_id: CallId,
        delta: String,
    },
    ReasoningDelta {
        text: String,
    },
    /// Text in which the model declines to answer, on a wire that keeps it apart from the
    /// assistant text (Chat Completions `delta.refusal`). It never joins a [`StreamEvent::TextDelta`]
    /// and reaches the turn as [`crate::Item::AssistantRefusal`].
    RefusalDelta {
        text: String,
    },
    Warning {
        code: String,
        message: String,
    },
    /// The shown text moves to another assistant message: the next [`StreamEvent::TextDelta`]
    /// belongs to a message other than the one the text delta before it was shown for. A
    /// producer emits one each time the shown text changes message, so messages streamed one
    /// after another are separated by exactly one boundary, and a consumer re-emitting the stream
    /// (an ingress encoder) can place each shown text in the [`crate::Item::AssistantText`] it
    /// belongs to. Without a boundary, text deltas continue one message whatever is shown between
    /// them. A wire that interleaves the deltas of two messages (Responses pairs a delta with its
    /// item by `item_id`; no server is known to interleave) gives one boundary per change, so the
    /// text after a boundary then continues a message shown before it.
    ///
    /// **Not model output.** It carries nothing. A producer emits it only immediately before a
    /// text delta when text of another message of the turn was shown just before, so it never
    /// precedes a turn's first output event. Fallback eligibility and the first-output clock do
    /// not count it, and a [`VecSink`] stores it without charging it to either of its bounds. It
    /// has braces so that, as every other event, it refuses a field it does not have (a unit
    /// variant of this enum would read one and drop it).
    MessageBoundary {},
    /// An output item that arrived whole, with no delta that could show it, in its place among
    /// the streamed ones: provider state the turn carries as [`crate::Item::Opaque`] with exactly
    /// this provenance and payload (a Messages `redacted_thinking` block). A consumer
    /// re-emitting the wire that produced it places it here; any other consumer ignores it.
    ///
    /// It shows the caller no text, but it is model output: it ends fallback eligibility and
    /// starts the first-output clock as any delta does. A [`VecSink`] counts it as one event and
    /// does not charge its payload to its byte bound, which bounds shown text: the payload is the
    /// provider's state, which the outcome carries whole as well.
    OpaqueItem {
        provenance: Provenance,
        payload: Value,
    },
}

/// Awaited by the producer. Backpressure and sink refusal are observable, never dropped.
pub trait StreamSink: Send {
    fn emit(&mut self, event: StreamEvent) -> BoxFuture<'_, Result<(), Error>>;
}

/// An asynchronous, object-safe, single-attempt port. Implementations do not choose fallback.
pub trait Model: Send + Sync {
    fn provenance(&self) -> &Provenance;
    fn capabilities(&self) -> &Capabilities;
    /// Runs one attempt, emitting each [`StreamEvent`] into `sink` as the turn produces it.
    ///
    /// Every emitted event is final: nothing later in the turn retracts or replaces it. A
    /// successful outcome's assistant text is the text deltas shown for each message; a server's
    /// own text that differed is reported with [`SHOWN_TEXT_KEPT`], and shown text no single
    /// message could be with [`SHOWN_TEXT_UNPAIRED`], on the sink before this returns that
    /// outcome, never for a turn it refuses (see [`StreamEvent`]).
    fn turn<'a>(
        &'a self,
        request: &'a TurnRequest,
        sink: &'a mut dyn StreamSink,
        cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>>;
}

/// The most texts one embed request carries, all batches together.
pub const MAX_EMBED_TEXTS: usize = 16 * 1024;

/// The widest dimension count a vector may have, asked for or returned.
pub const MAX_EMBED_DIMENSIONS: usize = 64 * 1024;

/// Texts to embed and the bounds of one embed request.
///
/// `timeout` bounds the whole request, every batch, credential and read included; past it the
/// request fails `deadline`. `dimensions` asks for vectors of that length; absent leaves the
/// model's default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbedRequest {
    pub texts: Vec<String>,
    pub dimensions: Option<u32>,
    pub timeout: Duration,
}

impl EmbedRequest {
    pub const fn new(texts: Vec<String>, timeout: Duration) -> Self {
        Self {
            texts,
            dimensions: None,
            timeout,
        }
    }

    /// # Errors
    /// Refuses no text, an empty text, more than [`MAX_EMBED_TEXTS`] texts or texts past
    /// [`crate::MAX_REQUEST_BYTES`] together (`too-large`), zero or more than
    /// [`MAX_EMBED_DIMENSIONS`] dimensions, and a timeout of zero or past one day. No message
    /// quotes a text.
    pub fn validate(&self) -> Result<(), Error> {
        if self.texts.is_empty() || self.texts.iter().any(String::is_empty) {
            return Err(Error::invalid(
                "an embed request needs at least one text and no text may be empty",
            ));
        }
        let bytes = self
            .texts
            .iter()
            .try_fold(0_usize, |total, text| total.checked_add(text.len()));
        if self.texts.len() > MAX_EMBED_TEXTS
            || bytes.is_none_or(|bytes| bytes > crate::MAX_REQUEST_BYTES)
        {
            return Err(Error::too_large("embed request exceeds its text bounds"));
        }
        if self.dimensions.is_some_and(|dimensions| {
            dimensions == 0
                || !usize::try_from(dimensions).is_ok_and(|wide| wide <= MAX_EMBED_DIMENSIONS)
        }) {
            return Err(Error::invalid(
                "embed dimensions must be positive and within their bound",
            ));
        }
        if self.timeout.is_zero() || self.timeout > Duration::from_hours(24) {
            return Err(Error::invalid(
                "an embed timeout must be positive and at most one day",
            ));
        }
        Ok(())
    }
}

/// One vector per text of the request, in the request's order, all of one length.
#[derive(Debug, Clone, PartialEq)]
pub struct Embeddings {
    pub vectors: Vec<Vec<f32>>,
    /// Input tokens of every batch together; unknown when any batch reported none.
    pub usage: Usage,
    /// The model the far side reported; absent when it reported none.
    pub upstream_model: Option<Id>,
}

/// An asynchronous, object-safe embedding port: texts in, one vector per text out, in input
/// order. Implementations own the wire; this crate owns no transport.
pub trait Embedder: Send + Sync {
    /// Embeds every text of `request` within its timeout, or fails with a typed [`Error`]:
    /// `deadline` past the timeout, `cancelled` when `cancel` fires.
    fn embed<'a>(
        &'a self,
        request: &'a EmbedRequest,
        cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<Embeddings, Error>>;
}

/// A bounded sink for embedded consumers. Failure leaves the already accepted prefix intact.
///
/// The event bound counts the events that show the caller something or are model output, and
/// the byte bound charges each counted event's JSON encoding, except for two events:
///
/// - A [`StreamEvent::MessageBoundary`] is stored and charged to neither bound: it carries
///   nothing and is not output. A producer emits one only after text of another message was
///   shown, so the sink holds at most as many boundaries as counted events before them, and
///   refuses one past that `too-large`.
/// - A [`StreamEvent::OpaqueItem`] counts as one event, but its payload is not charged to the
///   byte bound, which bounds shown text.
#[derive(Debug)]
pub struct VecSink {
    events: Vec<StreamEvent>,
    max_events: usize,
    counted: usize,
    boundaries: usize,
    remaining_bytes: usize,
}

impl VecSink {
    pub const fn new(max_events: usize, max_bytes: usize) -> Self {
        Self {
            events: Vec::new(),
            max_events,
            counted: 0,
            boundaries: 0,
            remaining_bytes: max_bytes,
        }
    }
    pub fn events(&self) -> &[StreamEvent] {
        &self.events
    }
    pub fn text(&self) -> String {
        self.events
            .iter()
            .filter_map(|event| {
                if let StreamEvent::TextDelta { text } = event {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect()
    }
    pub fn into_events(self) -> Vec<StreamEvent> {
        self.events
    }
}
impl StreamSink for VecSink {
    fn emit(&mut self, event: StreamEvent) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            if matches!(event, StreamEvent::MessageBoundary {}) {
                if self.boundaries >= self.counted {
                    return Err(Error::too_large("stream sink event bound reached"));
                }
                self.boundaries += 1;
                self.events.push(event);
                return Ok(());
            }
            if self.counted >= self.max_events {
                return Err(Error::too_large("stream sink event bound reached"));
            }
            if !matches!(event, StreamEvent::OpaqueItem { .. }) {
                let size = encoded_len(&event, self.remaining_bytes)
                    .ok_or_else(|| Error::too_large("stream sink byte bound reached"))?;
                self.remaining_bytes -= size;
            }
            self.counted += 1;
            self.events.push(event);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{StreamEvent, StreamSink, VecSink};
    use crate::ErrorCode;
    use serde_json::json;

    #[test]
    fn a_call_announcement_carries_the_name_the_provider_sent() {
        let event: StreamEvent = serde_json::from_value(json!({
            "kind": "tool-call-started", "call_id": "call-1", "name": "lookup"
        }))
        .expect("an announcement is part of the stream vocabulary");
        let StreamEvent::ToolCallStarted { call_id, name } = &event else {
            panic!("decoded as another event: {event:?}");
        };
        assert_eq!((call_id.as_str(), name.as_str()), ("call-1", "lookup"));
        assert_eq!(
            serde_json::to_value(&event).expect("encodable"),
            json!({"kind": "tool-call-started", "call_id": "call-1", "name": "lookup"})
        );
    }

    #[test]
    fn an_announcement_without_a_usable_name_is_not_an_announcement() {
        for value in [
            json!({"kind": "tool-call-started", "call_id": "call-1"}),
            json!({"kind": "tool-call-started", "call_id": "call-1", "name": "has space"}),
            json!({"kind": "tool-call-started", "call_id": "call-1", "name": "lookup", "arguments": "{}"}),
        ] {
            assert!(serde_json::from_value::<StreamEvent>(value).is_err());
        }
    }

    #[test]
    fn a_message_boundary_and_an_opaque_item_have_their_own_kinds() {
        let boundary = json!({"kind": "message-boundary"});
        assert_eq!(
            serde_json::from_value::<StreamEvent>(boundary.clone()).expect("a boundary"),
            StreamEvent::MessageBoundary {}
        );
        assert_eq!(
            serde_json::to_value(StreamEvent::MessageBoundary {}).expect("encodable"),
            boundary
        );
        assert!(
            serde_json::from_value::<StreamEvent>(json!({"kind": "message-boundary", "text": "x"}))
                .is_err(),
            "a boundary carries nothing"
        );
        let opaque = json!({"kind": "opaque-item", "provenance": {
            "protocol": "messages", "provider": "p", "account": "a", "endpoint": "e",
            "model": "m", "binding_revision": "r"},
            "payload": {"type": "redacted_thinking", "data": "AAAA"}});
        let event: StreamEvent = serde_json::from_value(opaque.clone()).expect("an opaque item");
        assert!(matches!(event, StreamEvent::OpaqueItem { .. }));
        assert_eq!(serde_json::to_value(&event).expect("encodable"), opaque);
    }

    fn text(delta: &str) -> StreamEvent {
        StreamEvent::TextDelta {
            text: delta.to_owned(),
        }
    }

    fn opaque(data: &str) -> StreamEvent {
        serde_json::from_value(json!({"kind": "opaque-item", "provenance": {
            "protocol": "messages", "provider": "p", "account": "a", "endpoint": "e",
            "model": "m", "binding_revision": "r"},
            "payload": {"type": "redacted_thinking", "data": data}}))
        .expect("an opaque item")
    }

    #[tokio::test]
    async fn a_vec_sink_charges_a_boundary_to_neither_bound() {
        let mut sink = VecSink::new(2, 64);
        for event in [text("A"), StreamEvent::MessageBoundary {}, text("B")] {
            sink.emit(event)
                .await
                .expect("two counted events fit a bound of two");
        }
        assert_eq!(sink.events().len(), 3);
        let refused = sink
            .emit(text("C"))
            .await
            .expect_err("a third counted event");
        assert_eq!(refused.code, ErrorCode::TooLarge);
    }

    #[tokio::test]
    async fn a_vec_sink_holds_no_more_boundaries_than_counted_events_before_them() {
        let mut sink = VecSink::new(4, 1024);
        let first = sink.emit(StreamEvent::MessageBoundary {}).await;
        assert_eq!(
            first.expect_err("a boundary first").code,
            ErrorCode::TooLarge
        );
        sink.emit(text("A")).await.expect("text");
        sink.emit(StreamEvent::MessageBoundary {})
            .await
            .expect("one boundary after text");
        let second = sink.emit(StreamEvent::MessageBoundary {}).await;
        assert_eq!(
            second.expect_err("a second boundary").code,
            ErrorCode::TooLarge
        );
        assert_eq!(sink.events(), [text("A"), StreamEvent::MessageBoundary {}]);
    }

    #[tokio::test]
    async fn a_vec_sink_counts_an_opaque_item_as_an_event_but_not_its_payload_as_bytes() {
        let mut sink = VecSink::new(2, 64);
        sink.emit(opaque(&"A".repeat(4096)))
            .await
            .expect("a payload past the byte bound");
        sink.emit(text("Done."))
            .await
            .expect("shown text within the byte bound");
        let refused = sink.emit(opaque("A")).await.expect_err("a third event");
        assert_eq!(refused.code, ErrorCode::TooLarge);
        assert_eq!(sink.text(), "Done.");
    }
}
