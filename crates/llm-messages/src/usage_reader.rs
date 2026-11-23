//! The model and the counters a Messages answer reported, read from its bytes.
//!
//! The reading is the turn decoder's own: a message's model and usage are read by the decoder's
//! `Header`, a `message_delta`'s cumulative usage by the same `Header` method the decoder uses,
//! and the counters normalized by the same `Snapshot`. The reader cannot report a counter the
//! decoder would not, or read one differently.

use llm_core::ReportedUsage;
use llm_http::{AnswerBody, Framing, SseEvent};
use serde_json::Value;

use crate::decode::{Header, START};

/// Reads the usage one Messages answer reported from its bytes, fed in pieces split anywhere.
///
/// The answer is a stream of server-sent events, or, when the request did not stream, one
/// message; [`llm_http::AnswerBody`] tells them apart by the first byte that is not white space.
/// In a stream, `message_start` names the model and opens the counters, each `message_delta`
/// carries them on cumulatively, and `message_stop` makes them final; nothing after it is read.
/// The input total is known only when the answer reported all three of its parts (uncached input,
/// cache read, cache write), as for a decoded turn. Every counter the answer did not report stays
/// `None`, and a usage report the decoder would refuse is not taken: the counters stay what they
/// were before it.
///
/// Reading never fails: a malformed or cut answer reports what was read before it.
pub struct UsageReader {
    body: AnswerBody,
    header: Header,
    final_usage: bool,
}

impl Default for UsageReader {
    fn default() -> Self {
        Self::new()
    }
}

impl UsageReader {
    /// A reader that has read nothing.
    pub fn new() -> Self {
        Self {
            body: AnswerBody::new(Framing::PayloadsOnly),
            header: Header::default(),
            final_usage: false,
        }
    }

    /// Reads the answer's next bytes.
    pub fn feed(&mut self, bytes: &[u8]) {
        for event in self.body.push(bytes) {
            if let SseEvent::Payload { data, .. } = event {
                self.event(&data);
            }
        }
    }

    /// What the answer reported, once all of its bytes were fed.
    pub fn finish(mut self) -> ReportedUsage {
        if let Some(message) = self.body.finish() {
            // A refused message keeps what was read before the refusal, as a decoded one does.
            let _ = self.header.read(&message, "", false);
            self.final_usage = true;
        }
        ReportedUsage {
            model: self.header.upstream_model.clone(),
            usage: self.header.usage().unwrap_or_default(),
            final_usage: self.final_usage,
        }
    }

    fn event(&mut self, event: &Value) {
        if self.final_usage {
            return;
        }
        // A refused report leaves the counters as they were; the reader goes on.
        match event.get("type").and_then(Value::as_str) {
            Some("message_start") => {
                if let Some(message) = event.get("message") {
                    let _ = self.header.read(message, START, true);
                }
            }
            Some("message_delta") => {
                let _ = self.header.read_delta(event);
            }
            Some("message_stop") => self.final_usage = true,
            _ => {}
        }
    }
}

/// The usage a whole Messages answer reported: [`UsageReader`] fed `body` at once.
pub fn read_usage(body: &[u8]) -> ReportedUsage {
    let mut reader = UsageReader::new();
    reader.feed(body);
    reader.finish()
}
