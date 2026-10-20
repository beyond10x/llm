#![forbid(unsafe_code)]

//! Shared bounded HTTP and SSE transport; no vendor fields or credential acquisition.
//!
//! Single-attempt HTTP, bounded SSE and a one-shot JSON exchange. No credential lookup or routing.
//! Every refusal carries a retry class; whether another attempt is made is routing's decision.
//!
//! # A Tokio runtime is required
//!
//! Every exchange runs on Tokio: its deadlines are Tokio timers and `reqwest` connects on Tokio's
//! reactor. Polled on a thread that is in no Tokio runtime, an exchange, and a turn of every
//! protocol client built on this crate, is refused before it resolves a credential or connects:
//! `unsupported`, dispatch `not-sent`, not retriable, with a message naming the requirement
//! ([`require_tokio_runtime`]). It does not panic, and it does not start a runtime of its own. A
//! caller without one runs its turn through `b10x-llm-blocking`, whose `BlockingModel` owns a
//! runtime and blocks the calling thread for the turn.

mod runtime;
mod sse;
mod transport;
pub use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
pub use runtime::require_tokio_runtime;
pub use sse::{Framing, MAX_EVENT_BYTES, MAX_STREAM_BYTES, SseDecoder, SseEvent};
pub use transport::{
    CONNECT_TIMEOUT, HttpClient, Limits, MAX_EXCHANGE_BYTES, MAX_REFUSAL_BYTES, QuotaRule,
    REFUSAL_WAIT, ResponseTap, SseStream, retry_after, status_error,
};
