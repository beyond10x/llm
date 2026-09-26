//! Incremental framing adapted from Harness's bounded reader; no blocking reader is retained.
use llm_core::{Dispatch, Error};
use serde_json::Value;

pub const MAX_EVENT_BYTES: usize = 1024 * 1024;
pub const MAX_STREAM_BYTES: usize = 32 * 1024 * 1024;
const MAX_EVENTS: usize = 65_536;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    DoneSentinel,
    PayloadsOnly,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SseEvent {
    Done,
    Payload { event: Option<String>, data: Value },
}

#[derive(PartialEq, Eq)]
enum State {
    Open,
    Done,
    Failed,
}

/// Bounded across chunk, line, event and whole-stream boundaries. EOF is never a terminal event.
pub struct SseDecoder {
    framing: Framing,
    line: Vec<u8>,
    data: Vec<u8>,
    event: Option<String>,
    saw_data: bool,
    skip_lf: bool,
    first_line: bool,
    consumed: usize,
    events: usize,
    state: State,
}

impl SseDecoder {
    pub const fn new(framing: Framing) -> Self {
        Self {
            framing,
            line: Vec::new(),
            data: Vec::new(),
            event: None,
            saw_data: false,
            skip_lf: false,
            first_line: true,
            consumed: 0,
            events: 0,
            state: State::Open,
        }
    }

    /// Returns frames in order, followed by a failure if present. Valid frames before a malformed
    /// frame remain observable independently of how the network split its chunks.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Result<SseEvent, Error>> {
        if self.state == State::Failed {
            return vec![Err(Error::protocol("SSE decoder previously failed"))];
        }
        let mut events = Vec::new();
        for &byte in bytes {
            match self.push_byte(byte) {
                Ok(Some(event)) => events.push(Ok(event)),
                Ok(None) => {}
                Err(error) => {
                    events.push(Err(error));
                    self.state = State::Failed;
                    break;
                }
            }
        }
        events
    }

    fn push_byte(&mut self, byte: u8) -> Result<Option<SseEvent>, Error> {
        self.consumed += 1;
        if self.consumed > MAX_STREAM_BYTES {
            return Err(Error::too_large("SSE stream exceeds byte bound"));
        }
        if self.skip_lf {
            self.skip_lf = false;
            if byte == b'\n' {
                return Ok(None);
            }
        }
        if byte == b'\r' || byte == b'\n' {
            self.skip_lf = byte == b'\r';
            return self.finish_line();
        }
        if self.line.len() >= MAX_EVENT_BYTES {
            return Err(Error::too_large("SSE line exceeds byte bound"));
        }
        self.line.push(byte);
        Ok(None)
    }

    fn finish_line(&mut self) -> Result<Option<SseEvent>, Error> {
        let mut line = std::mem::take(&mut self.line);
        if self.first_line {
            self.first_line = false;
            if line.starts_with(&[0xef, 0xbb, 0xbf]) {
                line.drain(..3);
            }
        }
        let line =
            std::str::from_utf8(&line).map_err(|_| Error::protocol("SSE line is not UTF-8"))?;
        if line.is_empty() {
            if !self.saw_data {
                self.event = None;
                return Ok(None);
            }
            if self.state == State::Done {
                return Err(Error::protocol("SSE payload follows terminal sentinel"));
            }
            self.events += 1;
            if self.events > MAX_EVENTS {
                return Err(Error::too_large("SSE event count exceeds bound"));
            }
            self.saw_data = false;
            let data = std::mem::take(&mut self.data);
            let event = self.event.take();
            if self.framing == Framing::DoneSentinel && data == b"[DONE]" {
                self.state = State::Done;
                return Ok(Some(SseEvent::Done));
            }
            let data = serde_json::from_slice(&data)
                .map_err(|_| Error::protocol("SSE data is not valid JSON"))?;
            return Ok(Some(SseEvent::Payload { event, data }));
        }
        if line.starts_with(':') {
            return Ok(None);
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "data" => {
                let length = self
                    .data
                    .len()
                    .saturating_add(value.len())
                    .saturating_add(usize::from(self.saw_data));
                if length > MAX_EVENT_BYTES {
                    return Err(Error::too_large("SSE event exceeds byte bound"));
                }
                if self.saw_data {
                    self.data.push(b'\n');
                }
                self.data.extend_from_slice(value.as_bytes());
                self.saw_data = true;
            }
            "event" => self.event = Some(value.to_owned()),
            _ => {}
        }
        Ok(None)
    }

    /// # Errors
    /// Refuses EOF inside a data frame. A clean EOF still requires the adapter's terminal proof.
    pub fn finish(&self) -> Result<(), Error> {
        if self.state == State::Failed {
            return Err(Error::protocol("SSE decoder previously failed"));
        }
        if self.saw_data || !self.line.is_empty() {
            return Err(Error::protocol("SSE stream ended inside an event")
                .with_dispatch(Dispatch::Unknown));
        }
        Ok(())
    }
}
