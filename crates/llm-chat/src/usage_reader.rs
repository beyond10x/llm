//! The model and the counters a Chat Completions answer reported, read from its bytes.
//!
//! The reading is the turn decoder's own ([`crate::incoming`]): each chunk's or body's report is
//! read by the same `read_reported`, a stream's report that ends it is told apart by the same
//! `finishes`, and the counters, llama-server's `timings` included, are joined by the same
//! `Reported::counters`. The reader cannot report a counter the decoder would not, or read one
//! differently.

use llm_core::ReportedUsage;
use llm_http::{AnswerBody, Framing, SseEvent};
use serde_json::Value;

use crate::incoming::{Reported, finishes, read_reported};

/// Reads the usage one Chat Completions answer reported from its bytes, fed in pieces split
/// anywhere.
///
/// The answer is a stream of server-sent events ending in `data: [DONE]`, or, when the request did
/// not stream, one completion; [`llm_http::AnswerBody`] tells them apart by the first byte that is
/// not white space. In a stream every chunk may name the model; the counters come from the usage
/// chunk, and llama-server's `timings` only from the report that ends the stream, as for a
/// decoded turn. The sentinel makes them final; nothing after it is read. Every counter the
/// answer did not report stays `None`. A report the decoder would refuse leaves the counters as
/// they were, and counters that contradict each other (a subset over its total) are not taken.
///
/// Reading never fails: a malformed or cut answer reports what was read before it.
pub struct UsageReader {
    body: AnswerBody,
    reported: Reported,
    /// A chunk carried a finish reason: its report, and every one after it, ends the stream.
    finishing: bool,
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
            body: AnswerBody::new(Framing::DoneSentinel),
            reported: Reported::default(),
            finishing: false,
            final_usage: false,
        }
    }

    /// Reads the answer's next bytes.
    pub fn feed(&mut self, bytes: &[u8]) {
        for event in self.body.push(bytes) {
            match event {
                SseEvent::Done => self.final_usage = true,
                SseEvent::Payload { data, .. } => self.chunk(&data),
            }
        }
    }

    /// What the answer reported, once all of its bytes were fed.
    pub fn finish(mut self) -> ReportedUsage {
        if let Some(completion) = self.body.finish() {
            // A body that is not streamed is one report, and it ends the response.
            let _ = read_reported(&completion, &mut self.reported, true);
            self.final_usage = true;
        }
        ReportedUsage {
            model: self.reported.upstream_model.clone(),
            usage: self
                .reported
                .counters()
                .filter(|counters| counters.validate().is_ok())
                .unwrap_or_default(),
            final_usage: self.final_usage,
        }
    }

    fn chunk(&mut self, chunk: &Value) {
        if self.final_usage {
            return;
        }
        self.finishing |= finishes(chunk);
        // A refused report leaves what was read before it; the reader goes on.
        let _ = read_reported(chunk, &mut self.reported, self.finishing);
    }
}

/// The usage a whole Chat Completions answer reported: [`UsageReader`] fed `body` at once.
pub fn read_usage(body: &[u8]) -> ReportedUsage {
    let mut reader = UsageReader::new();
    reader.feed(body);
    reader.finish()
}
