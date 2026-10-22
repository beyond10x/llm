//! `llm.routes/1`: the short YAML document every consumer reads to learn which model a name
//! reaches, and how.
//!
//! An endpoint names its base URL, its wire protocol and how it authenticates; its secret is only
//! a source reference (`env:<VARIABLE>`, `file:<path>` or `keychain:<service>/<entry>`). A route
//! names one `<endpoint>/<model-id>`, split on the first `/` only, or a mapping with that `model`
//! and its `capabilities`. A name that is not a route but reads `<declared endpoint>/<model-id>`
//! resolves directly; such a direct name is itself one identifier of at most 256 bytes, so a
//! model id longer than the room it leaves is reached through a route.
//!
//! ```yaml
//! format: llm.routes/1
//! endpoints:
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
//! The document expands into an `llm.catalog/1` catalog: per endpoint one catalog provider, account
//! and endpoint, each with the endpoint's name as id, and the account's secret reference id is the
//! endpoint's name too; per route one served model, one serving model and one single-target
//! route without fallback, each with the route's name as id. Ports are then built by
//! [`crate::port`] and [`crate::CatalogModels::build`] on their unchanged rules.
//!
//! A route's `capabilities` may leave out `output_streaming` (true) and `input_during_output`
//! (false); a string route, and a mapping without `capabilities`, declares
//! [`default_capabilities`] (without reasoning efforts on `bedrock-converse`). A mapping may also
//! name `aliases`, further names that reach the route, and a `price` per million tokens.
//!
//! An endpoint with a `bedrock` table is an Amazon Bedrock Runtime endpoint:
//!
//! ```yaml
//! endpoints:
//!   bedrock-eu: {protocol: bedrock-converse, bedrock: {region: eu-central-1},
//!                auth: {kind: aws-container}}
//!   bedrock-keys: {protocol: messages, bedrock: {region: eu-central-1},
//!                  auth: {kind: aws-keys, access_key_id: env:AWS_ACCESS_KEY_ID,
//!                         secret_access_key: env:AWS_SECRET_ACCESS_KEY}}
//! ```
//!
//! Its `protocol` is `bedrock-converse` (Converse) or `messages` (the Anthropic body on
//! `InvokeModel`), its `base_url`, optional, replaces the Region's origin, and its route's model id
//! is the Bedrock model or inference-profile id. It authenticates with `aws-keys` (an access key
//! id, a secret access key and optionally a session token, each a secret reference) or
//! `aws-container` (the container credentials endpoint and its token file, which the caller
//! supplies through [`SecretSources::with_container_credentials`]). Its routes are served by
//! `llm-bedrock`'s `BedrockClient` and stay out of the catalog, whose bindings are the three
//! HTTP wires.
//!
//! [`Routes::parse_toml`] reads the same document written in TOML, with the same keys and the
//! same refusals.
//!
//! Parsing is strict and performs no I/O: an unknown field, a repeated key, an unknown format,
//! protocol, authentication kind or secret source, a literal secret, a capability the endpoint's
//! protocol cannot carry and a route to an undeclared endpoint are refused as `invalid-request`,
//! naming the field, endpoint or route and never echoing a value. [`Routes::resolvers`] turns the
//! references into the caller's resolvers without reading anything; an unset variable or a
//! missing file is refused at turn time, as `unauthorized`, by the endpoint's reference, before
//! any request is sent.

use crate::{AwsReferences, Port, Resolvers, port};
use llm_bedrock::{Api, BedrockTarget};
use llm_core::{
    AuthKind, BillingKind, BoxFuture, Capabilities, Dispatch, Error, ErrorCode, Id, Protocol,
    Provenance, TurnObservation,
};
use llm_cost::{
    AccountingInput, Amount, AttemptUsage, Currency, ModelPrice, Observation, PriceBook,
    PriceDocument, Rate, Total,
};
use llm_credentials::{ResolvedSecret, SecretError, SecretRef, SecretResolver};
use llm_http::HttpClient;
use llm_providers::{
    Account, ApiKeyHeader, BaseUrl, Binding, Endpoint, Provider, ServedModel, ServingModel,
};
use llm_routing::{Catalog, CatalogDocument, MAX_CONFIG_BYTES, Route, RouteTarget};
use serde::{
    Deserialize, Deserializer,
    de::{self, DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    collections::BTreeMap,
    fmt,
    marker::PhantomData,
    path::{Component, Path, PathBuf},
    sync::Arc,
};
use url::Url;

/// The format a routes document declares in its `format` field.
pub const FORMAT: &str = "llm.routes/1";

const DEFAULT_CONTEXT_WINDOW: u64 = 200_000;
const DEFAULT_MAX_OUTPUT_TOKENS: u64 = 16_384;
const MAX_PATH_BYTES: usize = 4096;
const EXPANDED: &str =
    "llm.routes/1 document expands beyond its byte bound when its YAML aliases are resolved";
const CATALOG_BOUND: &str = "llm.routes/1 document expands beyond the catalog's bound";
const REPEATED: &str = "the document declares it more than once";
const SECRET_FORMS: &str = "`auth.secret` must be a reference: env:<VARIABLE>, \
file:<absolute or ~/ path> or keychain:<service>/<entry>";
const REFERENCE_FORMS: &str = "must be a reference: env:<VARIABLE>, \
file:<absolute or ~/ path> or keychain:<service>/<entry>";
const PER_MILLION: u64 = 1_000_000;
const AWS_KEY_FIELDS: [&str; 3] = ["access_key_id", "secret_access_key", "session_token"];

/// The capabilities a route written as a plain `<endpoint>/<model-id>` string declares: tools,
/// tool choice, temperature and `top_p` allowed, reasoning efforts `low`, `medium` and `high`, a
/// context window of 200000 tokens, at most 16384 output tokens, streamed output and no input
/// during output. A route whose model differs writes its own `capabilities`.
pub fn default_capabilities() -> Capabilities {
    Capabilities {
        tools: true,
        tool_choice: true,
        temperature: true,
        top_p: true,
        reasoning_efforts: ["low", "medium", "high"].map(str::to_owned).into(),
        context_window: DEFAULT_CONTEXT_WINDOW,
        max_output_tokens: DEFAULT_MAX_OUTPUT_TOKENS,
        output_streaming: true,
        input_during_output: false,
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDocument {
    format: String,
    endpoints: Ordered<RawEndpoint>,
    #[serde(default)]
    routes: Ordered<RawRoute>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEndpoint {
    #[serde(default)]
    base_url: Option<String>,
    protocol: String,
    auth: RawAuth,
    #[serde(default)]
    bedrock: Option<RawBedrock>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBedrock {
    #[serde(default)]
    region: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAuth {
    kind: String,
    #[serde(default)]
    header: Option<String>,
    #[serde(default)]
    secret: Option<String>,
    #[serde(default)]
    access_key_id: Option<String>,
    #[serde(default)]
    secret_access_key: Option<String>,
    #[serde(default)]
    session_token: Option<String>,
}

impl RawAuth {
    /// The AWS key fields the document names, in [`AWS_KEY_FIELDS`] order.
    fn key_fields(&self) -> [(&'static str, Option<&String>); 3] {
        [
            (AWS_KEY_FIELDS[0], self.access_key_id.as_ref()),
            (AWS_KEY_FIELDS[1], self.secret_access_key.as_ref()),
            (AWS_KEY_FIELDS[2], self.session_token.as_ref()),
        ]
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPrice {
    currency: String,
    #[serde(default)]
    input: Option<String>,
    #[serde(default)]
    cached_input: Option<String>,
    #[serde(default)]
    cache_creation_input: Option<String>,
    #[serde(default)]
    output: Option<String>,
}

/// A route is a `<endpoint>/<model-id>` string or a table. Not `#[serde(untagged)]`, which
/// replaces the table's own refusal (an unknown key, by name) with "did not match any variant".
enum RawRoute {
    Model(String),
    Declared(RawDeclaredRoute),
}

impl<'de> Deserialize<'de> for RawRoute {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Route;
        impl<'de> Visitor<'de> for Route {
            type Value = RawRoute;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a `<endpoint>/<model-id>` string or a route table")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<RawRoute, E> {
                Ok(RawRoute::Model(value.to_owned()))
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<RawRoute, A::Error> {
                RawDeclaredRoute::deserialize(de::value::MapAccessDeserializer::new(map))
                    .map(RawRoute::Declared)
            }
        }
        deserializer.deserialize_any(Route)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDeclaredRoute {
    model: String,
    #[serde(default)]
    capabilities: Option<Capabilities>,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    price: Option<RawPrice>,
}

/// A mapping in document order. A repeated key is kept rather than merged, so [`Routes::parse`]
/// refuses it by name where a plain map would let the last occurrence silently win.
struct Ordered<V>(Vec<(String, V)>);

impl<V> Default for Ordered<V> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<'de, V: Deserialize<'de>> Deserialize<'de> for Ordered<V> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Entries<V>(PhantomData<V>);
        impl<'de, V: Deserialize<'de>> Visitor<'de> for Entries<V> {
            type Value = Ordered<V>;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a mapping")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut entries = Vec::new();
                while let Some(key) = map.next_key::<String>()? {
                    entries.push((key, map.next_value()?));
                }
                Ok(Ordered(entries))
            }
        }
        deserializer.deserialize_map(Entries(PhantomData))
    }
}

/// The YAML parser's own diagnostic can quote the document, so only its location is kept. The
/// diagnostic is never searched: a refusal's kind is decided where it arose.
fn malformed(error: &serde_yaml::Error) -> Error {
    let location = error.location().map_or(String::new(), |location| {
        format!(" at line {} column {}", location.line(), location.column())
    });
    Error::invalid(format!("invalid {FORMAT} YAML document{location}"))
}

/// The TOML parser's diagnostic can quote the document too: only the line and column its span
/// starts at are kept, and for an unknown key serde's own sentence, which names keys only.
fn malformed_toml(source: &str, error: &toml::de::Error) -> Error {
    let location = error.span().map_or(String::new(), |span| {
        let before = &source[..span.start.min(source.len())];
        let line = before.matches('\n').count() + 1;
        let column = before.len() - before.rfind('\n').map_or(0, |end| end + 1) + 1;
        format!(" at line {line} column {column}")
    });
    let message = error.message();
    let unknown = if message.starts_with("unknown field `") && !message.contains('\n') {
        format!(": {message}")
    } else {
        String::new()
    };
    Error::invalid(format!("invalid {FORMAT} TOML document{location}{unknown}"))
}

/// What one node is charged beyond its text. Before a value's type is known it is buffered as
/// serde's `Content` (32 bytes, in a vector that may hold twice its length), so 64 bytes is at
/// least what a node costs once deserialized.
const NODE_BYTES: usize = 64;

/// What a document may cost per byte of its source once every YAML alias is replayed: the most
/// an alias-free document can cost under [`NODE_BYTES`]. Measured, the densest alias-free YAML
/// holds one node per source byte (keys with implicit null values, `{a,a}`; explicit empty keys,
/// `?` lines) and decodes into at most one and a half bytes of text per source byte (`\L`
/// escapes), so 64 + 2 bytes per source byte bounds every document without aliases. A smaller
/// multiple would refuse ordinary documents: a plain route line costs about 4.5 times its length.
const EXPANDED_PER_SOURCE_BYTE: usize = NODE_BYTES + 2;

/// The budget of the shortest documents, so a few aliases in a small document stay ordinary.
const MIN_EXPANDED_BYTES: usize = 64 * 1024;

/// How much a document may cost while it is read, every alias replayed: what the densest
/// alias-free document of its length costs, and at least [`MIN_EXPANDED_BYTES`].
fn expansion_budget(source: &str) -> usize {
    source
        .len()
        .saturating_mul(EXPANDED_PER_SOURCE_BYTE)
        .max(MIN_EXPANDED_BYTES)
}

/// What a [`Measure`] walk may still spend, and whether it ran out.
struct Budget {
    left: Cell<usize>,
    exhausted: Cell<bool>,
}

/// Walks the document as the YAML parser presents it, every alias replayed, and keeps nothing.
///
/// The parser replays an alias from the events it already holds, without copying them, so this
/// walk holds no more than the parser does. Deserializing into owned values would instead copy
/// the anchored nodes once per alias: a short document could cost far more than its byte bound
/// before any of it was refused. Each node costs its text plus [`NODE_BYTES`], and the walk stops
/// when the [`expansion_budget`] is spent, marking the budget exhausted so the refusal is
/// classified by where it arose rather than by the parser's message.
#[derive(Clone, Copy)]
struct Measure<'a>(&'a Budget);

impl Measure<'_> {
    fn charge<E: de::Error>(self, text: usize) -> Result<(), E> {
        let Some(left) = self
            .0
            .left
            .get()
            .checked_sub(text.saturating_add(NODE_BYTES))
        else {
            self.0.exhausted.set(true);
            return Err(E::custom("expansion budget spent"));
        };
        self.0.left.set(left);
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

/// An endpoint or route name: a valid identifier holding no `/`.
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

/// An endpoint's secret reference. Its `Debug` names only the source kind: the variable name,
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

    /// The reference as the document writes it, for [`Routes::digest`] only.
    fn canonical(&self) -> String {
        match self {
            Self::Environment(variable) => format!("env:{variable}"),
            Self::File(FilePath::Absolute(path)) => format!("file:{}", path.display()),
            Self::File(FilePath::Home(path)) => format!("file:~/{}", path.display()),
            Self::Keychain { service, entry } => format!("keychain:{service}/{entry}"),
        }
    }
}

impl fmt::Debug for SecretSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretSource({}:[EXPLICIT])", self.kind().as_str())
    }
}

/// The source kind of an endpoint's secret reference.
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

/// One declared endpoint, as the document states it.
#[derive(Debug, Clone)]
pub struct EndpointDeclaration {
    name: Id,
    base_url: BaseUrl,
    protocol: Protocol,
    auth_kind: AuthKind,
    api_key_header: Option<ApiKeyHeader>,
    secret: Option<SecretSource>,
}

/// One endpoint as read: an HTTP endpoint the catalog expands, or a Bedrock Runtime endpoint.
enum Declared {
    Http(EndpointDeclaration),
    Bedrock(BedrockEndpointDeclaration),
}

fn read_endpoint(name: Id, raw: RawEndpoint) -> Result<Declared, Error> {
    let refuse = |message: &str| about("endpoint", &name, message);
    let protocol = match raw.protocol.as_str() {
        "messages" => Protocol::Messages,
        "responses" => Protocol::Responses,
        "chat-completions" => Protocol::ChatCompletions,
        "bedrock-converse" => Protocol::BedrockConverse,
        _ => {
            return Err(refuse(
                "unknown `protocol`; expected messages, responses, chat-completions or \
                 bedrock-converse",
            ));
        }
    };
    if raw.bedrock.is_some() || protocol == Protocol::BedrockConverse {
        return BedrockEndpointDeclaration::read(name, protocol, raw).map(Declared::Bedrock);
    }
    if matches!(raw.auth.kind.as_str(), "aws-keys" | "aws-container") {
        return Err(refuse(
            "only a Bedrock endpoint (one with a `bedrock` table) takes `auth.kind` aws-keys or \
             aws-container",
        ));
    }
    if let Some((field, _)) = raw
        .auth
        .key_fields()
        .into_iter()
        .find(|(_, value)| value.is_some())
    {
        return Err(refuse(&format!(
            "only an aws-keys endpoint takes `auth.{field}`"
        )));
    }
    EndpointDeclaration::read(name, protocol, raw).map(Declared::Http)
}

impl EndpointDeclaration {
    fn read(name: Id, protocol: Protocol, raw: RawEndpoint) -> Result<Self, Error> {
        let refuse = |message: &str| about("endpoint", &name, message);
        let base_url = raw
            .base_url
            .as_deref()
            .ok_or_else(|| refuse("requires `base_url`"))?;
        let base_url = BaseUrl::new(base_url).map_err(|error| refuse(&error.message))?;
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
                return Err(refuse("an api-key endpoint requires `auth.header`"));
            }
            (_, Some(_)) => {
                return Err(refuse("only an api-key endpoint takes `auth.header`"));
            }
            (_, None) => None,
        };
        let secret = match (auth_kind, raw.auth.secret) {
            (AuthKind::Anonymous, Some(_)) => {
                return Err(refuse("an anonymous endpoint cannot name `auth.secret`"));
            }
            (AuthKind::Anonymous, None) => None,
            (_, None) => {
                return Err(refuse("a credentialed endpoint requires `auth.secret`"));
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

    /// The endpoint's name; also its catalog provider, account, endpoint and secret reference id.
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

    /// The header an api-key endpoint's credential travels in.
    pub fn api_key_header(&self) -> Option<&ApiKeyHeader> {
        self.api_key_header.as_ref()
    }

    /// The source kind of the secret reference; `None` for an anonymous endpoint.
    pub fn secret_kind(&self) -> Option<SecretKind> {
        self.secret.as_ref().map(SecretSource::kind)
    }

    fn reference(&self) -> Result<SecretRef, Error> {
        SecretRef::new(self.name.as_str())
            .map_err(|_| about("endpoint", &self.name, "its name is not a secret reference"))
    }

    fn canonical(&self) -> Value {
        json!({
            "base_url": self.base_url.as_str(),
            "protocol": self.protocol,
            "auth": {
                "kind": self.auth_kind,
                "header": self.api_key_header.as_ref().map(ApiKeyHeader::as_str),
                "secret": self.secret.as_ref().map(SecretSource::canonical),
            },
        })
    }

    /// The catalog provider, account and endpoint this endpoint expands into.
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

/// How a Bedrock endpoint gets the AWS credentials it signs with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AwsCredentialKind {
    /// `aws-keys`: an access key id, a secret access key and optionally a session token, each
    /// a secret reference.
    Keys,
    /// `aws-container`: the container credentials endpoint (EKS Pod Identity) and its token
    /// file, which the caller supplies.
    Container,
}

impl AwsCredentialKind {
    /// The `auth.kind` the document writes: `aws-keys` or `aws-container`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keys => "aws-keys",
            Self::Container => "aws-container",
        }
    }
}

#[derive(Debug, Clone)]
enum AwsCredentials {
    Keys {
        access_key_id: SecretSource,
        secret_access_key: SecretSource,
        session_token: Option<SecretSource>,
    },
    Container,
}

/// One declared Amazon Bedrock Runtime endpoint, as the document states it.
#[derive(Debug, Clone)]
pub struct BedrockEndpointDeclaration {
    name: Id,
    region: String,
    protocol: Protocol,
    endpoint: Option<Url>,
    credentials: AwsCredentials,
}

/// The secret reference one AWS key field of a Bedrock endpoint resolves under: the endpoint's
/// name, `/`, the field.
fn key_reference(endpoint: &Id, field: &str) -> Result<SecretRef, Error> {
    SecretRef::new(format!("{endpoint}/{field}")).map_err(|_| {
        about(
            "endpoint",
            endpoint,
            "its name is too long to name its aws-keys references",
        )
    })
}

impl BedrockEndpointDeclaration {
    fn read(name: Id, protocol: Protocol, raw: RawEndpoint) -> Result<Self, Error> {
        let refuse = |message: &str| about("endpoint", &name, message);
        let api = match protocol {
            Protocol::BedrockConverse => Api::Converse,
            Protocol::Messages => Api::AnthropicInvoke,
            Protocol::Responses | Protocol::ChatCompletions => {
                return Err(refuse(
                    "a Bedrock endpoint's `protocol` must be bedrock-converse or messages",
                ));
            }
        };
        let region = raw
            .bedrock
            .and_then(|bedrock| bedrock.region)
            .ok_or_else(|| refuse("a Bedrock endpoint requires `bedrock.region`"))?;
        let endpoint = raw
            .base_url
            .map(|base_url| Url::parse(&base_url).map_err(|_| refuse("`base_url` is not a URL")))
            .transpose()?;
        let auth = raw.auth;
        if auth.header.is_some() {
            return Err(refuse("a Bedrock endpoint takes no `auth.header`"));
        }
        if auth.secret.is_some() {
            return Err(refuse(
                "a Bedrock endpoint takes no `auth.secret`; aws-keys names `auth.access_key_id` \
                 and `auth.secret_access_key`",
            ));
        }
        let credentials = match auth.kind.as_str() {
            "aws-keys" => {
                let [access_key_id, secret_access_key, session_token] =
                    auth.key_fields().map(|(field, value)| {
                        value
                            .map(|value| {
                                SecretSource::read(value).ok_or_else(|| {
                                    refuse(&format!("`auth.{field}` {REFERENCE_FORMS}"))
                                })
                            })
                            .transpose()
                    });
                let required = |field: &str, value: Option<SecretSource>| {
                    value.ok_or_else(|| {
                        refuse(&format!("an aws-keys endpoint requires `auth.{field}`"))
                    })
                };
                for field in AWS_KEY_FIELDS {
                    key_reference(&name, field)?;
                }
                AwsCredentials::Keys {
                    access_key_id: required(AWS_KEY_FIELDS[0], access_key_id?)?,
                    secret_access_key: required(AWS_KEY_FIELDS[1], secret_access_key?)?,
                    session_token: session_token?,
                }
            }
            "aws-container" => {
                if let Some((field, _)) = auth
                    .key_fields()
                    .into_iter()
                    .find(|(_, value)| value.is_some())
                {
                    return Err(refuse(&format!(
                        "an aws-container endpoint cannot also name `auth.{field}`: a Bedrock \
                         endpoint has one credential source"
                    )));
                }
                AwsCredentials::Container
            }
            _ => {
                return Err(refuse(
                    "a Bedrock endpoint's `auth.kind` must be aws-keys or aws-container",
                ));
            }
        };
        let declaration = Self {
            name,
            region,
            protocol,
            endpoint,
            credentials,
        };
        // The Region and the endpoint override are checked by the client's own rules, on a
        // probe target that names no route.
        let probe = Id::new("probe").map_err(|_| Error::invalid("invalid probe id"))?;
        declaration
            .target(&probe, "probe", bedrock_capabilities(api))
            .map_err(|error| about("endpoint", &declaration.name, &error.message))?;
        Ok(declaration)
    }

    /// The endpoint's name; also the provenance provider, account and endpoint of its turns.
    pub fn name(&self) -> &Id {
        &self.name
    }

    /// The AWS Region the requests are sent to and signed for.
    pub fn region(&self) -> &str {
        &self.region
    }

    /// `bedrock-converse`, or `messages` for the Anthropic body on `InvokeModel`.
    pub fn protocol(&self) -> Protocol {
        self.protocol
    }

    /// The origin that replaces `https://bedrock-runtime.{region}.amazonaws.com`, when declared.
    pub fn endpoint_override(&self) -> Option<&Url> {
        self.endpoint.as_ref()
    }

    /// The host the requests go to.
    pub fn host(&self) -> String {
        self.endpoint.as_ref().and_then(Url::host_str).map_or_else(
            || format!("bedrock-runtime.{}.amazonaws.com", self.region),
            str::to_owned,
        )
    }

    pub fn credential_kind(&self) -> AwsCredentialKind {
        match self.credentials {
            AwsCredentials::Keys { .. } => AwsCredentialKind::Keys,
            AwsCredentials::Container => AwsCredentialKind::Container,
        }
    }

    /// The source kinds of the aws-keys references, in field order; empty for aws-container.
    pub fn secret_kinds(&self) -> Vec<SecretKind> {
        match &self.credentials {
            AwsCredentials::Keys {
                access_key_id,
                secret_access_key,
                session_token,
            } => [
                Some(access_key_id),
                Some(secret_access_key),
                session_token.as_ref(),
            ]
            .into_iter()
            .flatten()
            .map(SecretSource::kind)
            .collect(),
            AwsCredentials::Container => Vec::new(),
        }
    }

    fn api(&self) -> Api {
        if self.protocol == Protocol::BedrockConverse {
            Api::Converse
        } else {
            Api::AnthropicInvoke
        }
    }

    /// The validated client target for one serving model of this endpoint.
    fn target(
        &self,
        serving_model: &Id,
        model_id: &str,
        capabilities: Capabilities,
    ) -> Result<BedrockTarget, Error> {
        capabilities.validate()?;
        // Every route streams its output and takes no input during it, as on the HTTP wires.
        capabilities.validate_protocol(Protocol::Messages)?;
        let provenance = Provenance {
            protocol: self.protocol,
            provider: self.name.clone(),
            account: self.name.clone(),
            endpoint: self.name.clone(),
            model: serving_model.clone(),
            binding_revision: self.revision(serving_model, model_id, &capabilities)?,
        };
        BedrockTarget::new(
            provenance,
            capabilities,
            self.region.clone(),
            model_id,
            self.api(),
            true,
            self.endpoint.clone(),
        )
    }

    /// What identifies the concrete definition a turn was bound to: a SHA-256 over the Region,
    /// wire, origin, credential kind, model id and capabilities.
    fn revision(
        &self,
        serving_model: &Id,
        model_id: &str,
        capabilities: &Capabilities,
    ) -> Result<Id, Error> {
        let definition = json!({
            "serving_model": serving_model,
            "region": self.region,
            "protocol": self.protocol,
            "endpoint": self.endpoint.as_ref().map(Url::as_str),
            "credentials": self.credential_kind().as_str(),
            "model_id": model_id,
            "capabilities": capabilities,
        });
        Id::new(sha256_hex(definition.to_string().as_bytes()))
            .map_err(|_| Error::invalid("invalid binding revision"))
    }

    fn references(&self) -> Result<AwsReferences, Error> {
        Ok(match &self.credentials {
            AwsCredentials::Keys { session_token, .. } => AwsReferences::Keys {
                account: self.name.clone(),
                access_key_id: key_reference(&self.name, AWS_KEY_FIELDS[0])?,
                secret_access_key: key_reference(&self.name, AWS_KEY_FIELDS[1])?,
                session_token: session_token
                    .as_ref()
                    .map(|_| key_reference(&self.name, AWS_KEY_FIELDS[2]))
                    .transpose()?,
            },
            AwsCredentials::Container => AwsReferences::Container {
                account: self.name.clone(),
                token: SecretRef::new(self.name.as_str()).map_err(|_| {
                    about("endpoint", &self.name, "its name is not a secret reference")
                })?,
            },
        })
    }

    fn canonical(&self) -> Value {
        let credentials = match &self.credentials {
            AwsCredentials::Keys {
                access_key_id,
                secret_access_key,
                session_token,
            } => json!({
                "kind": "aws-keys",
                "access_key_id": access_key_id.canonical(),
                "secret_access_key": secret_access_key.canonical(),
                "session_token": session_token.as_ref().map(SecretSource::canonical),
            }),
            AwsCredentials::Container => json!({"kind": "aws-container"}),
        };
        json!({
            "name": self.name,
            "region": self.region,
            "protocol": self.protocol,
            "base_url": self.endpoint.as_ref().map(Url::as_str),
            "auth": credentials,
        })
    }
}

/// The capabilities a Bedrock route declares when it writes none: [`default_capabilities`],
/// without reasoning efforts on Converse, whose wire carries none.
fn bedrock_capabilities(api: Api) -> Capabilities {
    let mut capabilities = default_capabilities();
    if api == Api::Converse {
        capabilities.reasoning_efforts.clear();
    }
    capabilities
}

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        hex.push(char::from(HEX[usize::from(byte >> 4)]));
        hex.push(char::from(HEX[usize::from(byte & 15)]));
    }
    hex
}

/// A route's price per million tokens, in one currency. A class it leaves out is unknown, never
/// free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutePrice {
    currency: Currency,
    input: Option<Amount>,
    cached_input: Option<Amount>,
    cache_creation_input: Option<Amount>,
    output: Option<Amount>,
}

impl RoutePrice {
    fn read(route: &Id, raw: RawPrice) -> Result<Self, Error> {
        let currency = Currency::new(raw.currency).map_err(|_| {
            about(
                "route",
                route,
                "`price.currency` must be three upper-case letters",
            )
        })?;
        let amount = |field: &str, value: Option<String>| {
            value
                .map(|value| {
                    Amount::parse(&value).map_err(|_| {
                        about(
                            "route",
                            route,
                            &format!(
                                "`price.{field}` must be a decimal amount per million tokens, \
                                 written as a string"
                            ),
                        )
                    })
                })
                .transpose()
        };
        Ok(Self {
            currency,
            input: amount("input", raw.input)?,
            cached_input: amount("cached_input", raw.cached_input)?,
            cache_creation_input: amount("cache_creation_input", raw.cache_creation_input)?,
            output: amount("output", raw.output)?,
        })
    }

    pub fn currency(&self) -> &Currency {
        &self.currency
    }

    /// Per million uncached input tokens.
    pub fn input(&self) -> Option<Amount> {
        self.input
    }

    /// Per million input tokens read from the cache.
    pub fn cached_input(&self) -> Option<Amount> {
        self.cached_input
    }

    /// Per million input tokens written to the cache.
    pub fn cache_creation_input(&self) -> Option<Amount> {
        self.cache_creation_input
    }

    /// Per million output tokens.
    pub fn output(&self) -> Option<Amount> {
        self.output
    }

    fn canonical(&self) -> Value {
        json!({
            "currency": self.currency.as_str(),
            "input": self.input.map(|amount| amount.to_string()),
            "cached_input": self.cached_input.map(|amount| amount.to_string()),
            "cache_creation_input": self.cache_creation_input.map(|amount| amount.to_string()),
            "output": self.output.map(|amount| amount.to_string()),
        })
    }
}

/// What one turn on a priced route cost, by the `llm-cost` rules: a line whose count or rate is
/// unknown stays unknown, and the complete total exists only when no line is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteCost {
    pub currency: Currency,
    pub total: Total,
}

/// One declared route, as the document states it.
#[derive(Debug, Clone)]
pub struct RouteDeclaration {
    name: Id,
    endpoint: Id,
    upstream_model: Id,
    capabilities: Capabilities,
    protocol: Protocol,
    aliases: Vec<Id>,
    price: Option<RoutePrice>,
}

impl RouteDeclaration {
    fn read<'a>(
        route: Id,
        declaration: RawRoute,
        reach: impl Fn(&Id) -> Option<Reached<'a>>,
    ) -> Result<Self, Error> {
        let refuse = |message: &str| about("route", &route, message);
        let (model, capabilities, named, price) = match declaration {
            RawRoute::Model(model) => (model, None, Vec::new(), None),
            RawRoute::Declared(declared) => (
                declared.model,
                declared.capabilities,
                declared.aliases,
                declared.price,
            ),
        };
        let (endpoint, upstream_model) = model
            .split_once('/')
            .ok_or_else(|| refuse("its model must read <endpoint>/<model-id>"))?;
        let endpoint = Id::new(endpoint)
            .ok()
            .and_then(|endpoint| reach(&endpoint))
            .ok_or_else(|| refuse("its model names an undeclared endpoint"))?;
        let upstream_model = Id::new(upstream_model)
            .map_err(|_| refuse("its model id must be 1..=256 printable ASCII bytes"))?;
        let capabilities = capabilities.unwrap_or_else(|| endpoint.default_capabilities());
        match endpoint {
            Reached::Http(declaration) => capabilities
                .validate()
                .and_then(|()| capabilities.validate_protocol(declaration.protocol))
                .map(drop),
            Reached::Bedrock(declaration) => declaration
                .target(&route, upstream_model.as_str(), capabilities.clone())
                .map(drop),
        }
        .map_err(|error| refuse(&error.message))?;
        let aliases = named
            .iter()
            .map(|alias| name(alias, "alias"))
            .collect::<Result<Vec<_>, _>>()?;
        let price = price
            .map(|price| RoutePrice::read(&route, price))
            .transpose()?;
        Ok(Self {
            name: route,
            endpoint: endpoint.name(),
            upstream_model,
            capabilities,
            protocol: endpoint.protocol(),
            aliases,
            price,
        })
    }

    /// The route's name; also its serving model, served model and catalog route id and alias.
    pub fn name(&self) -> &Id {
        &self.name
    }

    /// The declared endpoint the route's model names: everything before its first `/`.
    pub fn endpoint(&self) -> &Id {
        &self.endpoint
    }

    /// The model id sent upstream: everything after the first `/` of the route's model.
    pub fn upstream_model(&self) -> &Id {
        &self.upstream_model
    }

    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    /// The protocol of the route's endpoint.
    pub fn protocol(&self) -> Protocol {
        self.protocol
    }

    /// The further names that reach the route, in document order.
    pub fn aliases(&self) -> &[Id] {
        &self.aliases
    }

    /// The route's price per million tokens, when it declares one.
    pub fn price(&self) -> Option<&RoutePrice> {
        self.price.as_ref()
    }

    fn canonical(&self) -> Value {
        let mut aliases: Vec<&str> = self.aliases.iter().map(Id::as_str).collect();
        aliases.sort_unstable();
        json!({
            "endpoint": self.endpoint,
            "model": self.upstream_model,
            "capabilities": self.capabilities,
            "aliases": aliases,
            "price": self.price.as_ref().map(RoutePrice::canonical),
        })
    }
}

/// What a name reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The serving model id that answers the name: the route's name, or `<endpoint>/<model-id>`
    /// for a direct name.
    pub serving_model: Id,
    pub endpoint: Id,
    pub upstream_model: Id,
    pub protocol: Protocol,
}

/// A validated `llm.routes/1` document and the catalog it expands into.
#[derive(Debug)]
pub struct Routes {
    endpoints: BTreeMap<Id, EndpointDeclaration>,
    bedrock: BTreeMap<Id, BedrockEndpointDeclaration>,
    declared: BTreeMap<Id, RouteDeclaration>,
    aliases: BTreeMap<Id, Id>,
    catalog: Catalog,
    digest: String,
}

/// The endpoint a name reaches: one the catalog expands, or a Bedrock Runtime endpoint.
#[derive(Clone, Copy)]
enum Reached<'a> {
    Http(&'a EndpointDeclaration),
    Bedrock(&'a BedrockEndpointDeclaration),
}

impl Reached<'_> {
    fn name(self) -> Id {
        match self {
            Self::Http(endpoint) => endpoint.name.clone(),
            Self::Bedrock(endpoint) => endpoint.name.clone(),
        }
    }

    fn protocol(self) -> Protocol {
        match self {
            Self::Http(endpoint) => endpoint.protocol,
            Self::Bedrock(endpoint) => endpoint.protocol,
        }
    }

    fn default_capabilities(self) -> Capabilities {
        match self {
            Self::Http(_) => default_capabilities(),
            Self::Bedrock(endpoint) => bedrock_capabilities(endpoint.api()),
        }
    }
}

/// One serving model the expansion declares.
struct Served<'a> {
    id: &'a Id,
    endpoint: &'a EndpointDeclaration,
    upstream_model: &'a Id,
    capabilities: &'a Capabilities,
}

fn expand<'a>(
    endpoints: impl IntoIterator<Item = &'a EndpointDeclaration>,
    served: &[Served<'_>],
) -> Result<Catalog, Error> {
    let mut declared = (Vec::new(), Vec::new(), Vec::new());
    for endpoint in endpoints {
        let (provider, account, endpoint) = endpoint.declarations()?;
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
            endpoint_id: entry.endpoint.name.clone(),
            model_id: entry.id.clone(),
            protocol: entry.endpoint.protocol,
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
    /// `too-large` for a document above [`MAX_CONFIG_BYTES`], for one whose YAML aliases make
    /// it cost more while it is read than the densest document of its length without aliases
    /// (each node at 64 bytes plus its text, 66 bytes per source byte), and for one whose
    /// expansion exceeds the catalog's own bounds (`MAX_CONFIG_BYTES` of declarations, 4096 of
    /// each kind: a few thousand routes); otherwise `invalid-request` for malformed YAML, an
    /// unknown field (`providers` included), a repeated key, an unknown format, protocol,
    /// authentication kind or secret source, a literal secret, contradictory authentication
    /// fields, an invalid name, base URL, header, Region, price or capability declaration, a
    /// capability its endpoint's protocol cannot carry (input during output, or output that does
    /// not stream), a route to an undeclared endpoint and an alias that names more than one
    /// route. The message names the field, endpoint, route or alias and never a value.
    pub fn parse(source: &str) -> Result<Self, Error> {
        if source.len() > MAX_CONFIG_BYTES {
            return Err(Error::too_large("routes document exceeds its byte bound"));
        }
        let budget = Budget {
            left: Cell::new(expansion_budget(source)),
            exhausted: Cell::new(false),
        };
        if let Err(error) = Measure(&budget).deserialize(serde_yaml::Deserializer::from_str(source))
        {
            return Err(if budget.exhausted.get() {
                Error::too_large(EXPANDED)
            } else {
                malformed(&error)
            });
        }
        let raw: RawDocument = serde_yaml::from_str(source).map_err(|error| malformed(&error))?;
        Self::from_raw(raw)
    }

    /// Reads the same document written in TOML: the same keys, the same bounds and the same
    /// refusals as [`Routes::parse`]. A malformed document is refused by its line and column, and
    /// an unknown key also by its name.
    ///
    /// # Errors
    /// As [`Routes::parse`]; TOML has no aliases, so nothing is refused for expanding.
    pub fn parse_toml(source: &str) -> Result<Self, Error> {
        if source.len() > MAX_CONFIG_BYTES {
            return Err(Error::too_large("routes document exceeds its byte bound"));
        }
        let raw: RawDocument =
            toml::from_str(source).map_err(|error| malformed_toml(source, &error))?;
        Self::from_raw(raw)
    }

    fn from_raw(raw: RawDocument) -> Result<Self, Error> {
        if raw.format != FORMAT {
            return Err(Error::invalid(format!(
                "the document's `format` is not {FORMAT}"
            )));
        }
        let mut endpoints = BTreeMap::new();
        let mut bedrock = BTreeMap::new();
        for (endpoint, declaration) in raw.endpoints.0 {
            let endpoint = name(&endpoint, "endpoint")?;
            if endpoints.contains_key(&endpoint) || bedrock.contains_key(&endpoint) {
                return Err(about("endpoint", &endpoint, REPEATED));
            }
            match read_endpoint(endpoint.clone(), declaration)? {
                Declared::Http(declaration) => {
                    endpoints.insert(endpoint, declaration);
                }
                Declared::Bedrock(declaration) => {
                    bedrock.insert(endpoint, declaration);
                }
            }
        }
        let mut routes = BTreeMap::new();
        let mut aliases = BTreeMap::new();
        for (route, declaration) in raw.routes.0 {
            let route = name(&route, "route")?;
            if routes.contains_key(&route) {
                return Err(about("route", &route, REPEATED));
            }
            let reach = |endpoint: &Id| {
                endpoints
                    .get(endpoint)
                    .map(Reached::Http)
                    .or_else(|| bedrock.get(endpoint).map(Reached::Bedrock))
            };
            let declaration = RouteDeclaration::read(route.clone(), declaration, reach)?;
            for alias in &declaration.aliases {
                if aliases.insert(alias.clone(), route.clone()).is_some() {
                    return Err(about(
                        "alias",
                        alias,
                        "is repeated or names more than one route",
                    ));
                }
            }
            routes.insert(route, declaration);
        }
        if let Some(alias) = aliases.keys().find(|alias| routes.contains_key(*alias)) {
            return Err(about(
                "alias",
                alias,
                "is also a route's name, so it would name two routes",
            ));
        }
        let served: Vec<Served<'_>> = routes
            .values()
            .filter_map(|route| {
                endpoints.get(&route.endpoint).map(|endpoint| Served {
                    id: &route.name,
                    endpoint,
                    upstream_model: &route.upstream_model,
                    capabilities: &route.capabilities,
                })
            })
            .collect();
        let catalog = expand(endpoints.values(), &served)?;
        let digest = digest(&endpoints, &bedrock, &routes);
        Ok(Self {
            endpoints,
            bedrock,
            declared: routes,
            aliases,
            catalog,
            digest,
        })
    }

    /// The `llm.catalog/1` catalog the document expands into: its serving models are the routes
    /// to the HTTP endpoints. A Bedrock route has no catalog entry.
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// A SHA-256 over the validated document, in hex: the same for a YAML and a TOML document
    /// that declare the same endpoints, routes, aliases and prices, whatever their order,
    /// layout or comments.
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// Every declared HTTP endpoint, in name order.
    pub fn endpoints(&self) -> impl Iterator<Item = &EndpointDeclaration> {
        self.endpoints.values()
    }

    /// Every declared Bedrock Runtime endpoint, in name order.
    pub fn bedrock_endpoints(&self) -> impl Iterator<Item = &BedrockEndpointDeclaration> {
        self.bedrock.values()
    }

    /// Every declared route, Bedrock routes included, in name order.
    pub fn routes(&self) -> impl Iterator<Item = &RouteDeclaration> {
        self.declared.values()
    }

    /// The route a route name or alias reaches.
    pub fn route(&self, name: &str) -> Option<&RouteDeclaration> {
        let id = Id::new(name).ok()?;
        self.declared.get(&id).or_else(|| {
            self.aliases
                .get(&id)
                .and_then(|route| self.declared.get(route))
        })
    }

    fn reached(&self, endpoint: &Id) -> Option<Reached<'_>> {
        self.endpoints
            .get(endpoint)
            .map(Reached::Http)
            .or_else(|| self.bedrock.get(endpoint).map(Reached::Bedrock))
    }

    /// A `<declared endpoint>/<model-id>` name of at most 256 bytes: its serving model id,
    /// endpoint and model id.
    fn direct(&self, name: &str) -> Result<(Id, Reached<'_>, Id), Error> {
        let unknown =
            || Error::invalid("the name is neither a route nor <declared endpoint>/<model-id>");
        let (endpoint, model) = name.split_once('/').ok_or_else(unknown)?;
        let endpoint = Id::new(endpoint)
            .ok()
            .and_then(|endpoint| self.reached(&endpoint))
            .ok_or_else(unknown)?;
        let model = Id::new(model).map_err(|_| unknown())?;
        let serving = Id::new(name).map_err(|_| unknown())?;
        Ok((serving, endpoint, model))
    }

    /// The one-entry catalog a direct name to an HTTP endpoint expands into, by the same rules
    /// as a route.
    fn direct_catalog(
        endpoint: &EndpointDeclaration,
        serving: &Id,
        model: &Id,
    ) -> Result<Catalog, Error> {
        let capabilities = default_capabilities();
        expand(
            [endpoint],
            &[Served {
                id: serving,
                endpoint,
                upstream_model: model,
                capabilities: &capabilities,
            }],
        )
    }

    /// What a name reaches: a route or an alias first; else `<declared endpoint>/<model-id>`,
    /// split on the first `/`. A direct name is itself one identifier of at most 256 bytes, its
    /// serving model id; a longer model id is reached through a route.
    ///
    /// # Errors
    /// `invalid-request` for any other name, a direct name above 256 bytes included, without
    /// echoing it.
    pub fn target(&self, name: &str) -> Result<Target, Error> {
        if let Some(route) = self.route(name) {
            return Ok(Target {
                serving_model: route.name.clone(),
                endpoint: route.endpoint.clone(),
                upstream_model: route.upstream_model.clone(),
                protocol: route.protocol,
            });
        }
        let (serving_model, endpoint, upstream_model) = self.direct(name)?;
        Ok(Target {
            serving_model,
            endpoint: endpoint.name(),
            upstream_model,
            protocol: endpoint.protocol(),
        })
    }

    /// The validated binding a name reaches, as [`Routes::target`] resolves it. A direct name
    /// takes the default capabilities.
    ///
    /// # Errors
    /// As [`Routes::target`], and `unsupported` for a name that reaches a Bedrock endpoint,
    /// which has no catalog binding.
    pub fn binding(&self, name: &str) -> Result<Binding, Error> {
        let absent = || Error::invalid("no such serving model in the catalog");
        let no_binding = || {
            Error::unsupported(
                "a Bedrock route has no catalog binding: llm-bedrock's BedrockClient serves it",
            )
        };
        if let Some(route) = self.route(name) {
            if self.bedrock.contains_key(&route.endpoint) {
                return Err(no_binding());
            }
            return self
                .catalog
                .binding(&route.name)
                .cloned()
                .ok_or_else(absent);
        }
        match self.direct(name)? {
            (serving, Reached::Http(endpoint), model) => {
                Self::direct_catalog(endpoint, &serving, &model)?
                    .binding(&serving)
                    .cloned()
                    .ok_or_else(absent)
            }
            (_, Reached::Bedrock(_), _) => Err(no_binding()),
        }
    }

    /// Builds the port a name reaches: for an HTTP endpoint with [`crate::port`], a route over
    /// the expanded catalog and a direct name over the one-entry catalog it expands into, so the
    /// rules are identical; for a Bedrock endpoint, `llm-bedrock`'s `BedrockClient` over the
    /// route's target, signing with the endpoint's credentials.
    ///
    /// # Errors
    /// As [`Routes::target`], and as [`crate::port`] refuses a credentialed endpoint the caller
    /// has no resolver for (`unauthorized`); an `aws-container` endpoint without a container
    /// credentials endpoint from the caller is `unauthorized` too. Nothing is sent.
    pub fn port(
        &self,
        name: &str,
        http: HttpClient,
        resolvers: &dyn Resolvers,
    ) -> Result<Port, Error> {
        if let Some(route) = self.route(name) {
            if let Some(endpoint) = self.bedrock.get(&route.endpoint) {
                let target = endpoint.target(
                    &route.name,
                    route.upstream_model.as_str(),
                    route.capabilities.clone(),
                )?;
                return Port::bedrock(target, endpoint.references()?, http, resolvers);
            }
            return port(&self.catalog, &route.name, http, resolvers);
        }
        match self.direct(name)? {
            (serving, Reached::Http(endpoint), model) => {
                let catalog = Self::direct_catalog(endpoint, &serving, &model)?;
                port(&catalog, &serving, http, resolvers)
            }
            (serving, Reached::Bedrock(endpoint), model) => {
                let target = endpoint.target(
                    &serving,
                    model.as_str(),
                    bedrock_capabilities(endpoint.api()),
                )?;
                Port::bedrock(target, endpoint.references()?, http, resolvers)
            }
        }
    }

    /// What one turn on the route a name reaches cost, by the route's `price`: `None` when the
    /// name is no route or alias, or its route declares no price. The price applies to the
    /// model the route declares; a count the turn did not report, a rate the price leaves out
    /// and usage that is not final each leave a line unknown, so the complete total is absent.
    ///
    /// # Errors
    /// `invalid-request` for usage whose counters contradict each other.
    pub fn cost(
        &self,
        name: &str,
        observation: &TurnObservation,
    ) -> Result<Option<RouteCost>, Error> {
        let Some(route) = self.route(name) else {
            return Ok(None);
        };
        let Some(price) = &route.price else {
            return Ok(None);
        };
        let id = |value: &str| Id::new(value).map_err(|_| Error::invalid("invalid cost id"));
        let rate = |amount: Option<Amount>| {
            amount.map(|amount| Rate {
                amount,
                per_units: PER_MILLION,
            })
        };
        let refused = |_| Error::invalid("the route's price or the turn's usage is not valid");
        let book = PriceBook::from_document(PriceDocument::new(
            id("route-price")?,
            "llm.routes/1 route price",
            0,
            price.currency.clone(),
            vec![ModelPrice {
                id: route.name.clone(),
                serving_model_id: route.name.clone(),
                binding_revision: observation.binding.binding_revision.clone(),
                upstream_model: route.upstream_model.clone(),
                input: rate(price.input),
                cached_input: rate(price.cached_input),
                cache_creation_input: rate(price.cache_creation_input),
                output: rate(price.output),
            }],
        ))
        .map_err(refused)?;
        let attempt = AttemptUsage {
            id: id("turn")?,
            usage_record_id: id("turn")?,
            request_id: id("turn")?,
            route_target_id: route.name.clone(),
            serving_model_id: route.name.clone(),
            price_id: route.name.clone(),
            binding: observation.binding.clone(),
            upstream_model: Some(route.upstream_model.clone()),
            billing: BillingKind::Metered,
            dispatch: Dispatch::Accepted,
            failed: false,
            final_usage: observation.final_usage,
            usage: observation.usage.clone().unwrap_or_default(),
        };
        let report = book
            .quote(&AccountingInput::new(vec![Observation::Attempt(attempt)]))
            .map_err(refused)?;
        let total = report
            .records
            .into_iter()
            .next()
            .map(|record| record.total)
            .ok_or_else(|| Error::invalid("the cost report holds no record"))?;
        Ok(Some(RouteCost {
            currency: price.currency.clone(),
            total,
        }))
    }

    /// The caller's resolvers for every credentialed endpoint, keyed by the endpoint's name.
    /// Building them reads no variable, file or keychain entry.
    ///
    /// An endpoint whose source cannot be built gets no resolver, and [`Routes::port`] refuses
    /// only its routes, as `unauthorized` naming the endpoint and why: a source kind whose
    /// feature (`environment`, `file`, `keychain`) this build does not enable, a `file:~/`
    /// reference without an absolute home, a `keychain:` reference without an injected store,
    /// a path the file source refuses, and an `aws-container` endpoint whose container token
    /// file the caller did not supply or supplied as a path the file source refuses. Every
    /// other route still builds.
    ///
    /// # Errors
    /// `invalid-request` for an endpoint name too long to name its references.
    pub fn resolvers(&self, sources: &SecretSources) -> Result<RouteResolvers, Error> {
        let mut resolvers = BTreeMap::new();
        let mut unavailable = BTreeMap::new();
        let mut keep = |name: &Id, built: Result<Arc<dyn SecretResolver>, Error>| match built {
            Ok(resolver) => {
                resolvers.insert(name.clone(), resolver);
            }
            Err(error) => {
                unavailable.insert(name.clone(), error.message);
            }
        };
        for endpoint in self.endpoints.values() {
            let Some(secret) = &endpoint.secret else {
                continue;
            };
            let reference = endpoint.reference()?;
            keep(
                &endpoint.name,
                resolver(&endpoint.name, &reference, secret, sources),
            );
        }
        for endpoint in self.bedrock.values() {
            let built = match (&endpoint.credentials, endpoint.references()?) {
                (
                    AwsCredentials::Keys {
                        access_key_id,
                        secret_access_key,
                        session_token,
                    },
                    AwsReferences::Keys {
                        access_key_id: access_reference,
                        secret_access_key: secret_reference,
                        session_token: session_reference,
                        ..
                    },
                ) => [
                    (Some(access_reference), Some(access_key_id)),
                    (Some(secret_reference), Some(secret_access_key)),
                    (session_reference, session_token.as_ref()),
                ]
                .into_iter()
                .filter_map(|pair| match pair {
                    (Some(reference), Some(source)) => Some((reference, source)),
                    _ => None,
                })
                .map(|(reference, source)| {
                    resolver(&endpoint.name, &reference, source, sources)
                        .map(|built| (reference, built))
                })
                .collect::<Result<BTreeMap<_, _>, _>>()
                .map(|by_reference| Arc::new(ByReference(by_reference)) as Arc<dyn SecretResolver>),
                (AwsCredentials::Container, AwsReferences::Container { token, .. }) => {
                    match sources.container_token_file.as_ref() {
                        Some(token_file) => {
                            container_token(&endpoint.name, &token, token_file.clone())
                        }
                        None => Err(about(
                            "endpoint",
                            &endpoint.name,
                            "an `aws-container` endpoint needs the container authorization \
                             token file, and none was supplied",
                        )),
                    }
                }
                _ => {
                    return Err(about(
                        "endpoint",
                        &endpoint.name,
                        "its credentials and references disagree",
                    ));
                }
            };
            keep(&endpoint.name, built);
        }
        Ok(RouteResolvers {
            resolvers,
            unavailable,
            container_uri: sources.container_uri.clone(),
        })
    }
}

/// The resolver one secret reference builds, by its source kind.
fn resolver(
    endpoint: &Id,
    reference: &SecretRef,
    secret: &SecretSource,
    sources: &SecretSources,
) -> Result<Arc<dyn SecretResolver>, Error> {
    match secret {
        SecretSource::Environment(variable) => environment(endpoint, reference, variable),
        SecretSource::File(path) => file(endpoint, reference, path, sources.home.as_deref()),
        SecretSource::Keychain { service, entry } => {
            keychain(endpoint, reference, service, entry, sources)
        }
    }
}

/// The resolver of an `aws-container` endpoint's authorization token file.
fn container_token(
    endpoint: &Id,
    token: &SecretRef,
    path: PathBuf,
) -> Result<Arc<dyn SecretResolver>, Error> {
    file(endpoint, token, &FilePath::Absolute(path), None)
}

/// One resolver per reference: an aws-keys endpoint's key fields may each name another source.
struct ByReference(BTreeMap<SecretRef, Arc<dyn SecretResolver>>);

impl SecretResolver for ByReference {
    fn resolve<'a>(
        &'a self,
        reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        match self.0.get(reference) {
            Some(resolver) => resolver.resolve(reference),
            None => Box::pin(async { Err(SecretError::Missing) }),
        }
    }
}

/// The canonical JSON of the validated document, hashed for [`Routes::digest`].
fn digest(
    endpoints: &BTreeMap<Id, EndpointDeclaration>,
    bedrock: &BTreeMap<Id, BedrockEndpointDeclaration>,
    routes: &BTreeMap<Id, RouteDeclaration>,
) -> String {
    let endpoints: serde_json::Map<String, Value> = endpoints
        .values()
        .map(|endpoint| (endpoint.name.to_string(), endpoint.canonical()))
        .chain(
            bedrock
                .values()
                .map(|endpoint| (endpoint.name.to_string(), endpoint.canonical())),
        )
        .collect();
    let routes: serde_json::Map<String, Value> = routes
        .values()
        .map(|route| (route.name.to_string(), route.canonical()))
        .collect();
    let document = json!({"format": FORMAT, "endpoints": endpoints, "routes": routes});
    sha256_hex(document.to_string().as_bytes())
}

#[cfg(any(
    not(feature = "environment"),
    not(feature = "file"),
    not(feature = "keychain")
))]
fn compiled_out(endpoint: &Id, source: &str, feature: &str) -> Error {
    about(
        "endpoint",
        endpoint,
        &format!(
            "`{source}` secrets need feature `{feature}` of llm-models, which this build does not enable"
        ),
    )
}

#[cfg(feature = "environment")]
fn environment(
    endpoint: &Id,
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
            "endpoint",
            endpoint,
            "its `env:` variable name is not usable",
        )
    })
}

#[cfg(not(feature = "environment"))]
fn environment(
    endpoint: &Id,
    _reference: &SecretRef,
    _variable: &str,
) -> Result<Arc<dyn SecretResolver>, Error> {
    Err(compiled_out(endpoint, "env:", "environment"))
}

#[cfg(feature = "file")]
fn file(
    endpoint: &Id,
    reference: &SecretRef,
    path: &FilePath,
    home: Option<&Path>,
) -> Result<Arc<dyn SecretResolver>, Error> {
    let path = match path {
        FilePath::Absolute(path) => path.clone(),
        FilePath::Home(relative) => home
            .ok_or_else(|| {
                about(
                    "endpoint",
                    endpoint,
                    "a `file:~/` secret needs a home directory, and none was supplied",
                )
            })?
            .join(relative),
    };
    llm_credentials::file::FileResolver::new(BTreeMap::from([(reference.clone(), path)]))
        .map(|resolver| Arc::new(resolver) as Arc<dyn SecretResolver>)
        .map_err(|_| {
            about(
                "endpoint",
                endpoint,
                "its `file:` secret does not expand to an absolute path without `.` or `..`",
            )
        })
}

#[cfg(not(feature = "file"))]
fn file(
    endpoint: &Id,
    _reference: &SecretRef,
    _path: &FilePath,
    _home: Option<&Path>,
) -> Result<Arc<dyn SecretResolver>, Error> {
    Err(compiled_out(endpoint, "file:", "file"))
}

#[cfg(feature = "keychain")]
fn keychain(
    endpoint: &Id,
    reference: &SecretRef,
    service: &Id,
    entry: &Id,
    sources: &SecretSources,
) -> Result<Arc<dyn SecretResolver>, Error> {
    use llm_credentials::keychain::{KeychainEntry, KeychainResolver};
    let store = sources.keychain.clone().ok_or_else(|| {
        about(
            "endpoint",
            endpoint,
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
    .map_err(|_| about("endpoint", endpoint, "its `keychain:` entry is not usable"))
}

#[cfg(not(feature = "keychain"))]
fn keychain(
    endpoint: &Id,
    _reference: &SecretRef,
    _service: &Id,
    _entry: &Id,
    _sources: &SecretSources,
) -> Result<Arc<dyn SecretResolver>, Error> {
    Err(compiled_out(endpoint, "keychain:", "keychain"))
}

/// Where the caller's secret sources live. Nothing is read implicitly: a `file:~/` reference
/// expands only against the home given here, and a `keychain:` reference reads only the store
/// injected here.
#[derive(Clone, Default)]
pub struct SecretSources {
    home: Option<PathBuf>,
    container_uri: Option<String>,
    container_token_file: Option<PathBuf>,
    #[cfg(feature = "keychain")]
    keychain: Option<Arc<keyring_core::CredentialStore>>,
}

impl SecretSources {
    /// No home, no container credentials endpoint and no keychain store.
    pub fn new() -> Self {
        Self::default()
    }

    /// The container credentials endpoint and the absolute path of its authorization token file
    /// that an `aws-container` Bedrock endpoint uses: on EKS Pod Identity, the values of
    /// `AWS_CONTAINER_CREDENTIALS_FULL_URI` and `AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE`. Nothing
    /// is read here; the token file is read, and the endpoint asked, at turn time. Needs feature
    /// `file`.
    #[must_use]
    pub fn with_container_credentials(
        mut self,
        uri: impl Into<String>,
        token_file: impl Into<PathBuf>,
    ) -> Self {
        self.container_uri = Some(uri.into());
        self.container_token_file = Some(token_file.into());
        self
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
        debug.field("container_uri", &self.container_uri.is_some());
        debug.field("container_token_file", &self.container_token_file.is_some());
        #[cfg(feature = "keychain")]
        debug.field("keychain", &self.keychain.is_some());
        debug.finish()
    }
}

/// The resolvers a routes document's references build, one per credentialed endpoint, keyed by
/// the endpoint's name (its account id).
pub struct RouteResolvers {
    resolvers: BTreeMap<Id, Arc<dyn SecretResolver>>,
    /// Why each credentialed endpoint without a resolver has none.
    unavailable: BTreeMap<Id, String>,
    container_uri: Option<String>,
}

impl RouteResolvers {
    /// The accounts, one per credentialed endpoint, a resolver was built for, in name order.
    pub fn accounts(&self) -> impl Iterator<Item = &Id> {
        self.resolvers.keys()
    }
}

impl fmt::Debug for RouteResolvers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouteResolvers")
            .field("accounts", &self.resolvers.keys().collect::<Vec<_>>())
            .field("unavailable", &self.unavailable.keys().collect::<Vec<_>>())
            .field("container_uri", &self.container_uri.is_some())
            .finish()
    }
}

impl Resolvers for RouteResolvers {
    fn resolver(&self, account_id: &Id) -> Option<Arc<dyn SecretResolver>> {
        self.resolvers.get(account_id).cloned()
    }

    fn container_credentials_uri(&self) -> Option<&str> {
        self.container_uri.as_deref()
    }

    fn unavailable(&self, account_id: &Id) -> Option<&str> {
        self.unavailable.get(account_id).map(String::as_str)
    }
}
