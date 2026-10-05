use http::HeaderName;
use llm_core::{AuthKind, BillingKind, Capabilities, Error, Id, Protocol};
use llm_credentials::SecretRef;
use serde::{Deserialize, Deserializer, Serialize};
use url::Url;

/// An explicitly selected HTTP API prefix. It contains no credential-bearing URL components.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct BaseUrl(String);

impl BaseUrl {
    /// # Errors
    /// Refuses oversized URLs, non-HTTP schemes, userinfo, query, fragment and control bytes.
    pub fn new(value: &str) -> Result<Self, Error> {
        if value.len() > 4096 || value.bytes().any(|b| b.is_ascii_control() || b == b' ') {
            return Err(Error::invalid(
                "endpoint URL is oversized or contains whitespace",
            ));
        }
        let mut url = Url::parse(value).map_err(|_| Error::invalid("invalid endpoint URL"))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(Error::invalid(
                "endpoint must be HTTP(S) with a host and no userinfo, query or fragment",
            ));
        }
        // Treat /v1 and /v1/ identically without losing an explicitly configured prefix.
        if !url.path().ends_with('/') {
            let path = format!("{}/", url.path());
            url.set_path(&path);
        }
        Ok(Self(url.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn request_url(&self, protocol: Protocol) -> String {
        let path = match protocol {
            Protocol::Responses => "responses",
            Protocol::Messages => "messages",
            Protocol::ChatCompletions => "chat/completions",
        };
        format!("{}{path}", self.0)
    }
}

impl<'de> Deserialize<'de> for BaseUrl {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::new(&value).map_err(serde::de::Error::custom)
    }
}

/// An HTTP authentication header name, independent of protocol selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ApiKeyHeader(String);

impl ApiKeyHeader {
    /// # Errors
    /// Refuses invalid/oversized names and HTTP routing/framing or content-negotiation headers.
    pub fn new(value: &str) -> Result<Self, Error> {
        if value.len() > 128 {
            return Err(Error::invalid("API-key header name exceeds its bound"));
        }
        let header = HeaderName::from_bytes(value.as_bytes())
            .map_err(|_| Error::invalid("invalid API-key header name"))?;
        if matches!(
            header.as_str(),
            "host"
                | "content-type"
                | "content-length"
                | "content-encoding"
                | "transfer-encoding"
                | "connection"
                | "upgrade"
                | "te"
                | "trailer"
                | "accept"
                | "accept-encoding"
                | "proxy-authorization"
                | "cookie"
        ) {
            return Err(Error::invalid(
                "API-key header cannot alter HTTP request semantics",
            ));
        }
        Ok(Self(header.as_str().to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ApiKeyHeader {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::new(&value).map_err(serde::de::Error::custom)
    }
}

/// Operator-owned identity and descriptive category. Neither selects a protocol or vendor preset.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub id: Id,
    pub category: Id,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Account {
    pub id: Id,
    pub provider_id: Id,
    pub auth_kind: AuthKind,
    pub billing_kind: BillingKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_reference_id: Option<SecretRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_header: Option<ApiKeyHeader>,
}

impl Account {
    /// # Errors
    /// Requires explicit anonymous access or a referenced credential, with no contradictory fields.
    pub fn validate(&self) -> Result<(), Error> {
        match self.auth_kind {
            AuthKind::Anonymous => {
                if self.secret_reference_id.is_some() || self.api_key_header.is_some() {
                    return Err(Error::invalid(
                        "anonymous accounts cannot name credential fields",
                    ));
                }
            }
            AuthKind::Bearer | AuthKind::ApiKey | AuthKind::SubscriptionOauth => {
                if self.secret_reference_id.is_none() {
                    return Err(Error::invalid(
                        "authenticated accounts require a secret reference",
                    ));
                }
                if (self.auth_kind == AuthKind::ApiKey) != self.api_key_header.is_some() {
                    return Err(Error::invalid(
                        "exactly API-key accounts require an API-key header",
                    ));
                }
            }
        }
        // A subscription token is never billed as metered API use.
        if self.auth_kind == AuthKind::SubscriptionOauth
            && self.billing_kind != BillingKind::Subscription
        {
            return Err(Error::invalid(
                "subscription OAuth accounts require subscription billing",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub id: Id,
    pub account_id: Id,
    pub base_url: BaseUrl,
}

/// Model catalog identity and the exact name sent upstream; not a built-in model-name enum.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServedModel {
    pub id: Id,
    pub upstream_name: Id,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServingModel {
    pub id: Id,
    pub endpoint_id: Id,
    pub model_id: Id,
    pub protocol: Protocol,
    pub capabilities: Capabilities,
}
