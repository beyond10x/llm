//! Provider descriptions (`llm.provider-description/1`): what every consumer reads about one
//! provider, without I/O.
//!
//! The inference half is a base-URL template holding `{instance}` once, inside the host's first
//! label, the wires served there and their authentication. The optional control-plane half names a
//! pinned `OpenAPI` document by URL and `SHA-256` digest, its HTTPS server, its authentication and the
//! `operationId` of each of four fixed roles. Parsing never fetches the document and resolves no
//! secret; checking the digest against the document is the reader's job.

use crate::{ApiKeyHeader, BaseUrl, Provider};
use llm_core::{AuthKind, Error, Protocol};
use serde::Deserialize;
use url::Url;

/// The largest description document accepted.
pub const MAX_DESCRIPTION_BYTES: usize = 64 * 1024;
/// The largest instance name accepted.
pub const MAX_INSTANCE_BYTES: usize = 48;
const MAX_OPERATION_ID_BYTES: usize = 128;
const PLACEHOLDER: &str = "{instance}";

#[derive(Debug, Clone, Deserialize)]
enum Format {
    #[serde(rename = "llm.provider-description/1")]
    V1,
}

/// A parsed but not yet validated description document.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderDescriptionDocument {
    #[allow(dead_code)]
    format: Format,
    provider: Provider,
    inference: Inference,
    #[serde(default)]
    control_plane: Option<ControlPlane>,
}

impl ProviderDescriptionDocument {
    /// # Errors
    /// Refuses oversized input, malformed TOML, an unknown format and unknown fields, without
    /// echoing any input value.
    pub fn parse(source: &str) -> Result<Self, Error> {
        if source.len() > MAX_DESCRIPTION_BYTES {
            return Err(Error::invalid(
                "provider description exceeds its byte bound",
            ));
        }
        toml::from_str(source).map_err(|_: toml::de::Error| {
            Error::invalid("invalid llm.provider-description/1 TOML document")
        })
    }

    /// # Errors
    /// Refuses a template, wire list, authentication or control plane the format does not allow.
    pub fn validate(self) -> Result<ProviderDescription, Error> {
        self.inference.validate()?;
        if let Some(control_plane) = &self.control_plane {
            control_plane.validate()?;
        }
        Ok(ProviderDescription {
            provider: self.provider,
            inference: self.inference,
            control_plane: self.control_plane,
        })
    }
}

/// A validated provider description.
#[derive(Debug, Clone)]
pub struct ProviderDescription {
    provider: Provider,
    inference: Inference,
    control_plane: Option<ControlPlane>,
}

impl ProviderDescription {
    /// Parses and validates one `llm.provider-description/1` document.
    ///
    /// # Errors
    /// Every refusal is `invalid` with a fixed message naming no input value.
    pub fn parse(source: &str) -> Result<Self, Error> {
        ProviderDescriptionDocument::parse(source)?.validate()
    }

    pub fn provider(&self) -> &Provider {
        &self.provider
    }

    pub fn inference(&self) -> &Inference {
        &self.inference
    }

    pub fn control_plane(&self) -> Option<&ControlPlane> {
        self.control_plane.as_ref()
    }

    /// Fills the template with one instance name.
    ///
    /// # Errors
    /// Refuses, before substitution, an instance that is not 1-48 bytes of `[a-z0-9]`, so an
    /// instance can change neither host, path nor scheme.
    pub fn inference_base_url(&self, instance: &str) -> Result<BaseUrl, Error> {
        if !valid_instance(instance) {
            return Err(Error::invalid(
                "instance must be 1-48 bytes of lower-case ASCII letters and digits",
            ));
        }
        fill(&self.inference.base_url_template, instance)
    }
}

/// Where and how inference is served.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inference {
    base_url_template: String,
    protocols: Vec<Protocol>,
    auth_kind: AuthKind,
    #[serde(default)]
    api_key_header: Option<ApiKeyHeader>,
}

impl Inference {
    pub fn base_url_template(&self) -> &str {
        &self.base_url_template
    }

    /// The wires in declared order.
    pub fn protocols(&self) -> &[Protocol] {
        &self.protocols
    }

    pub fn auth_kind(&self) -> AuthKind {
        self.auth_kind
    }

    pub fn api_key_header(&self) -> Option<&ApiKeyHeader> {
        self.api_key_header.as_ref()
    }

    fn validate(&self) -> Result<(), Error> {
        validate_template(&self.base_url_template)?;
        if self.protocols.is_empty() {
            return Err(Error::invalid(
                "provider description must name at least one protocol",
            ));
        }
        for (index, protocol) in self.protocols.iter().enumerate() {
            if self.protocols[..index].contains(protocol) {
                return Err(Error::invalid(
                    "provider description names a protocol twice",
                ));
            }
        }
        validate_auth(self.auth_kind, self.api_key_header.as_ref())
    }
}

/// The provider's control plane, compiled by its reader from the pinned `OpenAPI` document.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlPlane {
    openapi_url: String,
    document_sha256: String,
    server_url: String,
    auth_kind: AuthKind,
    #[serde(default)]
    api_key_header: Option<ApiKeyHeader>,
    operations: Operations,
}

impl ControlPlane {
    pub fn openapi_url(&self) -> &str {
        &self.openapi_url
    }

    /// The `SHA-256` of the pinned document, 64 lower-case hex digits. Only its shape is checked.
    pub fn document_sha256(&self) -> &str {
        &self.document_sha256
    }

    pub fn server_url(&self) -> &str {
        &self.server_url
    }

    pub fn auth_kind(&self) -> AuthKind {
        self.auth_kind
    }

    pub fn api_key_header(&self) -> Option<&ApiKeyHeader> {
        self.api_key_header.as_ref()
    }

    pub fn operations(&self) -> &Operations {
        &self.operations
    }

    fn validate(&self) -> Result<(), Error> {
        if !https_url(&self.openapi_url) {
            return Err(Error::invalid(
                "OpenAPI URL must be HTTPS with a host and no userinfo, query or fragment",
            ));
        }
        if self.document_sha256.len() != 64
            || !self
                .document_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(Error::invalid(
                "OpenAPI digest must be 64 lower-case hex digits",
            ));
        }
        if !https_url(&self.server_url) {
            return Err(Error::invalid(
                "control-plane server must be HTTPS with a host and no userinfo, query or fragment",
            ));
        }
        validate_auth(self.auth_kind, self.api_key_header.as_ref())?;
        self.operations.validate()
    }
}

/// The `operationId` of each fixed control-plane role.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operations {
    create_instance: String,
    list_instances: String,
    get_instance: String,
    delete_instance: String,
}

impl Operations {
    pub fn create_instance(&self) -> &str {
        &self.create_instance
    }

    pub fn list_instances(&self) -> &str {
        &self.list_instances
    }

    pub fn get_instance(&self) -> &str {
        &self.get_instance
    }

    pub fn delete_instance(&self) -> &str {
        &self.delete_instance
    }

    /// `(role, operationId)` in the order create, list, get, delete.
    pub fn roles(&self) -> [(&'static str, &str); 4] {
        [
            ("create_instance", self.create_instance.as_str()),
            ("list_instances", self.list_instances.as_str()),
            ("get_instance", self.get_instance.as_str()),
            ("delete_instance", self.delete_instance.as_str()),
        ]
    }

    fn validate(&self) -> Result<(), Error> {
        let roles = self.roles();
        for (index, (_, operation)) in roles.iter().enumerate() {
            if operation.is_empty()
                || operation.len() > MAX_OPERATION_ID_BYTES
                || !operation
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
            {
                return Err(Error::invalid(
                    "operation ID must be 1-128 bytes of ASCII letters, digits, '_', '.' or '-'",
                ));
            }
            if roles[..index]
                .iter()
                .any(|(_, earlier)| earlier == operation)
            {
                return Err(Error::invalid(
                    "two control-plane roles name the same operation",
                ));
            }
        }
        Ok(())
    }
}

fn validate_auth(kind: AuthKind, header: Option<&ApiKeyHeader>) -> Result<(), Error> {
    if kind == AuthKind::SubscriptionOauth {
        return Err(Error::invalid(
            "provider descriptions describe API access, not subscription OAuth",
        ));
    }
    if (kind == AuthKind::ApiKey) != header.is_some() {
        return Err(Error::invalid(
            "exactly API-key authentication requires an API-key header",
        ));
    }
    Ok(())
}

fn valid_instance(instance: &str) -> bool {
    !instance.is_empty()
        && instance.len() <= MAX_INSTANCE_BYTES
        && instance
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

fn https_url(value: &str) -> bool {
    if value.len() > 4096 || value.bytes().any(|b| b.is_ascii_control() || b == b' ') {
        return false;
    }
    Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
    })
}

fn validate_template(template: &str) -> Result<(), Error> {
    let placeholder_message = "base URL template must hold {instance} exactly once, inside the host's first label, and no other brace";
    if template.matches(PLACEHOLDER).count() != 1
        || template.replace(PLACEHOLDER, "").contains(['{', '}'])
    {
        return Err(Error::invalid(placeholder_message));
    }
    let (Some(authority), Some(position)) = (template.find("://"), template.find(PLACEHOLDER))
    else {
        return Err(Error::invalid(placeholder_message));
    };
    let host_start = authority + 3;
    let first_label_end = template[host_start..]
        .find(['.', ':', '/', '@', '?', '#'])
        .map_or(template.len(), |end| host_start + end);
    if position < host_start || position + PLACEHOLDER.len() > first_label_end {
        return Err(Error::invalid(placeholder_message));
    }
    // A fixed domain follows the instance's label, so no instance names a single-label host.
    if !template[first_label_end..].starts_with('.') {
        return Err(Error::invalid(
            "base URL template must keep a fixed domain after the instance's label",
        ));
    }
    // The shortest and the longest instance must both give an endpoint URL.
    let longest = "0".repeat(MAX_INSTANCE_BYTES);
    for probe in ["0", longest.as_str()] {
        fill(template, probe)
            .map_err(|_| Error::invalid("base URL template does not form a valid endpoint URL"))?;
    }
    Ok(())
}

fn fill(template: &str, instance: &str) -> Result<BaseUrl, Error> {
    BaseUrl::new(&template.replacen(PLACEHOLDER, instance, 1))
}

/// Descriptions shipped with this crate.
pub mod descriptions {
    use super::ProviderDescription;

    const RUNPOD: &str = include_str!("../descriptions/runpod.toml");

    /// Runpod: pods serving vLLM's `OpenAI`-compatible server, and the Runpod REST control plane.
    ///
    /// # Panics
    /// Never: the shipped document is validated by this crate's tests.
    pub fn runpod() -> ProviderDescription {
        ProviderDescription::parse(RUNPOD).expect("the shipped Runpod description is valid")
    }

    /// The shipped description with this name, if there is one.
    pub fn by_name(name: &str) -> Option<ProviderDescription> {
        match name {
            "runpod" => Some(runpod()),
            _ => None,
        }
    }
}
