//! The integration tests of `llm-cli`, linked as one test binary: one module per subject.
//! `tests/credential_process.rs` is not among them: it is its own harness and the command its
//! cases start as a credential process, so it stays a test target of its own.

mod adversary_w24_refusal;
mod command;
