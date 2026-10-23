#![forbid(unsafe_code)]

//! Amazon Bedrock Runtime behind the [`llm_core::Model`] port.
//!
//! Two wires, each with a streamed and a whole-answer operation:
//!
//! - **Converse** (`/converse-stream`, `/converse`): Bedrock's own neutral request, any model
//!   that supports it. Provenance protocol [`llm_core::Protocol::BedrockConverse`].
//! - **Anthropic `InvokeModel`** (`/invoke-with-response-stream`, `/invoke`): the Anthropic
//!   Messages body with `anthropic_version: bedrock-2023-05-31`, for Anthropic models.
//!   Provenance protocol [`llm_core::Protocol::Messages`].
//!
//! Streamed answers arrive as AWS event-stream frames ([`eventstream`]). Every request is signed
//! with AWS Signature Version 4 ([`sigv4`]), written here on `ring`: no AWS SDK crate. The
//! credentials come from a [`CredentialSource`] the caller injects: keys a
//! [`llm_credentials::SecretResolver`] resolves ([`StaticCredentials`]), or the container
//! credentials endpoint EKS Pod Identity and ECS task roles serve ([`ContainerCredentials`]).
//! Feature `process`, off by default, adds `ProcessCredentials`: the keys an operator's
//! `credential_process` command prints, such as `aws configure export-credentials`.

mod client;
mod converse;
mod credentials;
mod errors;
pub mod eventstream;
mod invoke;
pub mod sigv4;
mod time;

pub use client::{Api, BedrockClient, BedrockTarget, check_request, encode_request};
#[cfg(feature = "process")]
pub use credentials::ProcessCredentials;
pub use credentials::{AwsCredentials, ContainerCredentials, CredentialSource, StaticCredentials};
