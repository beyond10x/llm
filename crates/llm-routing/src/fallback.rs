//! Explicit ordered fallback over single-attempt `Model` ports.
//!
//! Only the route's declared, compatible, admitted targets are attempted, in declared order, and
//! only after a defined failure class that left nothing visible and was provably not accepted.

use crate::{Catalog, MAX_ROUTE_TARGETS, RouteExplanation, Selection};
use llm_core::{
    AuthKind, BillingKind, BoxFuture, Cancel, Dispatch, Error, ErrorCode, Id, Model, Provenance,
    StreamEvent, StreamSink, TurnObservation, TurnOutcome, TurnRequest,
};
use std::time::Instant;

/// Supplies the single-attempt port that serves a declared serving model.
pub trait Models: Sync {
    fn model(&self, serving_model_id: &Id) -> Option<&dyn Model>;
}

/// Caller-owned bounds. `max_attempts == 1` disables fallback regardless of the route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FallbackPolicy {
    pub max_attempts: usize,
    /// No attempt starts at or after this instant.
    pub deadline: Option<Instant>,
}

impl Default for FallbackPolicy {
    fn default() -> Self {
        Self {
            max_attempts: MAX_ROUTE_TARGETS,
            deadline: None,
        }
    }
}

impl FallbackPolicy {
    /// Attempt only the first compatible target.
    pub const fn disabled() -> Self {
        Self {
            max_attempts: 1,
            deadline: None,
        }
    }

    /// # Errors
    /// Refuses an attempt bound outside one through the route target bound.
    pub fn validate(&self) -> Result<(), Error> {
        if self.max_attempts == 0 || self.max_attempts > MAX_ROUTE_TARGETS {
            return Err(Error::invalid(
                "fallback attempt bound must be between one and 64",
            ));
        }
        Ok(())
    }
}

/// What one started attempt returned. Evidence is kept for failures as well as successes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptResult {
    Succeeded {
        observation: TurnObservation,
    },
    Failed {
        code: ErrorCode,
        dispatch: Dispatch,
        observation: Option<TurnObservation>,
    },
}

/// One started attempt against one declared route target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub target_id: Id,
    pub serving_model_id: Id,
    pub position: usize,
    pub provenance: Provenance,
    pub auth_kind: AuthKind,
    pub billing_kind: BillingKind,
    /// Stream events offered to the caller's sink during this attempt.
    pub visible_events: usize,
    pub result: AttemptResult,
}

/// Why the run stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Halt {
    Succeeded,
    /// No compatible target remained; the explanation names every rejection.
    Exhausted,
    /// The failure class is not eligible for fallback.
    IneligibleFailure,
    /// Output had already reached the caller.
    VisibleOutput,
    /// The request may have been accepted, so it is never replayed elsewhere.
    AmbiguousDispatch,
    /// The caller's admission check refused the next target.
    LimitRefused,
    Deadline,
    Cancelled,
    /// The caller's attempt bound was reached.
    AttemptBound,
}

impl Halt {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Exhausted => "exhausted",
            Self::IneligibleFailure => "ineligible-failure",
            Self::VisibleOutput => "visible-output",
            Self::AmbiguousDispatch => "ambiguous-dispatch",
            Self::LimitRefused => "limit-refused",
            Self::Deadline => "deadline",
            Self::Cancelled => "cancelled",
            Self::AttemptBound => "attempt-bound",
        }
    }
}

/// The ordered record of a fallback run and its final result.
#[derive(Debug)]
pub struct FallbackRun {
    pub explanation: RouteExplanation,
    pub attempts: Vec<Attempt>,
    pub halt: Halt,
    pub result: Result<TurnOutcome, Error>,
}

struct Counting<'s> {
    inner: &'s mut dyn StreamSink,
    offered: usize,
}

impl StreamSink for Counting<'_> {
    fn emit(&mut self, event: StreamEvent) -> BoxFuture<'_, Result<(), Error>> {
        self.offered = self.offered.saturating_add(1);
        self.inner.emit(event)
    }
}

const fn eligible(error: &Error) -> bool {
    matches!(error.dispatch, Dispatch::NotSent | Dispatch::Rejected)
        && matches!(
            error.code,
            ErrorCode::Transport | ErrorCode::RateLimited | ErrorCode::Unavailable
        )
}

/// The caller's side of a run: model ports, limit decision, output sink and cancellation.
pub struct Ports<'p> {
    pub models: &'p dyn Models,
    /// The caller's limit decision, consulted before every attempt including the first.
    pub admit: &'p mut (dyn FnMut(&Selection<'_>) -> Result<(), Error> + Send),
    pub sink: &'p mut dyn StreamSink,
    pub cancel: &'p Cancel,
}

impl Catalog {
    /// Run a turn over the route's declared compatible targets in order, falling back only after
    /// an eligible failure that exposed no output.
    /// # Errors
    /// Refuses invalid input, unknown aliases, invalid policies and missing or mismatched models
    /// before any attempt. Every later outcome, including refusal, is reported in the run.
    pub async fn run_turn(
        &self,
        request: &TurnRequest,
        input_tokens: Option<u64>,
        policy: FallbackPolicy,
        ports: Ports<'_>,
    ) -> Result<FallbackRun, Error> {
        let Ports {
            models,
            admit,
            sink,
            cancel,
        } = ports;
        policy.validate()?;
        let explanation = self.explain(request, input_tokens)?;
        // Only targets the run could attempt are bound and checked for a model.
        let mut candidates = Vec::new();
        let mut bounded = false;
        for target in &explanation.targets {
            if !target.rejections.is_empty() {
                continue;
            }
            if candidates.len() >= policy.max_attempts {
                bounded = true;
                break;
            }
            let selection = self.select(&explanation.alias, &target.target_id, request)?;
            let model = models
                .model(&selection.target.serving_model_id)
                .ok_or_else(|| Error::invalid("no model serves a compatible route target"))?;
            if model.provenance() != selection.binding.provenance() {
                return Err(Error::invalid(
                    "model does not serve its route target's binding",
                ));
            }
            candidates.push((selection, model));
        }
        let mut attempts = Vec::new();
        let mut last = Err(no_compatible_target(&explanation));
        for (selection, model) in candidates {
            let refused = if cancel.is_cancelled() {
                Some((Halt::Cancelled, Err(Error::cancelled())))
            } else if policy
                .deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
            {
                let error = Error::new(ErrorCode::Deadline, "fallback deadline reached");
                Some((Halt::Deadline, Err(error)))
            } else if let Err(error) = admit(&selection) {
                Some((Halt::LimitRefused, Err(error)))
            } else {
                None
            };
            if let Some((halt, result)) = refused {
                return Ok(finish(explanation, attempts, halt, result));
            }
            let mut counting = Counting {
                inner: &mut *sink,
                offered: 0,
            };
            let returned = model.turn(&selection.request, &mut counting, cancel).await;
            let visible_events = counting.offered;
            let (recorded, halt, result) = settle(returned, &selection, visible_events);
            attempts.push(attempt(&selection, visible_events, recorded));
            match halt {
                Some(halt) => return Ok(finish(explanation, attempts, halt, result)),
                None => last = result,
            }
        }
        let halt = if bounded {
            Halt::AttemptBound
        } else {
            Halt::Exhausted
        };
        Ok(finish(explanation, attempts, halt, last))
    }
}

/// Classify one attempt's return. `None` permits trying the next declared target.
fn settle(
    returned: Result<TurnOutcome, Error>,
    selection: &Selection<'_>,
    visible_events: usize,
) -> (AttemptResult, Option<Halt>, Result<TurnOutcome, Error>) {
    let provenance = selection.binding.provenance();
    match returned {
        Ok(outcome) => match outcome.validate_for(&selection.request, provenance) {
            Ok(()) => (
                AttemptResult::Succeeded {
                    observation: outcome.observation.clone(),
                },
                Some(Halt::Succeeded),
                Ok(outcome),
            ),
            Err(error) => {
                let mut error = error.with_dispatch(Dispatch::Accepted);
                // A refused outcome still reported its attempt's evidence; keep it when bound.
                if outcome.observation.validate_for(provenance).is_ok() {
                    error = error.with_observation(outcome.observation);
                }
                (failed(&error), Some(Halt::IneligibleFailure), Err(error))
            }
        },
        Err(mut error) => {
            let halt = if error.validate_for(provenance).is_err() {
                // Contradictory evidence proves nothing about dispatch. Foreign evidence is not
                // this attempt's; bound evidence is kept even when its dispatch claim is not.
                if error
                    .observation
                    .as_ref()
                    .is_some_and(|observation| observation.validate_for(provenance).is_err())
                {
                    error.observation = None;
                }
                error.dispatch = Dispatch::Unknown;
                Some(Halt::AmbiguousDispatch)
            } else if visible_events > 0 {
                Some(Halt::VisibleOutput)
            } else if error.dispatch == Dispatch::Unknown {
                Some(Halt::AmbiguousDispatch)
            } else if eligible(&error) {
                None
            } else {
                Some(Halt::IneligibleFailure)
            };
            (failed(&error), halt, Err(error))
        }
    }
}

fn failed(error: &Error) -> AttemptResult {
    AttemptResult::Failed {
        code: error.code,
        dispatch: error.dispatch,
        observation: error.observation.as_deref().cloned(),
    }
}

fn attempt(selection: &Selection<'_>, visible_events: usize, result: AttemptResult) -> Attempt {
    let account = &selection.binding.declaration().account;
    Attempt {
        target_id: selection.target.id.clone(),
        serving_model_id: selection.target.serving_model_id.clone(),
        position: selection.target.position,
        provenance: selection.binding.provenance().clone(),
        auth_kind: account.auth_kind,
        billing_kind: account.billing_kind,
        visible_events,
        result,
    }
}

const fn finish(
    explanation: RouteExplanation,
    attempts: Vec<Attempt>,
    halt: Halt,
    result: Result<TurnOutcome, Error>,
) -> FallbackRun {
    FallbackRun {
        explanation,
        attempts,
        halt,
        result,
    }
}

pub(crate) fn no_compatible_target(explanation: &RouteExplanation) -> Error {
    let reasons = explanation
        .targets
        .iter()
        .flat_map(|target| target.rejections.iter().map(|reason| reason.label()))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(", ");
    Error::unsupported(format!("route has no compatible target: {reasons}"))
}
