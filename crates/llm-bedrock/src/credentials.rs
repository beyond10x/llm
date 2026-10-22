//! AWS credentials and the two sources a Bedrock client takes them from.
//!
//! [`StaticCredentials`] resolves an access key pair, and optionally a session token, through
//! the caller's [`SecretResolver`] on every call and caches nothing. [`ContainerCredentials`]
//! fetches temporary credentials from the container credentials endpoint that EKS Pod Identity
//! and ECS task roles serve, and keeps them until five minutes before they expire; a refresh in
//! those five minutes that fails leaves the kept credentials in use until they expire.
//!
//! Every failure is `unauthorized`, dispatch `not-sent` and final, except cancellation and an
//! endpoint that did not answer, answered too late or answered with a server failure: that one
//! keeps the transport's `transport` or `deadline` class and its `Retry-After`, and is
//! `not-sent` and retriable. No message carries a key, a token, the endpoint's answer or its
//! URI; a message may name a secret reference, which is the operator's non-secret lookup name.

use crate::time::parse_rfc3339_utc;
use http::{HeaderMap, HeaderValue};
use llm_core::{BoxFuture, Cancel, Dispatch, Error, ErrorCode};
use llm_credentials::{ReferenceError, ResolvedSecret, SecretError, SecretRef, SecretResolver};
use llm_http::HttpClient;
use serde::de::{self, Deserialize, Deserializer, IgnoredAny, MapAccess, Visitor};
use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::Mutex;
use url::{Host, Url};
use zeroize::Zeroizing;

/// Cached container credentials are fetched again once they expire within this window.
const REFRESH_WINDOW: Duration = Duration::from_mins(5);
/// The longest key part or token accepted: 16 KiB.
const MAX_PART_BYTES: usize = 16 * 1024;
/// The container credentials endpoint's link-local IPv4 addresses: ECS, then EKS Pod Identity.
const CONTAINER_IPV4: [Ipv4Addr; 2] = [
    Ipv4Addr::new(169, 254, 170, 2),
    Ipv4Addr::new(169, 254, 170, 23),
];
/// EKS Pod Identity's IPv6 address, `fd00:ec2::23`.
const CONTAINER_IPV6: Ipv6Addr = Ipv6Addr::new(0xfd00, 0xec2, 0, 0, 0, 0, 0, 0x23);
const REDACTED: &str = "[REDACTED]";

/// One set of AWS credentials: an access key id, its secret, an optional session token and,
/// for temporary credentials, when they expire.
///
/// The secret access key and the session token are zeroized on drop. `Debug` shows none of the
/// three strings, the access key id included.
#[derive(Clone)]
pub struct AwsCredentials {
    access_key_id: String,
    secret_access_key: Zeroizing<String>,
    session_token: Option<Zeroizing<String>>,
    expires: Option<SystemTime>,
}

impl AwsCredentials {
    pub fn new(
        access_key_id: impl Into<String>,
        secret_access_key: impl Into<String>,
        session_token: Option<String>,
        expires: Option<SystemTime>,
    ) -> Self {
        Self {
            access_key_id: access_key_id.into(),
            secret_access_key: Zeroizing::new(secret_access_key.into()),
            session_token: session_token.map(Zeroizing::new),
            expires,
        }
    }

    pub fn access_key_id(&self) -> &str {
        &self.access_key_id
    }

    pub fn secret_access_key(&self) -> &str {
        &self.secret_access_key
    }

    pub fn session_token(&self) -> Option<&str> {
        self.session_token.as_deref().map(String::as_str)
    }

    /// When temporary credentials stop being accepted; `None` for long-term keys.
    pub fn expires(&self) -> Option<SystemTime> {
        self.expires
    }
}

impl fmt::Debug for AwsCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AwsCredentials")
            .field("access_key_id", &REDACTED)
            .field("secret_access_key", &REDACTED)
            .field(
                "session_token",
                &self.session_token.as_ref().map(|_| REDACTED),
            )
            .field("expires", &self.expires)
            .finish()
    }
}

/// Where a Bedrock client gets the credentials it signs a request with.
pub trait CredentialSource: Send + Sync {
    /// The credentials to sign the next request with.
    ///
    /// # Errors
    /// `unauthorized` when no usable credentials can be had, `transport` when their endpoint
    /// did not answer, `deadline` when it answered too late, `cancelled` on cancellation; each
    /// one dispatch `not-sent`.
    fn credentials<'a>(
        &'a self,
        cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<AwsCredentials, Error>>;
}

/// An access key pair, and optionally a session token, resolved through a [`SecretResolver`] on
/// every call; nothing is cached here. Given an `EnvironmentResolver` these are the static
/// environment keys.
///
/// One trailing line end of each value is removed, as a file written by an editor or `echo`
/// carries one; what is left must be non-empty visible ASCII of at most 16 KiB. A session token
/// reference the resolver reports missing means long-term keys without a token.
pub struct StaticCredentials {
    resolver: Arc<dyn SecretResolver>,
    access_key_id: SecretRef,
    secret_access_key: SecretRef,
    session_token: Option<SecretRef>,
}

impl StaticCredentials {
    pub fn new(
        resolver: Arc<dyn SecretResolver>,
        access_key_id: SecretRef,
        secret_access_key: SecretRef,
        session_token: Option<SecretRef>,
    ) -> Self {
        Self {
            resolver,
            access_key_id,
            secret_access_key,
            session_token,
        }
    }
}

impl fmt::Debug for StaticCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StaticCredentials")
            .field("access_key_id", &self.access_key_id)
            .field("secret_access_key", &self.secret_access_key)
            .field("session_token", &self.session_token)
            .finish_non_exhaustive()
    }
}

impl CredentialSource for StaticCredentials {
    fn credentials<'a>(
        &'a self,
        cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<AwsCredentials, Error>> {
        Box::pin(async move {
            let resolver = self.resolver.as_ref();
            let access_key_id = key_part(
                resolve(resolver, &self.access_key_id, cancel)
                    .await?
                    .secret
                    .expose(),
                &self.access_key_id,
            )?;
            let secret_access_key = key_part(
                resolve(resolver, &self.secret_access_key, cancel)
                    .await?
                    .secret
                    .expose(),
                &self.secret_access_key,
            )?;
            let session_token = match &self.session_token {
                None => None,
                Some(reference) => match ask(resolver, reference, cancel).await? {
                    Ok(answer) => Some(key_part(answer.secret.expose(), reference)?),
                    Err(SecretError::Missing) => None,
                    Err(error) => return Err(secret_error(reference, error)),
                },
            };
            Ok(AwsCredentials {
                access_key_id: access_key_id.as_str().to_owned(),
                secret_access_key,
                session_token,
                expires: None,
            })
        })
    }
}

/// Temporary credentials from the container credentials endpoint
/// (`AWS_CONTAINER_CREDENTIALS_FULL_URI`), fetched with the token the resolver gives for the
/// token reference as `authorization`, and kept until five minutes before they expire.
///
/// A refresh in those five minutes that fails, other than by cancellation, answers the kept
/// credentials while their `Expiration` has not passed, and the failure once it has. Each call
/// in the window asks the endpoint again.
///
/// The URI must be `https`, or `http` to a loopback address or to an address the endpoint is
/// served on: `169.254.170.2`, `169.254.170.23` or `[fd00:ec2::23]`. A host name is not an
/// address, `localhost` included. One trailing line end of the token is removed; what is left
/// must be non-empty printable ASCII of at most 16 KiB.
pub struct ContainerCredentials {
    http: HttpClient,
    uri: Url,
    resolver: Arc<dyn SecretResolver>,
    token: SecretRef,
    cached: Mutex<Option<AwsCredentials>>,
}

impl ContainerCredentials {
    /// # Errors
    /// `unauthorized` for a URI that does not parse, carries user information or a fragment, or
    /// is neither `https` nor `http` to an admitted address. The message never quotes the URI.
    pub fn new(
        http: HttpClient,
        uri: &str,
        resolver: Arc<dyn SecretResolver>,
        token: SecretRef,
    ) -> Result<Self, Error> {
        Ok(Self {
            http,
            uri: admitted_uri(uri)?,
            resolver,
            token,
            cached: Mutex::new(None),
        })
    }

    async fn fetch(&self, cancel: &Cancel) -> Result<AwsCredentials, Error> {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", self.token_value(cancel).await?);
        let answer: Answer = self
            .http
            .get_json(self.uri.as_str(), headers, cancel)
            .await
            .map_err(endpoint_error)?;
        parse_answer(answer, SystemTime::now())
    }

    /// The token as a sensitive header value. The resolved bytes are zeroized on return.
    async fn token_value(&self, cancel: &Cancel) -> Result<HeaderValue, Error> {
        let resolved = resolve(self.resolver.as_ref(), &self.token, cancel).await?;
        let token = without_line_end(resolved.secret.expose());
        let refused = || {
            unauthorized(format!(
                "secret reference `{}` is not a container authorization token",
                self.token.as_str()
            ))
        };
        if token.is_empty()
            || token.len() > MAX_PART_BYTES
            || !token
                .iter()
                .all(|byte| *byte == b' ' || byte.is_ascii_graphic())
        {
            return Err(refused());
        }
        let mut value = HeaderValue::from_bytes(token).map_err(|_| refused())?;
        value.set_sensitive(true);
        Ok(value)
    }
}

impl fmt::Debug for ContainerCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContainerCredentials")
            .field("endpoint", &self.uri.origin().ascii_serialization())
            .field("token", &self.token)
            .finish_non_exhaustive()
    }
}

impl CredentialSource for ContainerCredentials {
    fn credentials<'a>(
        &'a self,
        cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<AwsCredentials, Error>> {
        Box::pin(async move {
            if cancel.is_cancelled() {
                return Err(Error::cancelled());
            }
            // One fetch at a time: a caller that waited here finds the credentials just fetched.
            let mut cached = tokio::select! {
                biased;
                () = cancel.cancelled() => return Err(Error::cancelled()),
                cached = self.cached.lock() => cached,
            };
            if let Some(credentials) = cached
                .as_ref()
                .filter(|credentials| fresh(credentials, SystemTime::now()))
            {
                return Ok(credentials.clone());
            }
            match self.fetch(cancel).await {
                Ok(credentials) => {
                    *cached = Some(credentials.clone());
                    Ok(credentials)
                }
                Err(error) => match cached.as_ref() {
                    Some(kept)
                        if error.code != ErrorCode::Cancelled
                            && unexpired(kept, SystemTime::now()) =>
                    {
                        Ok(kept.clone())
                    }
                    _ => Err(error),
                },
            }
        })
    }
}

/// Whether `credentials` outlive the refresh window from `now`; long-term keys always do.
fn fresh(credentials: &AwsCredentials, now: SystemTime) -> bool {
    credentials.expires.is_none_or(|expires| {
        expires
            .duration_since(now)
            .is_ok_and(|left| left > REFRESH_WINDOW)
    })
}

/// Whether `credentials` are still accepted at `now`: their `Expiration` lies after it.
fn unexpired(credentials: &AwsCredentials, now: SystemTime) -> bool {
    credentials.expires.is_none_or(|expires| expires > now)
}

fn admitted_uri(text: &str) -> Result<Url, Error> {
    let refused = || {
        unauthorized(
            "container credentials URI is not https or http to a loopback or container \
             endpoint address, or carries user information or a fragment",
        )
    };
    let uri = Url::parse(text).map_err(|_| refused())?;
    if !uri.username().is_empty() || uri.password().is_some() || uri.fragment().is_some() {
        return Err(refused());
    }
    let admitted = match (uri.scheme(), uri.host()) {
        ("https", Some(_)) => true,
        ("http", Some(Host::Ipv4(address))) => {
            address.is_loopback() || CONTAINER_IPV4.contains(&address)
        }
        ("http", Some(Host::Ipv6(address))) => address.is_loopback() || address == CONTAINER_IPV6,
        _ => false,
    };
    if admitted { Ok(uri) } else { Err(refused()) }
}

/// The container credentials endpoint's answer: the four members read, each one at most once.
/// The secret access key and the token are zeroized on drop from the moment they are decoded,
/// and no other copy of the document is built. Other members are skipped unread; an answer that
/// is not one JSON object is refused.
struct Answer {
    access_key_id: Option<String>,
    secret_access_key: Option<Zeroizing<String>>,
    token: Option<Zeroizing<String>>,
    expiration: Option<String>,
}

impl<'de> Deserialize<'de> for Answer {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(AnswerVisitor)
    }
}

struct AnswerVisitor;

impl<'de> Visitor<'de> for AnswerVisitor {
    type Value = Answer;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a container credentials object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Answer, A::Error> {
        let mut answer = Answer {
            access_key_id: None,
            secret_access_key: None,
            token: None,
            expiration: None,
        };
        while let Some(name) = map.next_key::<String>()? {
            match name.as_str() {
                "AccessKeyId" => once(&mut answer.access_key_id, "AccessKeyId", || {
                    map.next_value::<String>()
                })?,
                "SecretAccessKey" => {
                    once(&mut answer.secret_access_key, "SecretAccessKey", || {
                        map.next_value::<String>().map(Zeroizing::new)
                    })?;
                }
                "Token" => once(&mut answer.token, "Token", || {
                    map.next_value::<String>().map(Zeroizing::new)
                })?,
                "Expiration" => once(&mut answer.expiration, "Expiration", || {
                    map.next_value::<String>()
                })?,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(answer)
    }
}

/// Sets `slot` to the value `read` decodes, unless the member was already read.
fn once<T, E: de::Error>(
    slot: &mut Option<T>,
    name: &'static str,
    read: impl FnOnce() -> Result<T, E>,
) -> Result<(), E> {
    if slot.is_some() {
        return Err(E::duplicate_field(name));
    }
    *slot = Some(read()?);
    Ok(())
}

/// Reads `{AccessKeyId, SecretAccessKey, Token, Expiration}`. Every one of the four must be a
/// non-empty string, and the expiry a UTC instant after `now`. The secret members move into the
/// credentials without a copy.
fn parse_answer(answer: Answer, now: SystemTime) -> Result<AwsCredentials, Error> {
    let missing = |name: &str| {
        unauthorized(format!(
            "container credentials answer has no usable `{name}`"
        ))
    };
    let access_key_id = answer
        .access_key_id
        .filter(|value| !value.is_empty())
        .ok_or_else(|| missing("AccessKeyId"))?;
    let secret_access_key = answer
        .secret_access_key
        .filter(|value| !value.is_empty())
        .ok_or_else(|| missing("SecretAccessKey"))?;
    let session_token = answer
        .token
        .filter(|value| !value.is_empty())
        .ok_or_else(|| missing("Token"))?;
    let expiration = answer
        .expiration
        .filter(|value| !value.is_empty())
        .ok_or_else(|| missing("Expiration"))?;
    let expires = parse_rfc3339_utc(&expiration).ok_or_else(|| {
        unauthorized("container credentials answer has an `Expiration` that is not a UTC instant")
    })?;
    if expires <= now {
        return Err(unauthorized(
            "container credentials answer has already expired",
        ));
    }
    Ok(AwsCredentials {
        access_key_id,
        secret_access_key,
        session_token: Some(session_token),
        expires: Some(expires),
    })
}

/// The endpoint's failure as a credential failure. Cancellation stays cancellation and a missing
/// runtime stays unsupported. An endpoint that did not answer, cut its answer or answered 408 or
/// 5xx keeps its `transport` class, and one that answered too late its `deadline` class; either
/// keeps the `Retry-After` the transport read and is retriable. Every other refusal is
/// `unauthorized` and final. Each one is `not-sent`: no turn has been dispatched. The transport's
/// messages are fixed texts and counts, never the answer.
fn endpoint_error(error: Error) -> Error {
    match error.code {
        ErrorCode::Cancelled => Error::cancelled(),
        ErrorCode::Unsupported => error.with_dispatch(Dispatch::NotSent).with_retriable(false),
        ErrorCode::Transport | ErrorCode::Deadline => {
            let mut failed = error;
            failed.message = format!("container credentials endpoint failed: {}", failed.message);
            failed.with_dispatch(Dispatch::NotSent).with_retriable(true)
        }
        _ => unauthorized(format!(
            "container credentials endpoint failed: {}",
            error.message
        )),
    }
}

/// One static key part, as [`StaticCredentials`] describes it, from the resolved bytes of
/// `reference`.
fn key_part(bytes: &[u8], reference: &SecretRef) -> Result<Zeroizing<String>, Error> {
    let part = without_line_end(bytes);
    let refused = || {
        unauthorized(format!(
            "secret reference `{}` is not an AWS key part",
            reference.as_str()
        ))
    };
    if part.is_empty() || part.len() > MAX_PART_BYTES || !part.iter().all(u8::is_ascii_graphic) {
        return Err(refused());
    }
    let part = std::str::from_utf8(part).map_err(|_| refused())?;
    Ok(Zeroizing::new(part.to_owned()))
}

/// The resolver's answer for `reference`, unless the caller cancels first.
async fn ask(
    resolver: &dyn SecretResolver,
    reference: &SecretRef,
    cancel: &Cancel,
) -> Result<Result<ResolvedSecret, SecretError>, Error> {
    if cancel.is_cancelled() {
        return Err(Error::cancelled());
    }
    tokio::select! {
        biased;
        () = cancel.cancelled() => Err(Error::cancelled()),
        reply = resolver.resolve(reference) => Ok(reply),
    }
}

async fn resolve(
    resolver: &dyn SecretResolver,
    reference: &SecretRef,
    cancel: &Cancel,
) -> Result<ResolvedSecret, Error> {
    ask(resolver, reference, cancel)
        .await?
        .map_err(|error| secret_error(reference, error))
}

/// A refusal naming its reference. A `SecretError` is a fixed text, and the reference is the
/// operator's non-secret lookup name.
fn secret_error(reference: &SecretRef, error: SecretError) -> Error {
    unauthorized(ReferenceError::new(error, reference.clone()).to_string())
}

/// `bytes` without one trailing `\r\n` or `\n`.
fn without_line_end(bytes: &[u8]) -> &[u8] {
    bytes
        .strip_suffix(b"\r\n")
        .or_else(|| bytes.strip_suffix(b"\n"))
        .unwrap_or(bytes)
}

fn unauthorized(message: impl Into<String>) -> Error {
    Error::new(ErrorCode::Unauthorized, message)
}
