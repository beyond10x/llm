#![forbid(unsafe_code)]

//! Shared bounded HTTP and SSE transport; no vendor fields or credential acquisition.
//!
//! Single-attempt HTTP, bounded SSE and a one-shot JSON exchange. No credential lookup or routing.
//! Every refusal carries a retry class; whether another attempt is made is routing's decision.
//!
//! # A Tokio runtime is required
//!
//! Every exchange runs on Tokio: its deadlines are Tokio timers and `reqwest` connects on Tokio's
//! reactor. This crate starts no runtime of its own. An exchange, and a turn of every protocol
//! client built on this crate, polled on a thread that is in no Tokio runtime is refused with
//! `unsupported`, not retriable, and a message naming the requirement and the ways out of it; it
//! does not panic. On its first poll the refusal comes before a credential is resolved or a
//! connection opened, dispatch `not-sent` ([`require_tokio_runtime`]). A call moved out of its
//! runtime part-way is refused at its next poll of a wait that runs on Tokio, with the dispatch
//! it had reached ([`within_tokio_runtime`]). A caller without a runtime runs the call inside one
//! it starts or, for a single model, through `b10x-llm-blocking`, whose `BlockingModel` owns a
//! runtime and blocks the calling thread for the turn. A runtime built without its time and IO
//! drivers counts as a runtime here, and Tokio panics in it.

mod runtime;
mod sse;
mod transport;
pub use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
pub use runtime::{require_tokio_runtime, within_tokio_runtime};
pub use sse::{Framing, MAX_EVENT_BYTES, MAX_STREAM_BYTES, SseDecoder, SseEvent};
pub use transport::{
    CONNECT_TIMEOUT, HttpClient, Limits, MAX_EXCHANGE_BYTES, MAX_REFUSAL_BYTES, QuotaRule,
    REFUSAL_WAIT, ResponseTap, SseStream, retry_after, status_error,
};
