use crate::{Provenance, TurnObservation};
use serde::{Deserialize, Serialize};

/// What is known about dispatch, independently of the error category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Dispatch {
    NotSent,
    Rejected,
    Unknown,
    Accepted,
}

/// Stable neutral failure categories. Diagnostics must not contain secret or response bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorCode {
    InvalidRequest,
    Transport,
    Protocol,
    Unauthorized,
    RateLimited,
    Refused,
    TooLarge,
    Unsupported,
    Cancelled,
    Deadline,
    Unavailable,
}

/// Safe diagnostics, dispatch evidence and a retry class.
///
/// Dispatch says what the far side may have done; `retriable` says whether another identical
/// attempt may answer. They are separate facts: a stream cut inside an event keeps `accepted`
/// dispatch and is retriable. Neither is permission to resend after output became visible;
/// [`Error::may_retry`] is the whole class decision and the caller owns visibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(deny_unknown_fields)]
#[error("{code:?}: {message}")]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
    pub dispatch: Dispatch,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
    /// Set by the producer that observed the failure; absent on the wire when false.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub retriable: bool,
    /// Last valid bound evidence, including partial usage from an interrupted stream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation: Option<Box<TurnObservation>>,
}

impl Error {
    /// Construct a local failure. After network dispatch, explicitly attach dispatch evidence.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            dispatch: Dispatch::NotSent,
            retry_after_ms: None,
            retriable: false,
            observation: None,
        }
    }

    #[must_use]
    pub const fn with_dispatch(mut self, dispatch: Dispatch) -> Self {
        self.dispatch = dispatch;
        self
    }

    /// States whether another identical attempt may answer.
    #[must_use]
    pub const fn with_retriable(mut self, retriable: bool) -> Self {
        self.retriable = retriable;
        self
    }

    /// Whether this failure may be attempted again before any output is visible: its producer
    /// marked it retriable and its code is one Harness retries. Unauthorized, refused,
    /// invalid-request, unsupported, too-large, cancelled and deadline failures never are,
    /// whatever the mark says.
    pub const fn may_retry(&self) -> bool {
        self.retriable
            && matches!(
                self.code,
                ErrorCode::Transport
                    | ErrorCode::RateLimited
                    | ErrorCode::Unavailable
                    | ErrorCode::Protocol
            )
    }

    #[must_use]
    pub fn with_observation(mut self, observation: TurnObservation) -> Self {
        self.observation = Some(Box::new(observation));
        self
    }

    /// # Errors
    /// Refuses foreign or contradictory observations and upstream evidence on an unsent attempt.
    pub fn validate_for(&self, target: &Provenance) -> Result<(), Self> {
        if let Some(observation) = &self.observation {
            if self.dispatch == Dispatch::NotSent {
                return Err(Self::protocol(
                    "unsent failure carries upstream observations",
                ));
            }
            observation.validate_for(target)?;
        }
        Ok(())
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidRequest, message)
    }
    pub fn protocol(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Protocol, message)
    }
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unsupported, message)
    }
    pub fn too_large(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::TooLarge, message)
    }
    pub fn cancelled() -> Self {
        Self::new(ErrorCode::Cancelled, "the caller cancelled")
    }
}
