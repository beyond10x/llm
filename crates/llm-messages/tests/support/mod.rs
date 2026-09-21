//! Deterministic fixture binding shared by the Messages tests. No network or credential store.
#![allow(dead_code)]

use llm_core::{AuthKind, BillingKind, BoxFuture, Capabilities, Id, Protocol};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_providers::{
    Account, ApiKeyHeader, BaseUrl, Binding, BindingDocument, Endpoint, Provider, ServedModel,
    ServingModel,
};

pub fn capabilities() -> Capabilities {
    Capabilities {
        tools: true,
        tool_choice: true,
        temperature: true,
        top_p: true,
        reasoning_efforts: vec!["medium".to_owned(), "high".to_owned()],
        context_window: 32_768,
        max_output_tokens: 2_048,
    }
}

/// The fixture binding, parameterized only by what a test actually varies.
pub fn binding_with(base_url: &str, capabilities: Capabilities) -> Binding {
    BindingDocument::new(
        Provider {
            id: Id::new("lab").unwrap(),
            category: Id::new("hosted").unwrap(),
        },
        Account {
            id: Id::new("account").unwrap(),
            provider_id: Id::new("lab").unwrap(),
            auth_kind: AuthKind::ApiKey,
            billing_kind: BillingKind::Metered,
            secret_reference_id: Some(SecretRef::new("messages-key").unwrap()),
            api_key_header: Some(ApiKeyHeader::new("x-api-key").unwrap()),
        },
        Endpoint {
            id: Id::new("endpoint").unwrap(),
            account_id: Id::new("account").unwrap(),
            base_url: BaseUrl::new(base_url).unwrap(),
        },
        ServedModel {
            id: Id::new("internal-model").unwrap(),
            upstream_name: Id::new("example/Model-Revision").unwrap(),
        },
        ServingModel {
            id: Id::new("serving").unwrap(),
            endpoint_id: Id::new("endpoint").unwrap(),
            model_id: Id::new("internal-model").unwrap(),
            protocol: Protocol::Messages,
            capabilities,
        },
    )
    .bind()
    .unwrap()
}

pub fn binding() -> Binding {
    binding_with("https://messages.example.invalid/v1", capabilities())
}

/// An injected resolver holding one fixture credential. Nothing is read from this machine.
pub struct StaticResolver {
    pub material: Vec<u8>,
    pub failure: Option<SecretError>,
}

impl StaticResolver {
    pub fn new(material: &str) -> Self {
        Self {
            material: material.as_bytes().to_vec(),
            failure: None,
        }
    }
    pub fn failing(failure: SecretError) -> Self {
        Self {
            material: Vec::new(),
            failure: Some(failure),
        }
    }
}

impl SecretResolver for StaticResolver {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move {
            if let Some(failure) = self.failure {
                return Err(failure);
            }
            Ok(ResolvedSecret {
                secret: Secret::new(self.material.clone())?,
                version: SecretVersion::new("generation-1".to_owned())?,
            })
        })
    }
}
