//! The integration tests of `llm-models`, linked as one test binary: one module per subject.
//! `tests/process_routes.rs` is not among them: it is its own harness and the command its cases
//! start as a credential process, so it stays a test target of its own.

mod adversary_w09_capabilities;
mod adversary_w10_routes;
mod adversary_w10_usage_limit;
mod adversary_w11_anonymous_resolver;
mod adversary_w11_no_runtime;
mod adversary_w11_pass2_runtime_dropped;
mod adversary_w24_refusal_route;
mod adversary_w26_feature_documents;
mod adversary_w26_web_identity_routes;
mod construction;
mod feature_declarations;
mod no_tokio_runtime;
mod relayed_failure;
mod routes;
mod routes_adversary;
mod routes_adversary_pass2;
mod routes_toml_bedrock;
mod web_identity_routes;
