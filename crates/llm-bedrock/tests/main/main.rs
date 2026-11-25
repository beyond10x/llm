//! The integration tests of `llm-bedrock`, linked as one test binary: one module per subject.
//! `tests/process_credentials.rs` is not among them: it is its own harness and the command its
//! cases start as a credential process, so it stays a test target of its own.

mod adversary_d262_system_messages;
mod adversary_w2026_11_09_u2;
mod adversary_w21_unpublished;
mod adversary_w25_structured_output;
mod adversary_w25_web_identity;
mod adversary_w26_image_input;
mod client;
mod client_cache_marks;
mod converse;
mod credentials;
mod eventstream;
mod image_input;
mod security_w25_web_identity;
mod sigv4_vectors;
mod structured_output;
mod system_messages;
mod web_identity;
