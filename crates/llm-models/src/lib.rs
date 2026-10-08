#![forbid(unsafe_code)]

//! Builds the single-attempt port a catalog serving model declares.
//!
//! The binding's protocol selects the Chat Completions, Responses or Messages client; the
//! binding's account selects the credential resolver through a lookup the caller supplies. An
//! anonymous account needs no resolver. A credentialed account without one is refused as
//! `unauthorized` before any I/O. Construction sends nothing and resolves no secret: resolution
//! stays at turn time, inside the client.

use llm_chat::ChatClient;
use llm_core::{
    AuthKind, BoxFuture, Cancel, Capabilities, Error, ErrorCode, Id, Model, Protocol, Provenance,
    StreamSink, TurnOutcome, TurnRequest,
};
use llm_credentials::{ResolvedSecret, SecretError, SecretRef, SecretResolver};
use llm_http::HttpClient;
use llm_messages::MessagesClient;
use llm_providers::Binding;
use llm_responses::ResponsesClient;
use llm_routing::{Catalog, Models};
use std::{collections::BTreeMap, sync::Arc};

/// The caller's credential resolvers, looked up by account id.
///
/// Implemented for a map from account id to resolver and for any closure of that shape.
pub trait Resolvers {
    /// The resolver for this account, or `None` when the caller has none for it.
    fn resolver(&self, account_id: &Id) -> Option<Arc<dyn SecretResolver>>;
}

impl Resolvers for BTreeMap<Id, Arc<dyn SecretResolver>> {
    fn resolver(&self, account_id: &Id) -> Option<Arc<dyn SecretResolver>> {
        self.get(account_id).cloned()
    }
}

impl<F> Resolvers for F
where
    F: Fn(&Id) -> Option<Arc<dyn SecretResolver>>,
{
    fn resolver(&self, account_id: &Id) -> Option<Arc<dyn SecretResolver>> {
        self(account_id)
    }
}

/// Handed to the client of an anonymous account, whose binding never asks for a secret. It
/// refuses every call instead of pretending a credential is merely missing.
struct Anonymous;

impl SecretResolver for Anonymous {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async { Err(SecretError::InvalidReference) })
    }
}

enum Client {
    Chat(ChatClient),
    Responses(ResponsesClient),
    Messages(MessagesClient),
}

/// The single-attempt port one serving model declares: one client over its binding's protocol.
pub struct Port {
    client: Client,
    resolver: Option<Arc<dyn SecretResolver>>,
}

impl Port {
    /// The protocol whose client this port is.
    pub fn protocol(&self) -> Protocol {
        match self.client {
            Client::Chat(_) => Protocol::ChatCompletions,
            Client::Responses(_) => Protocol::Responses,
            Client::Messages(_) => Protocol::Messages,
        }
    }

    /// The caller's resolver this port presents credentials from; `None` for an anonymous
    /// account.
    pub fn resolver(&self) -> Option<&Arc<dyn SecretResolver>> {
        self.resolver.as_ref()
    }

    fn model(&self) -> &dyn Model {
        match &self.client {
            Client::Chat(client) => client,
            Client::Responses(client) => client,
            Client::Messages(client) => client,
        }
    }

    fn from_binding(
        binding: Binding,
        http: HttpClient,
        resolvers: &dyn Resolvers,
    ) -> Result<Self, Error> {
        let account = &binding.declaration().account;
        let resolver = match account.auth_kind {
            AuthKind::Anonymous => None,
            AuthKind::Bearer | AuthKind::ApiKey | AuthKind::SubscriptionOauth => {
                Some(resolvers.resolver(&account.id).ok_or_else(|| {
                    Error::new(
                        ErrorCode::Unauthorized,
                        format!(
                            "no credential resolver is supplied for account `{}`",
                            account.id
                        ),
                    )
                })?)
            }
        };
        let attached = resolver
            .clone()
            .unwrap_or_else(|| Arc::new(Anonymous) as Arc<dyn SecretResolver>);
        let client = match binding.provenance().protocol {
            Protocol::ChatCompletions => Client::Chat(ChatClient::new(binding, http, attached)),
            Protocol::Responses => {
                Client::Responses(ResponsesClient::new(binding, http, attached)?)
            }
            Protocol::Messages => Client::Messages(MessagesClient::new(binding, http, attached)?),
        };
        Ok(Self { client, resolver })
    }
}

impl Model for Port {
    fn provenance(&self) -> &Provenance {
        self.model().provenance()
    }
    fn capabilities(&self) -> &Capabilities {
        self.model().capabilities()
    }
    fn turn<'a>(
        &'a self,
        request: &'a TurnRequest,
        sink: &'a mut dyn StreamSink,
        cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        self.model().turn(request, sink, cancel)
    }
}

/// Builds the port the catalog declares for one serving model.
///
/// # Errors
/// Refuses, before any I/O, a serving model id the catalog does not declare
/// (`invalid-request`) and a credentialed account the caller has no resolver for
/// (`unauthorized`).
pub fn port(
    catalog: &Catalog,
    serving_model_id: &Id,
    http: HttpClient,
    resolvers: &dyn Resolvers,
) -> Result<Port, Error> {
    let binding = catalog
        .binding(serving_model_id)
        .ok_or_else(|| Error::invalid("no such serving model in the catalog"))?;
    Port::from_binding(binding.clone(), http, resolvers)
}

/// A port for every serving model a catalog declares, served to ordered fallback as
/// [`Models`].
pub struct CatalogModels {
    ports: BTreeMap<Id, Port>,
}

impl CatalogModels {
    /// Builds every serving model's port on the rules of [`port`].
    ///
    /// # Errors
    /// Refuses as a whole, before any I/O, on the first serving model (in id order) whose port
    /// cannot be built.
    pub fn build(
        catalog: &Catalog,
        http: &HttpClient,
        resolvers: &dyn Resolvers,
    ) -> Result<Self, Error> {
        let ports = catalog
            .bindings()
            .map(|(id, binding)| {
                Port::from_binding(binding.clone(), http.clone(), resolvers)
                    .map(|port| (id.clone(), port))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { ports })
    }

    /// The port built for this serving model.
    pub fn get(&self, serving_model_id: &Id) -> Option<&Port> {
        self.ports.get(serving_model_id)
    }

    /// Every built port, keyed by serving-model id, in id order.
    pub fn iter(&self) -> impl Iterator<Item = (&Id, &Port)> {
        self.ports.iter()
    }

    pub fn len(&self) -> usize {
        self.ports.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ports.is_empty()
    }
}

impl Models for CatalogModels {
    fn model(&self, serving_model_id: &Id) -> Option<&dyn Model> {
        self.ports
            .get(serving_model_id)
            .map(|port| port as &dyn Model)
    }
}
