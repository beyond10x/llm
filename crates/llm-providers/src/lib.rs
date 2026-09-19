#![forbid(unsafe_code)]

//! Provider/account and authentication bindings independent of protocol selection.
//!
//! Binding validation performs no I/O. Authentication resolves only the explicitly named secret.

mod auth;
mod binding;
mod declaration;

pub use auth::PreparedAuth;
pub use binding::{Binding, BindingDocument};
pub use declaration::{
    Account, ApiKeyHeader, BaseUrl, Endpoint, Provider, ServedModel, ServingModel,
};
