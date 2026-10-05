#![forbid(unsafe_code)]

//! Responses protocol projection, gateway ingress and a single-attempt client for the declared
//! supported subset.
//!
//! One contract serves both directions. [`project_request`] turns a neutral [`TurnRequest`] into
//! the body this wire accepts; [`ingest_request`] reads that same body back into a neutral request,
//! which is what a gateway ingress surface needs. [`decode_stream`] turns the wire's decoded
//! server-sent-event payloads into a neutral [`llm_core::TurnOutcome`]. [`ResponsesClient`] puts
//! the three on one bound endpoint as an [`llm_core::Model`].
//!
//! # What is here and what is not
//!
//! Everything here names an `OpenAI` Responses field or event. Server-sent-event **framing** is
//! not: bounded line, event and stream decoding belongs to `llm-http`'s `SseDecoder`, and this
//! crate consumes the payload values that decoder yields. Credential acquisition, endpoint
//! selection, retry and fallback belong to their own boundaries and are absent here. The
//! projection and the decoder perform no I/O; only [`ResponsesClient`] does, through `llm-http`
//! and an injected `llm_credentials::SecretResolver`.
//!
//! # Where the pinned subset comes from
//!
//! The request and stream shapes are adapted from `beyond10x/harness` commit
//! `9e401e40b12c2a47b54257587dbed8c16b3e93d8`, `crates/harness-responses/src/{lib,project}.rs`,
//! and from that repository's `verification-report:openai-responses-on-vllm`, a run of the same
//! wire against `vllm/vllm-openai:v0.27.1`. Four behaviours of that adapter are deliberately not
//! reproduced; [`docs/responses.md`](https://github.com/beyond10x/llm/blob/main/docs/responses.md)
//! names each one and why.

mod binding;
mod client;
mod conversation;
mod request;
mod stream;

pub use binding::{Binding, PATH, PROTOCOL};
pub use client::ResponsesClient;
pub use conversation::{Conversation, request_headers};
pub use request::{
    ACCEPTED_BODY_FIELDS, CARRIED_ENTRY_TYPES, INCLUDE, TOOL_NAME_PATTERN, encode_request,
    ingest_request, project_request,
};
pub use stream::{ACCEPTED_STREAM_EVENTS, REASONING_DELTA_EVENTS, StreamDecoding, decode_stream};
