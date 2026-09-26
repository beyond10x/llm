use crate::{Amount, Currency, RecordedCharge};
use llm_core::{BillingKind, Id, Provenance};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetPolicy {
    pub id: Id,
    pub currency: Currency,
    pub limit: Amount,
    pub max_active: u32,
}
impl BudgetPolicy {
    pub(crate) fn validate(&self) -> Result<(), BudgetError> {
        if self.max_active == 0 || self.max_active > 65_536 {
            return Err(BudgetError::InvalidPolicy);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Operation {
    Inference {
        binding: Provenance,
        billing: BillingKind,
        request_id: Id,
    },
    Compute {
        deployment: Id,
        provider: Id,
        account: Id,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReservationRequest {
    pub id: Id,
    pub operation: Operation,
    /// Explicit forecast bound; not a claim about the provider's final invoice.
    pub reserved: Amount,
    pub assumption: Id,
    /// Inclusive expiry. Only an unstarted obligation can be freed by expiry.
    pub expires_at_ms: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Reserved,
    Started,
    Uncertain,
    StopRequired,
    Stopped,
    Settled,
    Cancelled,
}
impl Phase {
    pub const fn terminal(self) -> bool {
        matches!(self, Self::Settled | Self::Cancelled)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settlement {
    pub currency: Currency,
    pub known: Amount,
    pub complete: bool,
    pub evidence: Id,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reservation {
    pub request: ReservationRequest,
    pub phase: Phase,
    pub settlement: Option<Settlement>,
    pub evidence: Option<Id>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostedCharge {
    pub observation: RecordedCharge,
    pub reconciliation: Option<Id>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "kebab-case", deny_unknown_fields)]
pub enum BudgetCommand {
    Tick,
    Reserve {
        request: ReservationRequest,
    },
    Begin {
        id: Id,
    },
    Cancel {
        id: Id,
    },
    Uncertain {
        id: Id,
        evidence: Id,
    },
    Settle {
        id: Id,
        settlement: Settlement,
    },
    RequireStop {
        id: Id,
        evidence: Id,
    },
    ConfirmStopped {
        id: Id,
        evidence: Id,
    },
    Renew {
        id: Id,
        additional: Amount,
        expires_at_ms: u64,
        assumption: Id,
    },
    PostCharge {
        charge: RecordedCharge,
    },
    ReconcileCharge {
        id: Id,
        amount: Amount,
        evidence: Id,
    },
}

/// A start receipt issued only after the adapter's commit. It is neither Clone nor
/// serializable: reopening a journal never reissues a historical start receipt.
#[derive(Debug)]
pub struct DispatchPermit {
    pub(crate) ledger: Id,
    pub(crate) reservation: Id,
}
impl DispatchPermit {
    pub fn ledger(&self) -> &Id {
        &self.ledger
    }
    pub fn reservation(&self) -> &Id {
        &self.reservation
    }
}
#[derive(Debug)]
pub struct BudgetReceipt {
    pub permit: Option<DispatchPermit>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BudgetTotals {
    pub settled_nanos: String,
    pub held_nanos: String,
    pub exposure_nanos: String,
    pub uncertain_count: usize,
    pub active_count: usize,
    pub above_limit: bool,
    pub stop_required: Vec<Id>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BudgetView {
    pub storage_failed: bool,
    pub policy: BudgetPolicy,
    pub now_ms: u64,
    pub reservations: Vec<Reservation>,
    pub charges: Vec<PostedCharge>,
    pub totals: BudgetTotals,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "kebab-case")]
pub enum BudgetError {
    #[error("invalid budget policy")]
    InvalidPolicy,
    #[error("budget policy does not match the existing ledger")]
    PolicyMismatch,
    #[error("budget identifier already exists")]
    Duplicate,
    #[error("compute deployment already has an outstanding obligation")]
    ResourceBusy,
    #[error("budget obligation does not exist")]
    Missing,
    #[error("operation is not valid in this phase")]
    WrongPhase,
    #[error("reservation expiry must be later than the current time")]
    InvalidExpiry,
    #[error("budget clock moved backwards")]
    ClockReversed,
    #[error("budget has unresolved spend or shutdown obligations")]
    UncertainSpend,
    #[error("declared budget would be exceeded")]
    LimitExceeded,
    #[error("concurrent obligation limit would be exceeded")]
    ConcurrencyExceeded,
    #[error("monetary arithmetic exceeds its range")]
    Arithmetic,
    #[error("charge or currency is invalid")]
    InvalidCharge,
    #[error("ledger document exceeds its bound")]
    TooLarge,
    #[error("ledger is already owned by another handle or process")]
    OwnerBusy,
    #[error("ledger storage is unavailable; no new permit was issued")]
    Storage,
    #[error("ledger journal is malformed, unsupported or inconsistent")]
    InvalidJournal,
}
