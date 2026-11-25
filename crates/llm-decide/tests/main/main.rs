//! The integration tests of `llm-decide`, built as one binary. Each module is one file of cases
//! with its own helpers and fakes; no module reads an item of another.

mod adversary_u2_chain;
mod adversary_w27_judge;
mod adversary_w27_judge_pass2;
mod classify;
mod judge;
