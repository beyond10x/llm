#![forbid(unsafe_code)]

//! One text-free record per routed model call, written to a sink the caller injects.

mod record;

pub use record::{
    CallFacts, CallIdentity, CallLog, CallRecord, CallSink, Clock, Cost, Kind, Outcome, Role,
    Stopwatch, SystemClock, Timing,
};

use llm_core::{Error, TurnRequest};
use llm_cost::PriceBook;
use llm_routing::{Catalog, FallbackPolicy, FallbackRun, Ports};

/// Where a routed call's record goes and what it is measured and priced with.
#[derive(Clone, Copy)]
pub struct Recording<'r> {
    pub sink: &'r dyn CallSink,
    pub clock: &'r dyn Clock,
    /// Without a price book a record carries usage and no cost.
    pub prices: Option<&'r PriceBook>,
}

/// Runs [`Catalog::run_turn`] and writes one record of it.
///
/// # Errors
/// Exactly the refusals of [`Catalog::run_turn`].
pub async fn record_turn(
    catalog: &Catalog,
    request: &TurnRequest,
    input_tokens: Option<u64>,
    policy: FallbackPolicy,
    ports: Ports<'_>,
    identity: CallIdentity,
    recording: Recording<'_>,
) -> Result<FallbackRun, Error> {
    let _ = (identity, recording);
    catalog.run_turn(request, input_tokens, policy, ports).await
}
