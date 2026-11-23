//! One answer body read as it arrives: one JSON value, or a server-sent-event stream.
//!
//! A usage reader is fed an answer's bytes in whatever pieces the connection delivered them and
//! does not know beforehand whether the answer was streamed. [`AnswerBody`] decides that from the
//! body itself, frames a stream with the same [`SseDecoder`] a client reads it with, and holds a
//! JSON body until it is whole.

use serde_json::Value;

use crate::{Framing, MAX_STREAM_BYTES, SseDecoder, SseEvent};

/// One answer body fed piece by piece.
///
/// The first byte that is not ASCII white space decides what the body is: `{` begins one JSON
/// value, an answer that was not streamed; anything else begins a server-sent-event stream. A
/// stream is framed as it arrives and each event is handed back as soon as its frame is whole; a
/// JSON body is held and read once, by [`AnswerBody::finish`].
///
/// Reading never fails. A stream stops at its first framing failure (a frame over
/// [`crate::MAX_EVENT_BYTES`], a stream over [`MAX_STREAM_BYTES`], a payload that is not JSON) and
/// yields nothing after it; a JSON body over [`MAX_STREAM_BYTES`], or one that is not one JSON
/// value, yields no value.
pub struct AnswerBody {
    framing: Framing,
    state: State,
}

enum State {
    /// Only white space so far.
    Undecided(Vec<u8>),
    /// A body that is not streamed, held until it is whole; `None` once it outgrew its bound.
    Json(Option<Vec<u8>>),
    /// An event stream.
    Events(SseDecoder),
}

impl AnswerBody {
    /// A body whose stream, if it is one, ends as `framing` says.
    pub const fn new(framing: Framing) -> Self {
        Self {
            framing,
            state: State::Undecided(Vec::new()),
        }
    }

    /// Reads the next bytes and returns the events of a stream whose frames they completed, in
    /// order. A body that is not streamed returns none here.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<SseEvent> {
        if let State::Undecided(held) = &mut self.state {
            held.extend_from_slice(bytes);
            let Some(&first) = held.iter().find(|byte| !byte.is_ascii_whitespace()) else {
                if held.len() > MAX_STREAM_BYTES {
                    self.state = State::Json(None);
                }
                return Vec::new();
            };
            let held = std::mem::take(held);
            if first == b'{' {
                self.state = State::Json(Some(Vec::new()));
                return self.push(&held);
            }
            self.state = State::Events(SseDecoder::new(self.framing));
            return self.push(&held);
        }
        match &mut self.state {
            State::Json(held) => {
                if let Some(body) = held {
                    if body.len().saturating_add(bytes.len()) > MAX_STREAM_BYTES {
                        *held = None;
                    } else {
                        body.extend_from_slice(bytes);
                    }
                }
                Vec::new()
            }
            State::Events(decoder) => decoder
                .push(bytes)
                .into_iter()
                .map_while(Result::ok)
                .collect(),
            State::Undecided(_) => Vec::new(),
        }
    }

    /// The whole value of a body that is not streamed: `None` for a stream, and for a body that
    /// is not one JSON value or outgrew its bound.
    pub fn finish(self) -> Option<Value> {
        match self.state {
            State::Json(Some(body)) => serde_json::from_slice(&body).ok(),
            _ => None,
        }
    }
}
