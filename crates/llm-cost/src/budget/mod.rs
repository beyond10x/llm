//! Declared-estimate admission for one explicitly owned deployment.
//! This engine performs no I/O. Use the optional SQLite owner for durable permits.
mod engine;
#[cfg(feature = "sqlite")]
mod sqlite;
mod types;

pub use engine::BudgetEngine;
#[cfg(feature = "sqlite")]
pub use sqlite::SqliteLedger;
pub use types::*;
