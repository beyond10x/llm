#![forbid(unsafe_code)]

//! Model-facing values and an asynchronous single-turn port; no tool execution or I/O.
//!
//! The contract and extraction provenance are documented in `docs/contract-v1.md`.

mod bound;
mod error;
mod id;
mod item;
mod port;
mod turn;
mod vocabulary;

pub use bound::*;
pub use error::{Dispatch, Error, ErrorCode};
pub use id::{CallId, Id, InvalidId, ToolName};
pub use item::{Item, ToolCall};
pub use port::{BoxFuture, Model, StreamEvent, StreamSink, VecSink};
pub use tokio_util::sync::CancellationToken as Cancel;
pub use turn::{
    OutcomeDocument, Sampling, StopReason, ToolChoice, ToolSpec, TurnDocument, TurnObservation,
    TurnOutcome, TurnRequest, Usage,
};
pub use vocabulary::{AuthKind, BillingKind, Capabilities, Protocol, Provenance};
