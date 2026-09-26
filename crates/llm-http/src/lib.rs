#![forbid(unsafe_code)]

//! Shared bounded HTTP and SSE transport; no vendor fields or credential acquisition.
//!
//! Single-attempt asynchronous HTTP and bounded SSE. No credential lookup or route selection.

mod sse;
mod transport;
pub use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
pub use sse::{Framing, MAX_EVENT_BYTES, MAX_STREAM_BYTES, SseDecoder, SseEvent};
pub use transport::{HttpClient, Limits, SseStream, retry_after, status_error};
