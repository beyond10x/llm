//! The model and the counters a Responses answer reported, read from its bytes.
//!
//! The reading of one response object is the turn decoder's own ([`crate::stream`]): the same
//! function reads the counters and the same one the model, so the reader cannot report a counter
//! the decoder would not, or read one differently.

use llm_core::ReportedUsage;
use llm_http::{AnswerBody, Framing, SseEvent};
use serde_json::Value;

use crate::stream::{identifier, usage};

/// The events whose response object is terminal, and with it the counters it reports: the two
/// the decoder decides a turn from, and the failed response, whose counters are terminal too.
const TERMINAL_EVENTS: [&str; 3] = [
    "response.completed",
    "response.incomplete",
    "response.failed",
];

/// Reads the usage one Responses answer reported from its bytes, fed in pieces split anywhere.
///
/// The answer is a stream of server-sent events, or, when the request did not stream, one
/// response object; [`llm_http::AnswerBody`] tells them apart by the first byte that is not white
/// space. In a stream, every event that carries a response object (`response.created`,
/// `response.in_progress`, the terminal ones) may name the model, and the terminal one reports
/// the counters; nothing after a terminal event is read. The answer's own `model` is reported,
/// never a configured one; every counter it did not report stays `None`. Counters that contradict
/// each other (a subset over its total) are not taken.
///
/// Reading never fails: a malformed or cut answer reports what was read before it.
pub struct UsageReader {
    body: AnswerBody,
    report: ReportedUsage,
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
            report: ReportedUsage::default(),
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
    pub fn finish(self) -> ReportedUsage {
        let Self { body, mut report } = self;
        if let Some(response) = body.finish() {
            read_response(&mut report, &response);
            report.final_usage = true;
        }
        report
    }

    fn event(&mut self, event: &Value) {
        if self.report.final_usage {
            return;
        }
        if let Some(response) = event.get("response") {
            read_response(&mut self.report, response);
        }
        let kind = event.get("type").and_then(Value::as_str);
        self.report.final_usage = kind.is_some_and(|kind| TERMINAL_EVENTS.contains(&kind));
    }
}

/// Takes the model and the counters one response object reports into `report`, by the decoder's own
/// reading of them. A model or counters the object does not carry leave what was read before.
fn read_response(report: &mut ReportedUsage, response: &Value) {
    if let Some(model) = identifier(response.get("model")) {
        report.model = Some(model);
    }
    if let Some(counters) = usage(response).filter(|counters| counters.validate().is_ok()) {
        report.usage = counters;
    }
}

/// The usage a whole Responses answer reported: [`UsageReader`] fed `body` at once.
pub fn read_usage(body: &[u8]) -> ReportedUsage {
    let mut reader = UsageReader::new();
    reader.feed(body);
    reader.finish()
}
