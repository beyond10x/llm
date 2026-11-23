//! What one answer reported about its own usage, read from its bytes by a protocol crate.

use serde::{Deserialize, Serialize};

use crate::{Id, Usage};

/// The model and the counters one answer reported, read by a protocol crate's usage reader
/// (`llm_responses::UsageReader`, `llm_messages::UsageReader`, `llm_chat::UsageReader`).
///
/// Nothing here is filled in: a model the answer did not name is `None`, and so is every counter
/// it did not report. The counters mean what they mean in [`Usage`] everywhere: input includes
/// cache read and cache write, reasoning is part of output.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportedUsage {
    /// The model the answer named as the one that served it. A configured alias never fills it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<Id>,
    /// The counters the answer reported, each `None` when it did not report it.
    #[serde(default)]
    pub usage: Usage,
    /// The reader saw the point where the answer's counters are final: the terminal event of a
    /// stream, or the whole of a body that is not streamed. False makes the counters what the
    /// answer had reported when it stopped.
    pub final_usage: bool,
}
