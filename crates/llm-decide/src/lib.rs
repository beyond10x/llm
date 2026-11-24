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
//!
//! A rubric verdict asks one [`Rubric`]: a versioned text, the JSON Schema of the verdict and a
//! route with exactly one target and fallback off. [`judge`] makes one call through
//! `llm_calls::record_turn` with the schema as strict output, which writes one call record of
//! kind `Verdict`, and returns a [`Verdict`] naming the rubric version and digest, the route
//! target and the upstream model that answered, with the answer as canonical JSON. An answer
//! that is not the one whole JSON value the model ended on, or is not valid against the schema, is
//! refused, never repaired.
//!
//! [`Routed`] is the candidate of an offline replay (`llm_calls::replay`): it answers each case
//! of a recorded corpus with one call on the candidate route, a turn through
//! `llm_calls::record_turn`, a classification through a [`ModelClassifier`] and a verdict through
//! [`judge`].

mod answer;
mod chain;
mod classify;
mod judge;
mod replay;
mod schema;

pub use chain::{Chain, ChainOutcome};
pub use classify::{
    Abstention, AbstentionCause, Answer, Ask, ChoiceAnswer, ChoiceCall, Classification, Classifier,
    Decision, DecisionEndpoint, EndpointClassifier, FORMAT_NAME, LabelConfidence, LabelProbability,
    LabelSet, Lexical, LexicalClassifier, MAX_BPS, MAX_LABEL_BYTES, MAX_LABELS, ModelClassifier,
    Path, Ports, SetupError, TOOL_NAME, basis_points,
};
pub use judge::{JudgeError, Rubric, RubricError, VERDICT_FORMAT_NAME, Verdict, judge};
pub use replay::Routed;
