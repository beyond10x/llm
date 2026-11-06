#![forbid(unsafe_code)]

//! Typed decisions over a routed model (`llm.decisions`).
//!
//! A closed-label classification asks one [`LabelSet`]. Each [`Classifier`] returns either a
//! [`Decision`] (one label of the set and a confidence per label in basis points, unknown left
//! absent) or an [`Abstention`] with its [`AbstentionCause`]. A [`Chain`] runs its classifiers in
//! order under one deadline until one decides, and reports every attempt.
//!
//! [`ModelClassifier`] asks a model through `llm_calls::record_turn` on one of two paths, a
//! strict output schema or one forced tool, so every model call writes one call record of kind
//! `Classification` to the caller's sink. [`LexicalClassifier`] wraps the caller's [`Lexical`]
//! classifier and makes no call.

mod chain;
mod classify;

pub use chain::{Chain, ChainOutcome};
pub use classify::{
    Abstention, AbstentionCause, Answer, Ask, Classification, Classifier, Decision, FORMAT_NAME,
    LabelConfidence, LabelSet, Lexical, LexicalClassifier, MAX_BPS, MAX_LABEL_BYTES, MAX_LABELS,
    ModelClassifier, Path, Ports, SetupError, TOOL_NAME,
};
