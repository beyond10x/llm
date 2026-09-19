use crate::{
    CallId, Cancel, Capabilities, Error, Provenance, TurnOutcome, TurnRequest, encoded_len,
};
use serde::{Deserialize, Serialize};
use std::{future::Future, pin::Pin};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum StreamEvent {
    TextDelta { text: String },
    ToolArgumentsDelta { call_id: CallId, delta: String },
    ReasoningDelta { text: String },
    Warning { code: String, message: String },
}

/// Awaited by the producer. Backpressure and sink refusal are observable, never dropped.
pub trait StreamSink: Send {
    fn emit(&mut self, event: StreamEvent) -> BoxFuture<'_, Result<(), Error>>;
}

/// An asynchronous, object-safe, single-attempt port. Implementations do not choose fallback.
pub trait Model: Send + Sync {
    fn provenance(&self) -> &Provenance;
    fn capabilities(&self) -> &Capabilities;
    fn turn<'a>(
        &'a self,
        request: &'a TurnRequest,
        sink: &'a mut dyn StreamSink,
        cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>>;
}

/// A bounded sink for embedded consumers. Failure leaves the already accepted prefix intact.
#[derive(Debug)]
pub struct VecSink {
    events: Vec<StreamEvent>,
    max_events: usize,
    remaining_bytes: usize,
}

impl VecSink {
    pub const fn new(max_events: usize, max_bytes: usize) -> Self {
        Self {
            events: Vec::new(),
            max_events,
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
            if self.events.len() >= self.max_events {
                return Err(Error::too_large("stream sink event bound reached"));
            }
            let size = encoded_len(&event, self.remaining_bytes)
                .ok_or_else(|| Error::too_large("stream sink byte bound reached"))?;
            self.remaining_bytes -= size;
            self.events.push(event);
            Ok(())
        })
    }
}
