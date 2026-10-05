use crate::{Account, Endpoint, Provider, ServedModel, ServingModel};
use llm_core::{AuthKind, Capabilities, Error, Id, Protocol, Provenance};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
enum Format {
    #[serde(rename = "llm.binding/1")]
    V1,
}

/// A versioned single-target declaration. Parsing and binding require no resolver or HTTP client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingDocument {
    format: Format,
    pub provider: Provider,
    pub account: Account,
    pub endpoint: Endpoint,
    pub model: ServedModel,
    pub serving: ServingModel,
}

impl BindingDocument {
    pub const fn new(
        provider: Provider,
        account: Account,
        endpoint: Endpoint,
        model: ServedModel,
        serving: ServingModel,
    ) -> Self {
        Self {
            format: Format::V1,
            provider,
            account,
            endpoint,
            model,
            serving,
        }
    }

    /// # Errors
    /// Refuses broken references, contradictory authentication and invalid capabilities.
    pub fn bind(self) -> Result<Binding, Error> {
        self.account.validate()?;
        self.serving.capabilities.validate()?;
        if self.account.provider_id != self.provider.id {
            return Err(Error::invalid("account references a different provider"));
        }
        if self.endpoint.account_id != self.account.id {
            return Err(Error::invalid("endpoint references a different account"));
        }
        if self.serving.endpoint_id != self.endpoint.id {
            return Err(Error::invalid(
                "serving model references a different endpoint",
            ));
        }
        if self.serving.model_id != self.model.id {
            return Err(Error::invalid("serving model references a different model"));
        }
        // Only Messages adds the subscription presentation; elsewhere the token would travel as
        // a plain bearer the route was never declared to accept.
        if self.account.auth_kind == AuthKind::SubscriptionOauth
            && self.serving.protocol != Protocol::Messages
        {
            return Err(Error::invalid(
                "subscription OAuth accounts are served only over Messages",
            ));
        }
        let provenance = Provenance {
            protocol: self.serving.protocol,
            provider: self.provider.id.clone(),
            account: self.account.id.clone(),
            endpoint: self.endpoint.id.clone(),
            model: self.model.id.clone(),
            binding_revision: self.revision()?,
        };
        let request_url = self.endpoint.base_url.request_url(self.serving.protocol);
        Ok(Binding {
            declaration: self,
            provenance,
            request_url,
        })
    }

    fn revision(&self) -> Result<Id, Error> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let bytes = serde_json::to_vec(self)
            .map_err(|_| Error::invalid("binding declaration cannot be encoded"))?;
        let mut revision = String::with_capacity(64);
        for byte in Sha256::digest(bytes) {
            revision.push(char::from(HEX[usize::from(byte >> 4)]));
            revision.push(char::from(HEX[usize::from(byte & 15)]));
        }
        Id::new(revision).map_err(|_| Error::invalid("invalid binding revision"))
    }
}

/// Validated immutable binding. An inspection cannot resolve credentials or provision resources.
#[derive(Debug, Clone)]
pub struct Binding {
    declaration: BindingDocument,
    provenance: Provenance,
    request_url: String,
}

impl Binding {
    pub fn declaration(&self) -> &BindingDocument {
        &self.declaration
    }
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
    pub fn capabilities(&self) -> &Capabilities {
        &self.declaration.serving.capabilities
    }
    pub fn request_url(&self) -> &str {
        &self.request_url
    }
    pub fn upstream_model(&self) -> &str {
        self.declaration.model.upstream_name.as_str()
    }
}
