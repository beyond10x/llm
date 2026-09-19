#![forbid(unsafe_code)]

//! Attributed usage estimates and recorded charges from explicit versioned prices.
//! No I/O, provider price lookup, currency conversion or budget enforcement.
//! Adapted from beyond10x/harness 709a2eb: harness-loop/src/price.rs, with exact
//! decimal inputs and the neutral contract's unknown quantities preserved.

mod document;
mod money;
mod pricing;

pub use document::{
    AccountingInput, AttemptUsage, ChargeKind, ComputePrice, ComputeUsage, ModelPrice, Observation,
    PriceBook, PriceDocument, RecordedCharge,
};
pub use money::{Amount, Currency, Rate};
pub use pricing::{Basis, CostReport, Line, Record, Total, Unit, UnknownReason};

pub const MAX_DOCUMENT_BYTES: usize = 1024 * 1024;
pub const MAX_ENTRIES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CostError {
    #[error("invalid or unsupported accounting document")]
    InvalidDocument,
    #[error("invalid exact nonnegative decimal amount")]
    InvalidAmount,
    #[error("currency must be three uppercase ASCII letters")]
    InvalidCurrency,
    #[error("accounting document exceeds its bound")]
    TooLarge,
    #[error("duplicate price identifier")]
    DuplicatePrice,
    #[error("duplicate observation identifier")]
    DuplicateObservation,
    #[error("price denominator must be positive")]
    InvalidDenominator,
    #[error("usage contains contradictory known counts")]
    InvalidUsage,
    #[error("recorded charge time interval must be nonempty and ordered")]
    InvalidPeriod,
    #[error("currencies differ; no implicit conversion is permitted")]
    CurrencyMismatch,
    #[error("monetary arithmetic exceeds its bound")]
    Overflow,
}
