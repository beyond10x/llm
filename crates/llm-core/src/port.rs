use crate::{
    CallId, Cancel, Capabilities, Error, Provenance, ToolName, TurnOutcome, TurnRequest,
    encoded_len,
};
use serde::{Deserialize, Serialize};
use std::{future::Future, pin::Pin};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

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
    Warning {
        code: String,
        message: String,
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

#[cfg(test)]
mod tests {
    use super::StreamEvent;
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
}
