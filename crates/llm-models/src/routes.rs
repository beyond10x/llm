//! `llm.routes/1`: the short YAML document every consumer reads to learn which model a name
//! reaches, and how.
//!
//! A provider names its base URL, its wire protocol and how it authenticates; its secret is only
//! a source reference (`env:<VARIABLE>`, `file:<path>` or `keychain:<service>/<entry>`). A route
//! names one `<provider>/<model-id>`, split on the first `/` only, or a mapping with that `model`
//! and its `capabilities`. A name that is not a route but reads `<declared provider>/<model-id>`
//! resolves directly.
//!
//! ```yaml
//! format: llm.routes/1
//! providers:
//!   anthropic:  {base_url: https://api.anthropic.com/v1, protocol: messages,
//!                auth: {kind: api-key, header: x-api-key, secret: env:ANTHROPIC_API_KEY}}
//!   lab:        {base_url: http://127.0.0.1:8000/v1, protocol: chat-completions,
//!                auth: {kind: anonymous}}
//! routes:
//!   sonnet: anthropic/claude-sonnet-4-5
//!   small:  {model: lab/small, capabilities: {tools: false, tool_choice: false,
//!            temperature: true, top_p: true, reasoning_efforts: [], context_window: 8192,
//!            max_output_tokens: 2048}}
//! ```
//!
//! The document expands into an `llm.catalog/1` catalog: per provider one provider, account and
//! endpoint, each with the provider's name as id, and the account's secret reference id is the
//! provider's name too; per route one served model, one serving model and one single-target
//! route without fallback, each with the route's name as id. Ports are then built by
//! [`crate::port`] and [`crate::CatalogModels::build`] on their unchanged rules.
//!
//! Parsing is strict and performs no I/O: an unknown field, a repeated key, an unknown format,
//! protocol, authentication kind or secret source, a literal secret and a route to an undeclared
//! provider are refused as `invalid-request`, naming the field, provider or route and never
//! echoing a value. [`Routes::resolvers`] turns the references into the caller's resolvers
//! without reading anything; an unset variable or a missing file is refused at turn time, as
//! `unauthorized`, by the provider's reference, before any request is sent.

use crate::{Port, Resolvers, port};
use llm_core::{AuthKind, BillingKind, Capabilities, Error, ErrorCode, Id, Protocol};
use llm_credentials::{SecretRef, SecretResolver};
use llm_http::HttpClient;
use llm_providers::{
    Account, ApiKeyHeader, BaseUrl, Binding, Endpoint, Provider, ServedModel, ServingModel,
};
use llm_routing::{Catalog, CatalogDocument, MAX_CONFIG_BYTES, Route, RouteTarget};
use serde::{
    Deserialize, Deserializer,
    de::{self, DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor},
};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    fmt,
    marker::PhantomData,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

/// The format a routes document declares in its `format` field.
pub const FORMAT: &str = "llm.routes/1";

const DEFAULT_CONTEXT_WINDOW: u64 = 200_000;
const DEFAULT_MAX_OUTPUT_TOKENS: u64 = 16_384;
const MAX_PATH_BYTES: usize = 4096;
const DUPLICATE_KEY: &str = "llm.routes duplicate mapping key";
const EXPANSION: &str = "llm.routes expansion bound";
const EXPANDED: &str =
    "llm.routes/1 document expands beyond its byte bound when its YAML aliases are resolved";
const CATALOG_BOUND: &str = "llm.routes/1 document expands beyond the catalog's bound";
const SECRET_FORMS: &str = "`auth.secret` must be a reference: env:<VARIABLE>, \
file:<absolute or ~/ path> or keychain:<service>/<entry>";

/// The capabilities a route written as a plain `<provider>/<model-id>` string declares: every
/// boolean true, reasoning efforts `low`, `medium` and `high`, a context window of 200000 tokens
/// and at most 16384 output tokens. A route whose model differs writes its own `capabilities`.
pub fn default_capabilities() -> Capabilities {
    Capabilities {
        tools: true,
        tool_choice: true,
        temperature: true,
        top_p: true,
        reasoning_efforts: ["low", "medium", "high"].map(str::to_owned).into(),
        context_window: DEFAULT_CONTEXT_WINDOW,
        max_output_tokens: DEFAULT_MAX_OUTPUT_TOKENS,
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDocument {
    format: String,
    providers: Unique<RawProvider>,
    #[serde(default)]
    routes: Unique<RawRoute>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProvider {
    base_url: String,
    protocol: String,
    auth: RawAuth,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAuth {
    kind: String,
    #[serde(default)]
    header: Option<String>,
    #[serde(default)]
    secret: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawRoute {
    Model(String),
    Declared(RawDeclaredRoute),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDeclaredRoute {
    model: String,
    capabilities: Capabilities,
}

/// A mapping in document order that refuses a repeated key, which a plain map would let the
/// last occurrence silently win.
struct Unique<V>(Vec<(String, V)>);

impl<V> Default for Unique<V> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<'de, V: Deserialize<'de>> Deserialize<'de> for Unique<V> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Entries<V>(PhantomData<V>);
        impl<'de, V: Deserialize<'de>> Visitor<'de> for Entries<V> {
            type Value = Unique<V>;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a mapping with unique keys")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut seen = BTreeSet::new();
                let mut entries = Vec::new();
                while let Some(key) = map.next_key::<String>()? {
                    if !seen.insert(key.clone()) {
                        return Err(de::Error::custom(DUPLICATE_KEY));
                    }
                    entries.push((key, map.next_value()?));
                }
                Ok(Unique(entries))
            }
        }
        deserializer.deserialize_map(Entries(PhantomData))
    }
}

/// The YAML parser's own diagnostic can quote the document, so only its location is kept.
fn malformed(error: &serde_yaml::Error) -> Error {
    if error.to_string().contains(EXPANSION) {
        return Error::too_large(EXPANDED);
    }
    let repeated = if error.to_string().contains(DUPLICATE_KEY) {
        ": a mapping key is repeated"
    } else {
        ""
    };
    let location = error.location().map_or(String::new(), |location| {
        format!(" at line {} column {}", location.line(), location.column())
    });
    Error::invalid(format!(
        "invalid {FORMAT} YAML document{repeated}{location}"
    ))
}

/// How many bytes the document may present once every YAML alias is replayed: twice its own
/// bound, which a document without aliases stays within (its decoded text is never longer than
/// its source, and every node it holds costs one byte below).
const MAX_EXPANDED_BYTES: usize = 2 * MAX_CONFIG_BYTES;

/// Walks the document as the YAML parser presents it, every alias replayed, and keeps nothing.
///
/// The parser replays an alias from the events it already holds, without copying them, so this
/// walk holds no more than the parser does. Deserializing into owned values would instead copy
/// the anchored text once per alias: a short document could cost far more than its byte bound
/// before any of it was refused. Each node costs its text plus one byte, so many small repeated
/// nodes are bounded too, and the walk stops at [`MAX_EXPANDED_BYTES`].
#[derive(Clone, Copy)]
struct Measure<'a>(&'a Cell<usize>);

impl Measure<'_> {
    fn charge<E: de::Error>(self, text: usize) -> Result<(), E> {
        let left = self
            .0
            .get()
            .checked_sub(text.saturating_add(1))
            .ok_or_else(|| E::custom(EXPANSION))?;
        self.0.set(left);
        Ok(())
    }
}

impl<'de> DeserializeSeed<'de> for Measure<'_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Measure<'_> {
    type Value = ();
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a YAML node")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> Result<(), E> {
        self.charge(0)
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> Result<(), E> {
        self.charge(0)
    }
    fn visit_i128<E: de::Error>(self, _: i128) -> Result<(), E> {
        self.charge(0)
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> Result<(), E> {
        self.charge(0)
    }
    fn visit_u128<E: de::Error>(self, _: u128) -> Result<(), E> {
        self.charge(0)
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<(), E> {
        self.charge(0)
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<(), E> {
        self.charge(value.len())
    }
    fn visit_bytes<E: de::Error>(self, value: &[u8]) -> Result<(), E> {
        self.charge(value.len())
    }
    fn visit_none<E: de::Error>(self) -> Result<(), E> {
        self.charge(0)
    }
    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        self.charge(0)
    }
    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        self.charge(0)?;
        deserializer.deserialize_any(self)
    }
    fn visit_newtype_struct<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        self.charge(0)?;
        deserializer.deserialize_any(self)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        self.charge(0)?;
        while seq.next_element_seed(self)?.is_some() {}
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        self.charge(0)?;
        while map.next_key_seed(self)?.is_some() {
            map.next_value_seed(self)?;
        }
        Ok(())
    }
    /// A tagged node (`!tag value`): the tag and its value both count.
    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<(), A::Error> {
        let ((), value) = data.variant_seed(self)?;
        value.newtype_variant_seed(self)
    }
}

/// A provider or route name: a valid identifier holding no `/`.
fn name(value: &str, kind: &str) -> Result<Id, Error> {
    Id::new(value)
        .ok()
        .filter(|_| !value.contains('/'))
        .ok_or_else(|| {
            Error::invalid(format!(
                "a {kind} name must be 1..=256 printable ASCII bytes without `/`"
            ))
        })
}

fn about(kind: &str, name: &Id, message: &str) -> Error {
    Error::invalid(format!("{kind} `{name}`: {message}"))
}

/// Where a `file:` secret lives: an absolute path, or one below the home the caller supplies.
#[derive(Clone)]
#[cfg_attr(
    not(feature = "file"),
    expect(dead_code, reason = "read only by the file source, compiled out here")
)]
enum FilePath {
    Absolute(PathBuf),
    Home(PathBuf),
}

impl FilePath {
    fn read(value: &str) -> Option<Self> {
        if value.len() > MAX_PATH_BYTES || value.chars().any(char::is_control) {
            return None;
        }
        let (relative, path) = match value.strip_prefix("~/") {
            Some(rest) => (true, Path::new(rest)),
            None if value.starts_with('/') => (false, Path::new(value)),
            None => return None,
        };
        let plain = path.components().all(|component| match component {
            Component::Normal(_) => true,
            Component::RootDir => !relative,
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => false,
        });
        (plain && path.file_name().is_some()).then(|| {
            if relative {
                Self::Home(path.to_owned())
            } else {
                Self::Absolute(path.to_owned())
            }
        })
    }
}

/// A provider's secret reference. Its `Debug` names only the source kind: the variable name,
/// path and keychain entry stay out of every diagnostic.
#[derive(Clone)]
enum SecretSource {
    Environment(String),
    File(FilePath),
    Keychain { service: Id, entry: Id },
}

impl SecretSource {
    fn read(value: &str) -> Option<Self> {
        if let Some(variable) = value.strip_prefix("env:") {
            let usable = !variable.is_empty()
                && variable.len() <= 256
                && variable
                    .bytes()
                    .all(|byte| byte.is_ascii_graphic() && byte != b'=');
            return usable.then(|| Self::Environment(variable.to_owned()));
        }
        if let Some(path) = value.strip_prefix("file:") {
            return FilePath::read(path).map(Self::File);
        }
        let (service, entry) = value.strip_prefix("keychain:")?.split_once('/')?;
        Some(Self::Keychain {
            service: Id::new(service).ok()?,
            entry: Id::new(entry).ok()?,
        })
    }

    fn kind(&self) -> SecretKind {
        match self {
            Self::Environment(_) => SecretKind::Environment,
            Self::File(_) => SecretKind::File,
            Self::Keychain { .. } => SecretKind::Keychain,
        }
    }
}

impl fmt::Debug for SecretSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretSource({}:[EXPLICIT])", self.kind().as_str())
    }
}

/// The source kind of a provider's secret reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKind {
    /// `env:<VARIABLE>`
    Environment,
    /// `file:<absolute or ~/ path>`
    File,
    /// `keychain:<service>/<entry>`
    Keychain,
}

impl SecretKind {
    /// The reference's prefix without its colon: `env`, `file` or `keychain`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Environment => "env",
            Self::File => "file",
            Self::Keychain => "keychain",
        }
    }
}

/// One declared provider, as the document states it.
#[derive(Debug, Clone)]
pub struct ProviderDeclaration {
    name: Id,
    base_url: BaseUrl,
    protocol: Protocol,
    auth_kind: AuthKind,
    api_key_header: Option<ApiKeyHeader>,
    secret: Option<SecretSource>,
}

impl ProviderDeclaration {
    fn read(name: Id, raw: RawProvider) -> Result<Self, Error> {
        let refuse = |message: &str| about("provider", &name, message);
        let base_url = BaseUrl::new(&raw.base_url).map_err(|error| refuse(&error.message))?;
        let protocol = match raw.protocol.as_str() {
            "messages" => Protocol::Messages,
            "responses" => Protocol::Responses,
            "chat-completions" => Protocol::ChatCompletions,
            _ => {
                return Err(refuse(
                    "unknown `protocol`; expected messages, responses or chat-completions",
                ));
            }
        };
        let auth_kind = match raw.auth.kind.as_str() {
            "anonymous" => AuthKind::Anonymous,
            "bearer" => AuthKind::Bearer,
            "api-key" => AuthKind::ApiKey,
            "subscription-oauth" => {
                return Err(refuse(
                    "`auth.kind` names a subscription OAuth login, which a routes document cannot declare",
                ));
            }
            _ => {
                return Err(refuse(
                    "unknown `auth.kind`; expected anonymous, bearer or api-key",
                ));
            }
        };
        let api_key_header = match (auth_kind, raw.auth.header) {
            (AuthKind::ApiKey, Some(header)) => {
                Some(ApiKeyHeader::new(&header).map_err(|error| refuse(&error.message))?)
            }
            (AuthKind::ApiKey, None) => {
                return Err(refuse("an api-key provider requires `auth.header`"));
            }
            (_, Some(_)) => {
                return Err(refuse("only an api-key provider takes `auth.header`"));
            }
            (_, None) => None,
        };
        let secret = match (auth_kind, raw.auth.secret) {
            (AuthKind::Anonymous, Some(_)) => {
                return Err(refuse("an anonymous provider cannot name `auth.secret`"));
            }
            (AuthKind::Anonymous, None) => None,
            (_, None) => {
                return Err(refuse("a credentialed provider requires `auth.secret`"));
            }
            (_, Some(secret)) => {
                Some(SecretSource::read(&secret).ok_or_else(|| refuse(SECRET_FORMS))?)
            }
        };
        Ok(Self {
            name,
            base_url,
            protocol,
            auth_kind,
            api_key_header,
            secret,
        })
    }

    /// The provider's name; also its account, endpoint and secret reference id.
    pub fn name(&self) -> &Id {
        &self.name
    }

    pub fn base_url(&self) -> &BaseUrl {
        &self.base_url
    }

    pub fn protocol(&self) -> Protocol {
        self.protocol
    }

    pub fn auth_kind(&self) -> AuthKind {
        self.auth_kind
    }

    /// The header an api-key provider's credential travels in.
    pub fn api_key_header(&self) -> Option<&ApiKeyHeader> {
        self.api_key_header.as_ref()
    }

    /// The source kind of the secret reference; `None` for an anonymous provider.
    pub fn secret_kind(&self) -> Option<SecretKind> {
        self.secret.as_ref().map(SecretSource::kind)
    }

    fn reference(&self) -> Result<SecretRef, Error> {
        SecretRef::new(self.name.as_str())
            .map_err(|_| about("provider", &self.name, "its name is not a secret reference"))
    }

    fn declarations(&self) -> Result<(Provider, Account, Endpoint), Error> {
        let anonymous = self.auth_kind == AuthKind::Anonymous;
        let category = Id::new(if anonymous { "self-hosted" } else { "hosted" })
            .map_err(|_| Error::invalid("invalid provider category"))?;
        let provider = Provider {
            id: self.name.clone(),
            category,
        };
        let account = Account {
            id: self.name.clone(),
            provider_id: self.name.clone(),
            auth_kind: self.auth_kind,
            billing_kind: if anonymous {
                BillingKind::SelfHosted
            } else {
                BillingKind::Metered
            },
            secret_reference_id: self.secret.as_ref().map(|_| self.reference()).transpose()?,
            api_key_header: self.api_key_header.clone(),
        };
        let endpoint = Endpoint {
            id: self.name.clone(),
            account_id: self.name.clone(),
            base_url: self.base_url.clone(),
        };
        Ok((provider, account, endpoint))
    }
}

/// One declared route, as the document states it.
#[derive(Debug, Clone)]
pub struct RouteDeclaration {
    name: Id,
    provider: Id,
    upstream_model: Id,
    capabilities: Capabilities,
}

impl RouteDeclaration {
    /// The route's name; also its serving model, served model and catalog route id and alias.
    pub fn name(&self) -> &Id {
        &self.name
    }

    pub fn provider(&self) -> &Id {
        &self.provider
    }

    /// The model id sent upstream: everything after the first `/` of the route's model.
    pub fn upstream_model(&self) -> &Id {
        &self.upstream_model
    }

    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }
}

/// What a name reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The serving model id that answers the name: the route's name, or `<provider>/<model-id>`
    /// for a direct name.
    pub serving_model: Id,
    pub provider: Id,
    pub upstream_model: Id,
    pub protocol: Protocol,
}

/// A validated `llm.routes/1` document and the catalog it expands into.
#[derive(Debug)]
pub struct Routes {
    providers: BTreeMap<Id, ProviderDeclaration>,
    declared: BTreeMap<Id, RouteDeclaration>,
    catalog: Catalog,
}

/// One serving model the expansion declares.
struct Served<'a> {
    id: &'a Id,
    provider: &'a ProviderDeclaration,
    upstream_model: &'a Id,
    capabilities: &'a Capabilities,
}

fn expand<'a>(
    providers: impl IntoIterator<Item = &'a ProviderDeclaration>,
    served: &[Served<'_>],
) -> Result<Catalog, Error> {
    let mut declared = (Vec::new(), Vec::new(), Vec::new());
    for provider in providers {
        let (provider, account, endpoint) = provider.declarations()?;
        declared.0.push(provider);
        declared.1.push(account);
        declared.2.push(endpoint);
    }
    let (providers, accounts, endpoints) = declared;
    let mut models = Vec::with_capacity(served.len());
    let mut serving_models = Vec::with_capacity(served.len());
    let mut routes = Vec::with_capacity(served.len());
    let mut targets = Vec::with_capacity(served.len());
    for entry in served {
        models.push(ServedModel {
            id: entry.id.clone(),
            upstream_name: entry.upstream_model.clone(),
        });
        serving_models.push(ServingModel {
            id: entry.id.clone(),
            endpoint_id: entry.provider.name.clone(),
            model_id: entry.id.clone(),
            protocol: entry.provider.protocol,
            capabilities: entry.capabilities.clone(),
        });
        routes.push(Route {
            id: entry.id.clone(),
            alias: entry.id.clone(),
            fallback_enabled: false,
        });
        targets.push(RouteTarget {
            id: entry.id.clone(),
            route_id: entry.id.clone(),
            serving_model_id: entry.id.clone(),
            position: 0,
        });
    }
    CatalogDocument::new(
        providers,
        accounts,
        endpoints,
        models,
        serving_models,
        routes,
        targets,
    )
    .validate()
    .map_err(|error| {
        // The catalog bounds its declarations; the caller wrote a routes document, so the
        // refusal names that document rather than the catalog it was never shown.
        if error.code == ErrorCode::TooLarge {
            Error::too_large(CATALOG_BOUND)
        } else {
            error
        }
    })
}

impl Routes {
    /// Reads and validates a routes document and expands it into its catalog. Performs no I/O.
    ///
    /// # Errors
    /// `too-large` for a document above [`MAX_CONFIG_BYTES`], for one whose YAML aliases repeat
    /// it past twice that bound while it is read, and for one whose expansion exceeds the
    /// catalog's own bounds (`MAX_CONFIG_BYTES` of declarations, 4096 of each kind: a few
    /// thousand routes); otherwise `invalid-request` for malformed YAML,
    /// an unknown field, a repeated key, an unknown format, protocol, authentication kind or
    /// secret source, a literal secret, contradictory authentication fields, an invalid name,
    /// base URL, header or capability declaration, and a route to an undeclared provider. The
    /// message names the field, provider or route and never a value.
    pub fn parse(source: &str) -> Result<Self, Error> {
        if source.len() > MAX_CONFIG_BYTES {
            return Err(Error::too_large("routes document exceeds its byte bound"));
        }
        let budget = Cell::new(MAX_EXPANDED_BYTES);
        Measure(&budget)
            .deserialize(serde_yaml::Deserializer::from_str(source))
            .map_err(|error| malformed(&error))?;
        let raw: RawDocument = serde_yaml::from_str(source).map_err(|error| malformed(&error))?;
        if raw.format != FORMAT {
            return Err(Error::invalid(format!(
                "the document's `format` is not {FORMAT}"
            )));
        }
        let mut providers = BTreeMap::new();
        for (provider, declaration) in raw.providers.0 {
            let provider = name(&provider, "provider")?;
            let declaration = ProviderDeclaration::read(provider.clone(), declaration)?;
            providers.insert(provider, declaration);
        }
        let mut routes = BTreeMap::new();
        for (route, declaration) in raw.routes.0 {
            let route = name(&route, "route")?;
            let refuse = |message: &str| about("route", &route, message);
            let (model, capabilities) = match declaration {
                RawRoute::Model(model) => (model, default_capabilities()),
                RawRoute::Declared(declared) => (declared.model, declared.capabilities),
            };
            let (provider, upstream_model) = model
                .split_once('/')
                .ok_or_else(|| refuse("its model must read <provider>/<model-id>"))?;
            let provider = Id::new(provider)
                .ok()
                .filter(|provider| providers.contains_key(provider))
                .ok_or_else(|| refuse("its model names an undeclared provider"))?;
            let upstream_model = Id::new(upstream_model)
                .map_err(|_| refuse("its model id must be 1..=256 printable ASCII bytes"))?;
            capabilities
                .validate()
                .map_err(|error| refuse(&error.message))?;
            routes.insert(
                route.clone(),
                RouteDeclaration {
                    name: route,
                    provider,
                    upstream_model,
                    capabilities,
                },
            );
        }
        let served: Vec<Served<'_>> = routes
            .values()
            .map(|route| Served {
                id: &route.name,
                provider: &providers[&route.provider],
                upstream_model: &route.upstream_model,
                capabilities: &route.capabilities,
            })
            .collect();
        let catalog = expand(providers.values(), &served)?;
        Ok(Self {
            providers,
            declared: routes,
            catalog,
        })
    }

    /// The `llm.catalog/1` catalog the document expands into: its serving models are the routes.
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// Every declared provider, in name order.
    pub fn providers(&self) -> impl Iterator<Item = &ProviderDeclaration> {
        self.providers.values()
    }

    /// Every declared route, in name order.
    pub fn routes(&self) -> impl Iterator<Item = &RouteDeclaration> {
        self.declared.values()
    }

    fn route(&self, name: &str) -> Option<&RouteDeclaration> {
        Id::new(name).ok().and_then(|id| self.declared.get(&id))
    }

    /// A `<declared provider>/<model-id>` name: its serving model id, provider and model id.
    fn direct(&self, name: &str) -> Result<(Id, &ProviderDeclaration, Id), Error> {
        let unknown =
            || Error::invalid("the name is neither a route nor <declared provider>/<model-id>");
        let (provider, model) = name.split_once('/').ok_or_else(unknown)?;
        let provider = Id::new(provider)
            .ok()
            .and_then(|provider| self.providers.get(&provider))
            .ok_or_else(unknown)?;
        let model = Id::new(model).map_err(|_| unknown())?;
        let serving = Id::new(name).map_err(|_| unknown())?;
        Ok((serving, provider, model))
    }

    /// The one-entry catalog a direct name expands into, by the same rules as a route.
    fn direct_catalog(&self, name: &str) -> Result<(Catalog, Id), Error> {
        let (serving, provider, model) = self.direct(name)?;
        let capabilities = default_capabilities();
        let catalog = expand(
            [provider],
            &[Served {
                id: &serving,
                provider,
                upstream_model: &model,
                capabilities: &capabilities,
            }],
        )?;
        Ok((catalog, serving))
    }

    /// What a name reaches: a route first; else `<declared provider>/<model-id>`, split on the
    /// first `/`.
    ///
    /// # Errors
    /// `invalid-request` for any other name, without echoing it.
    pub fn target(&self, name: &str) -> Result<Target, Error> {
        if let Some(route) = self.route(name) {
            let provider = &self.providers[&route.provider];
            return Ok(Target {
                serving_model: route.name.clone(),
                provider: provider.name.clone(),
                upstream_model: route.upstream_model.clone(),
                protocol: provider.protocol,
            });
        }
        let (serving_model, provider, upstream_model) = self.direct(name)?;
        Ok(Target {
            serving_model,
            provider: provider.name.clone(),
            upstream_model,
            protocol: provider.protocol,
        })
    }

    /// The validated binding a name reaches, as [`Routes::target`] resolves it. A direct name
    /// takes the default capabilities.
    ///
    /// # Errors
    /// As [`Routes::target`].
    pub fn binding(&self, name: &str) -> Result<Binding, Error> {
        let absent = || Error::invalid("no such serving model in the catalog");
        if let Some(route) = self.route(name) {
            return self
                .catalog
                .binding(&route.name)
                .cloned()
                .ok_or_else(absent);
        }
        let (catalog, serving) = self.direct_catalog(name)?;
        catalog.binding(&serving).cloned().ok_or_else(absent)
    }

    /// Builds the port a name reaches with [`crate::port`]: a route over the expanded catalog, a
    /// direct name over the one-entry catalog it expands into, so the rules are identical.
    ///
    /// # Errors
    /// As [`Routes::target`], and as [`crate::port`] refuses a credentialed provider the caller
    /// has no resolver for (`unauthorized`).
    pub fn port(
        &self,
        name: &str,
        http: HttpClient,
        resolvers: &dyn Resolvers,
    ) -> Result<Port, Error> {
        if let Some(route) = self.route(name) {
            return port(&self.catalog, &route.name, http, resolvers);
        }
        let (catalog, serving) = self.direct_catalog(name)?;
        port(&catalog, &serving, http, resolvers)
    }

    /// The caller's resolvers for every credentialed provider, keyed by the provider's name.
    /// Building them reads no variable, file or keychain entry.
    ///
    /// # Errors
    /// `invalid-request` naming the provider for a source kind whose feature (`environment`,
    /// `file`, `keychain`) this build does not enable, a `file:~/` reference without a home, a
    /// `keychain:` reference without an injected store, and a path the file source refuses.
    pub fn resolvers(&self, sources: &SecretSources) -> Result<RouteResolvers, Error> {
        let mut resolvers = BTreeMap::new();
        for provider in self.providers.values() {
            let Some(secret) = &provider.secret else {
                continue;
            };
            let reference = provider.reference()?;
            let resolver = match secret {
                SecretSource::Environment(variable) => {
                    environment(&provider.name, &reference, variable)?
                }
                SecretSource::File(path) => {
                    file(&provider.name, &reference, path, sources.home.as_deref())?
                }
                SecretSource::Keychain { service, entry } => {
                    keychain(&provider.name, &reference, service, entry, sources)?
                }
            };
            resolvers.insert(provider.name.clone(), resolver);
        }
        Ok(RouteResolvers { resolvers })
    }
}

#[cfg(any(
    not(feature = "environment"),
    not(feature = "file"),
    not(feature = "keychain")
))]
fn compiled_out(provider: &Id, source: &str, feature: &str) -> Error {
    about(
        "provider",
        provider,
        &format!(
            "`{source}` secrets need feature `{feature}` of b10x-llm-models, which this build does not enable"
        ),
    )
}

#[cfg(feature = "environment")]
fn environment(
    provider: &Id,
    reference: &SecretRef,
    variable: &str,
) -> Result<Arc<dyn SecretResolver>, Error> {
    llm_credentials::environment::EnvironmentResolver::new(BTreeMap::from([(
        reference.clone(),
        variable.to_owned(),
    )]))
    .map(|resolver| Arc::new(resolver) as Arc<dyn SecretResolver>)
    .map_err(|_| {
        about(
            "provider",
            provider,
            "its `env:` variable name is not usable",
        )
    })
}

#[cfg(not(feature = "environment"))]
fn environment(
    provider: &Id,
    _reference: &SecretRef,
    _variable: &str,
) -> Result<Arc<dyn SecretResolver>, Error> {
    Err(compiled_out(provider, "env:", "environment"))
}

#[cfg(feature = "file")]
fn file(
    provider: &Id,
    reference: &SecretRef,
    path: &FilePath,
    home: Option<&Path>,
) -> Result<Arc<dyn SecretResolver>, Error> {
    let path = match path {
        FilePath::Absolute(path) => path.clone(),
        FilePath::Home(relative) => home
            .ok_or_else(|| {
                about(
                    "provider",
                    provider,
                    "a `file:~/` secret needs a home directory, and none was supplied",
                )
            })?
            .join(relative),
    };
    llm_credentials::file::FileResolver::new(BTreeMap::from([(reference.clone(), path)]))
        .map(|resolver| Arc::new(resolver) as Arc<dyn SecretResolver>)
        .map_err(|_| {
            about(
                "provider",
                provider,
                "its `file:` secret does not expand to an absolute path without `.` or `..`",
            )
        })
}

#[cfg(not(feature = "file"))]
fn file(
    provider: &Id,
    _reference: &SecretRef,
    _path: &FilePath,
    _home: Option<&Path>,
) -> Result<Arc<dyn SecretResolver>, Error> {
    Err(compiled_out(provider, "file:", "file"))
}

#[cfg(feature = "keychain")]
fn keychain(
    provider: &Id,
    reference: &SecretRef,
    service: &Id,
    entry: &Id,
    sources: &SecretSources,
) -> Result<Arc<dyn SecretResolver>, Error> {
    use llm_credentials::keychain::{KeychainEntry, KeychainResolver};
    let store = sources.keychain.clone().ok_or_else(|| {
        about(
            "provider",
            provider,
            "a `keychain:` secret needs an injected credential store, and none was supplied",
        )
    })?;
    KeychainResolver::new(
        store,
        BTreeMap::from([(
            reference.clone(),
            KeychainEntry {
                service: service.clone(),
                entry: entry.clone(),
            },
        )]),
    )
    .map(|resolver| Arc::new(resolver) as Arc<dyn SecretResolver>)
    .map_err(|_| about("provider", provider, "its `keychain:` entry is not usable"))
}

#[cfg(not(feature = "keychain"))]
fn keychain(
    provider: &Id,
    _reference: &SecretRef,
    _service: &Id,
    _entry: &Id,
    _sources: &SecretSources,
) -> Result<Arc<dyn SecretResolver>, Error> {
    Err(compiled_out(provider, "keychain:", "keychain"))
}

/// Where the caller's secret sources live. Nothing is read implicitly: a `file:~/` reference
/// expands only against the home given here, and a `keychain:` reference reads only the store
/// injected here.
#[derive(Clone, Default)]
pub struct SecretSources {
    home: Option<PathBuf>,
    #[cfg(feature = "keychain")]
    keychain: Option<Arc<keyring_core::CredentialStore>>,
}

impl SecretSources {
    /// No home and no keychain store.
    pub fn new() -> Self {
        Self::default()
    }

    /// The directory a `file:~/` reference expands against.
    #[must_use]
    pub fn with_home(mut self, home: impl Into<PathBuf>) -> Self {
        self.home = Some(home.into());
        self
    }

    /// The credential store `keychain:` references read from.
    #[cfg(feature = "keychain")]
    #[must_use]
    pub fn with_keychain(mut self, store: Arc<keyring_core::CredentialStore>) -> Self {
        self.keychain = Some(store);
        self
    }
}

impl fmt::Debug for SecretSources {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("SecretSources");
        debug.field("home", &self.home.is_some());
        #[cfg(feature = "keychain")]
        debug.field("keychain", &self.keychain.is_some());
        debug.finish()
    }
}

/// The resolvers a routes document's references build, one per credentialed provider, keyed by
/// the provider's name (its account id).
pub struct RouteResolvers {
    resolvers: BTreeMap<Id, Arc<dyn SecretResolver>>,
}

impl RouteResolvers {
    /// The providers a resolver was built for, in name order.
    pub fn accounts(&self) -> impl Iterator<Item = &Id> {
        self.resolvers.keys()
    }
}

impl fmt::Debug for RouteResolvers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouteResolvers")
            .field("accounts", &self.resolvers.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl Resolvers for RouteResolvers {
    fn resolver(&self, account_id: &Id) -> Option<Arc<dyn SecretResolver>> {
        self.resolvers.get(account_id).cloned()
    }
}
