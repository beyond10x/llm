//! Placeholder for the `OpenAI` Responses conformance observations.
//!
//! The unit implementing story:responses-projection replaces this file. Keep the two public items:
//! `VIEWS` names every view this domain answers, and `observe` returns `None` for a
//! command another domain owns. `target.rs` is shared and is not this unit's to edit.

use ess_conformance::target::TargetError;
use serde_json::Value;

use crate::target::Observed;

/// Every view name this domain answers through `query_view`.
pub const VIEWS: &[&str] = &[];

/// Observe one command of this domain, or `None` when the command belongs to another.
pub fn observe(_command: &str, _input: &Value) -> Option<Result<Observed, TargetError>> {
    None
}
