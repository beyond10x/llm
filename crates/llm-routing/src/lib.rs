#![forbid(unsafe_code)]

//! TOML catalogs, capability-aware resolution and explicit ordered fallback.
//!
//! Catalog validation and capability explanation are pure: no secret, network or hosting access.

mod catalog;
mod fallback;
mod selection;

pub use catalog::{
    Catalog, CatalogDocument, MAX_CONFIG_BYTES, MAX_ROUTE_TARGETS, Route, RouteTarget,
};
pub use fallback::{
    Attempt, AttemptResult, FallbackPolicy, FallbackRun, Halt, MAX_RETRY_ATTEMPTS, Models, Pause,
    Ports, RETRY_WARNING, RetryPolicy,
};
pub use selection::{Rejection, RouteExplanation, Selection, TargetExplanation};
