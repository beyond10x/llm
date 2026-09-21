#![forbid(unsafe_code)]

//! Chat Completions protocol projection and gateway ingress for the declared supported subset.
//!
//! Three surfaces over one documented subset of the `OpenAI` Chat Completions wire, which
//! `vLLM`'s server and other compatible endpoints also serve:
//!
//! - [`project_request`] builds an outgoing request from a validated neutral turn;
//! - [`project_response_bytes`], [`StreamProjection`] and [`decode_completion`] read a
//!   response back onto the neutral outcome and stream vocabulary;
//! - [`decode_ingress_request`], [`encode_ingress_completion`] and [`IngressStream`] serve
//!   the same subset to a client that speaks this wire to a gateway.
//!
//! [`ChatClient`] joins the first two over the shared bounded HTTP/SSE transport.
//!
//! What the subset covers, and what it refuses by name, is documented in `docs/chat.md`.
//! An unreported counter, identifier or model name stays unreported everywhere here: no
//! configured value is ever substituted for an observation the upstream did not make.

mod client;
mod incoming;
mod ingress;
mod outgoing;

pub use client::ChatClient;
pub use incoming::{StreamProjection, decode_completion, project_response_bytes};
// `StreamProjection::accept` takes one of these, so a caller holding framed events
// needs it; same reason the binding vocabulary below is re-exported.
pub use ingress::{
    IngressRequest, IngressStream, NAMED_UNSUPPORTED_REQUEST_FIELDS, SUPPORTED_REQUEST_FIELDS,
    decode_ingress_request, encode_ingress_completion,
};
pub use llm_http::SseEvent;
// The binding vocabulary this crate's public functions take and a caller has to build,
// re-exported so a consumer needs no second dependency to call them. `llm-http` re-exports
// its own header types for the same reason.
pub use llm_providers::{
    Account, BaseUrl, Binding, BindingDocument, Endpoint, Provider, ServedModel, ServingModel,
};
pub use outgoing::project_request;
