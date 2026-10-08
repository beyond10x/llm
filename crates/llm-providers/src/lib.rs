#![forbid(unsafe_code)]

//! Provider/account and authentication bindings independent of protocol selection.
//!
//! Binding validation performs no I/O. Authentication resolves only the explicitly named secret.
//! A provider description (`llm.provider-description/1`) names where and how a provider serves
//! inference and its pinned control-plane operations; parsing it performs no I/O either.

mod auth;
mod binding;
mod declaration;
mod description;

pub use auth::PreparedAuth;
pub use binding::{Binding, BindingDocument};
pub use declaration::{
    Account, ApiKeyHeader, BaseUrl, Endpoint, Provider, ServedModel, ServingModel,
};
pub use description::{
    ControlPlane, Inference, MAX_DESCRIPTION_BYTES, MAX_INSTANCE_BYTES, Operations,
    ProviderDescription, ProviderDescriptionDocument, descriptions,
};
