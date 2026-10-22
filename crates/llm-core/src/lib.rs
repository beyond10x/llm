#![forbid(unsafe_code)]

//! Model-facing values and an asynchronous single-turn port; no tool execution or I/O.
//!
//! The contract and extraction provenance are documented in `docs/contract-v1.md`.

mod bound;
mod controls;
mod error;
mod id;
mod item;
mod port;
mod turn;
mod vocabulary;

pub use bound::*;
pub use controls::{
    ADVISORY_OMITTED, Advisory, CacheTtl, Controls, Hints, MAX_PROMPT_CACHE_KEY_BYTES, PromptCache,
    ReasoningSummary, Thinking, Unforwarded, Verbosity, advisory_warning, field_path, wire_effort,
};
pub use error::{Dispatch, Error, ErrorCode};
pub use id::{CallId, Id, InvalidId, ToolName};
pub use item::{Item, ToolCall};
pub use port::{
    BoxFuture, Model, SHOWN_TEXT_KEPT, SHOWN_TEXT_UNPAIRED, StreamEvent, StreamSink, VecSink,
};
pub use tokio_util::sync::CancellationToken as Cancel;
pub use turn::{
    OutcomeDocument, Sampling, StopReason, ToolChoice, ToolSpec, TurnDocument, TurnObservation,
    TurnOutcome, TurnRequest, Usage,
};
pub use vocabulary::{
    AuthKind, BEDROCK_CONVERSE_REFUSAL, BillingKind, Capabilities, Protocol, Provenance,
};
