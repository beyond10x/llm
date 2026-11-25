//! The one integration-test binary of `llm-credentials`: each module is a former `tests/*.rs`
//! file.
//!
//! This file needs no feature. A module that needs one says so in its own first lines
//! (`#![cfg(...)]`), so a build without that feature leaves the module out.
//!
//! Two modules start this executable again as a child and name the case to run by its module
//! path (`environment`, `adversary_codex`): renaming a module moves that name.

mod adversary2_codex;
mod adversary_codex;
mod adversary_codex_renewal;
mod adversary_pointer;
mod adversary_secrets;
mod adversary_w2026_11_09_u5;
mod codex;
mod codex_renewal;
mod environment;
mod injected;
mod json_pointer;
mod live_subscription_turn;
mod local_adapters;
mod secrets;
mod secrets_boundary;
