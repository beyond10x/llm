//! Renewing a Codex login through its token endpoint, and writing it back where Codex keeps it.
//!
//! Opt-in (feature `codex-renewal`): [`CodexAuthFile`] stays read-only, [`CodexAuthFile::renew`]
//! renews once when asked, and [`CodexAuthFile::renewing`] wraps it in a resolver that renews a due
//! login before answering. Matches Harness `harness-credential/src/renewal.rs` except where stated.
//!
//! **When.** Renewal is due when the access token's JWT `exp` is at or before the clock plus the
//! margin ([`DEFAULT_RENEWAL_MARGIN`], fifteen minutes). A token whose `exp` cannot be read is
//! [`Renewal::Undated`]: no refresh token is spent on it and nothing is sent or written, and the
//! read rule still refuses to send it.
//!
//! **The exchange.** One JSON POST of `client_id`, `grant_type: refresh_token` and the refresh
//! token on disk, without `scope`, through [`llm_http::HttpClient::post_json`]: never retried, no
//! redirect, no ambient proxy, the answer bounded. The refresh token appears nowhere else.
//!
//! **Write-back.** Only the values at `/tokens/access_token`, `/tokens/refresh_token` (when the
//! answer carries one), `/tokens/id_token` (when both the document and the answer carry one) and
//! `/last_refresh` (when present) change. Each is replaced at its own position in the original
//! text, so every other byte survives, also where the same value appears elsewhere; the rewritten
//! document is parsed back and checked before it is written. It is written to a new file in the
//! same directory with the original's mode, flushed, and renamed over the original only when the
//! original still holds exactly the bytes the renewal read. A symlink at the path is refused rather
//! than replaced by a file, and so is a file with more than one name (a hard link), which the
//! rename would split, leaving the other name holding a refresh token the endpoint has retired.
//!
//! **Not repeating a grant.** [`RenewingCodexAuthFile`] records the login's bytes before it
//! presents a refresh token. A grant the endpoint refused, or one whose outcome is uncertain
//! (including a resolve dropped while the grant was in flight), is not presented again while the
//! file holds the same bytes: the next resolve refuses with the same kind, as
//! [`crate::CoordinatedResolver::refresh`] does for a rejected generation. Any change to the file,
//! such as `codex` logging in again, lifts it.
//!
//! **Secret copies.** The request body is zeroized when the HTTP client drops it, and every string
//! of the answer is zeroized after the tokens are taken from it. Copies inside the HTTP and TLS
//! stack and the JSON parser's scratch buffers are not covered.
use super::{CodexAuthError, CodexAuthFile, expiry, open, read_exact, unix_seconds};
use crate::{MAX_SECRET_BYTES, ResolvedSecret, SecretError, SecretRef, SecretResolver, local};
use llm_core::{BoxFuture, Cancel, Dispatch};
use llm_http::{HeaderMap, HttpClient, Limits};
use serde::{
    Deserializer, Serialize,
    de::{Error as _, MapAccess, Visitor},
};
use serde_json::value::RawValue;
use sha2::{Digest as _, Sha256};
use std::{
    fmt, fs,
    io::Write as _,
    ops::Range,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use zeroize::{Zeroize as _, Zeroizing};

/// The token endpoint the Codex CLI itself presents its refresh token to (Harness
/// `harness-cli/src/provider.rs:224`-`228`).
pub const CODEX_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
/// The public OAuth client a Codex login is issued to; this flow has no client secret.
pub const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
/// How long before expiry a login is renewed, as Harness's caller does.
pub const DEFAULT_RENEWAL_MARGIN: Duration = Duration::from_mins(15);
/// The exchange's own bound: one small document, not a stream.
const EXCHANGE_TIMEOUT: Duration = Duration::from_secs(30);

const ACCESS: [&str; 2] = ["tokens", "access_token"];
const REFRESH: [&str; 2] = ["tokens", "refresh_token"];
const ID: [&str; 2] = ["tokens", "id_token"];
const LAST_REFRESH: [&str; 1] = ["last_refresh"];

/// Where and when a Codex login is renewed. Carries no token; `Debug` shows the endpoint, the
/// client id and the margin.
#[derive(Clone)]
pub struct CodexRenewal {
    url: String,
    client_id: String,
    margin: Duration,
    client: HttpClient,
}

impl CodexRenewal {
    /// Codex's own endpoint and client id, the default margin and a 30 s exchange.
    ///
    /// # Errors
    /// The HTTP client could not be built.
    pub fn new() -> Result<Self, llm_core::Error> {
        Ok(Self {
            url: CODEX_TOKEN_URL.to_owned(),
            client_id: CODEX_CLIENT_ID.to_owned(),
            margin: DEFAULT_RENEWAL_MARGIN,
            client: HttpClient::new(Limits {
                response_headers: EXCHANGE_TIMEOUT,
                idle: EXCHANGE_TIMEOUT,
                total: EXCHANGE_TIMEOUT,
            })?,
        })
    }

    /// Replaces the token endpoint and the client id it is presented with.
    #[must_use]
    pub fn with_endpoint(mut self, url: impl Into<String>, client_id: impl Into<String>) -> Self {
        self.url = url.into();
        self.client_id = client_id.into();
        self
    }

    /// Replaces how long before expiry a login is renewed.
    #[must_use]
    pub const fn with_margin(mut self, margin: Duration) -> Self {
        self.margin = margin;
        self
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    pub const fn margin(&self) -> Duration {
        self.margin
    }
}

impl fmt::Debug for CodexRenewal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CodexRenewal")
            .field("url", &self.url)
            .field("client_id", &self.client_id)
            .field("margin", &self.margin)
            .finish_non_exhaustive()
    }
}

/// What one renewal did. Never a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Renewal {
    /// The access token expires after the clock plus the margin: nothing sent, nothing written.
    NotDue,
    /// The access token's `exp` cannot be read: left alone, nothing sent, nothing written.
    Undated,
    /// A new access token was issued and written back.
    Renewed(Renewed),
}

/// A completed renewal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Renewed {
    /// The new access token's `exp`, when it can be read.
    pub expires_unix: Option<i128>,
    /// Whether the endpoint issued a refresh token other than the one on disk, retiring it: a copy
    /// of the login taken before the renewal no longer holds a working credential.
    pub refresh_token_rotated: bool,
}

/// Why a renewal refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenewalRefusal {
    /// The configured path is relative; nothing was opened.
    NotAbsolute,
    /// No file, or no access token in it.
    Missing,
    /// The file, its document or its access token cannot be read.
    Unavailable,
    /// The file exceeds 1 MiB.
    TooLarge,
    /// A symlink or anything but a regular file is at the path, which a renewal would replace.
    NotARegularFile,
    /// The file has more than one name (a hard link), which the rename would split.
    MultipleLinks,
    /// Due, but nothing usable at `/tokens/refresh_token`; nothing was sent.
    NoRefreshToken,
    /// The exchange refused; [`CodexRenewalError::exchange`] carries its error.
    ExchangeFailed,
    /// The answer carried no non-empty string `access_token`; nothing was written.
    AnswerWithoutAccessToken,
    /// The answer carried an empty or non-string `refresh_token`; nothing was written.
    AnswerInvalidRefreshToken,
    /// The answer carried an empty or non-string `id_token`; nothing was written.
    AnswerInvalidIdToken,
    /// The file changed while the request was in flight; the newer file was kept.
    ChangedDuringRenewal,
    /// The renewed document could not be built or written; the original is unchanged.
    WriteFailed,
}

impl RenewalRefusal {
    /// The stable code the specification names (`spec/domains/secrets.yaml`).
    pub const fn code(self) -> &'static str {
        match self {
            Self::NotAbsolute => "not-absolute",
            Self::Missing => "missing",
            Self::Unavailable => "unavailable",
            Self::TooLarge => "too-large",
            Self::NotARegularFile => "not-a-regular-file",
            Self::MultipleLinks => "multiple-links",
            Self::NoRefreshToken => "no-refresh-token",
            Self::ExchangeFailed => "exchange-failed",
            Self::AnswerWithoutAccessToken => "answer-without-access-token",
            Self::AnswerInvalidRefreshToken => "answer-invalid-refresh-token",
            Self::AnswerInvalidIdToken => "answer-invalid-id-token",
            Self::ChangedDuringRenewal => "changed-during-renewal",
            Self::WriteFailed => "write-failed",
        }
    }

    const fn from_read(error: SecretError) -> Self {
        match error {
            SecretError::Missing => Self::Missing,
            SecretError::TooLarge => Self::TooLarge,
            _ => Self::Unavailable,
        }
    }
}

/// A refused renewal: why, the file it concerns, and the exchange's own error when it refused.
/// Carries no token and no byte of the endpoint's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexRenewalError {
    refusal: RenewalRefusal,
    path: PathBuf,
    exchange: Option<llm_core::Error>,
}

impl CodexRenewalError {
    pub const fn refusal(&self) -> RenewalRefusal {
        self.refusal
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The exchange's error, for [`RenewalRefusal::ExchangeFailed`].
    pub const fn exchange(&self) -> Option<&llm_core::Error> {
        self.exchange.as_ref()
    }

    /// The refusal as a resolver reports it. Before anything is sent a refusal keeps its read
    /// code (no refresh token is `Missing`); a refusal status from the endpoint is
    /// `RefreshRejected`; every refusal after the endpoint may have issued new tokens is
    /// `RefreshUncertain`, because the refresh token on disk may have been retired.
    pub fn kind(&self) -> SecretError {
        match self.refusal {
            RenewalRefusal::NotAbsolute
            | RenewalRefusal::Unavailable
            | RenewalRefusal::NotARegularFile => SecretError::Unavailable,
            RenewalRefusal::MultipleLinks => SecretError::UnsafeSource,
            RenewalRefusal::Missing | RenewalRefusal::NoRefreshToken => SecretError::Missing,
            RenewalRefusal::TooLarge => SecretError::TooLarge,
            RenewalRefusal::ExchangeFailed => match self.exchange.as_ref().map(|e| e.dispatch) {
                Some(Dispatch::Rejected) => SecretError::RefreshRejected,
                Some(Dispatch::NotSent) => SecretError::Unavailable,
                _ => SecretError::RefreshUncertain,
            },
            RenewalRefusal::AnswerWithoutAccessToken
            | RenewalRefusal::AnswerInvalidRefreshToken
            | RenewalRefusal::AnswerInvalidIdToken
            | RenewalRefusal::ChangedDuringRenewal
            | RenewalRefusal::WriteFailed => SecretError::RefreshUncertain,
        }
    }
}

impl fmt::Display for CodexRenewalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = self.path.display();
        match self.refusal {
            RenewalRefusal::NotAbsolute => write!(
                f,
                "the Codex login path {path} is not absolute; pass the absolute path of `auth.json`"
            ),
            RenewalRefusal::Missing => {
                write!(f, "no Codex access token in {path}; run `codex` to log in")
            }
            RenewalRefusal::Unavailable => write!(
                f,
                "the Codex login in {path} cannot be read for renewal; run `codex` to refresh the login"
            ),
            RenewalRefusal::TooLarge => {
                write!(f, "the Codex login in {path} exceeds its size bound")
            }
            RenewalRefusal::NotARegularFile => write!(
                f,
                "the Codex login {path} is not a regular file, so a renewal does not replace it"
            ),
            RenewalRefusal::MultipleLinks => write!(
                f,
                "the Codex login {path} has more than one name (a hard link), so a renewal, which replaces the file, does not split it"
            ),
            RenewalRefusal::NoRefreshToken => write!(
                f,
                "the Codex login in {path} holds no refresh token at `/tokens/refresh_token`; run `codex` to log in"
            ),
            RenewalRefusal::ExchangeFailed => {
                let cause = self
                    .exchange
                    .as_ref()
                    .map_or("the exchange failed", |error| error.message.as_str());
                write!(
                    f,
                    "the token endpoint did not renew the Codex login in {path}: {cause}"
                )
            }
            RenewalRefusal::AnswerWithoutAccessToken => write!(
                f,
                "the token endpoint answered without an `access_token`; {path} was not changed"
            ),
            RenewalRefusal::AnswerInvalidRefreshToken => write!(
                f,
                "the token endpoint answered with an empty or non-string refresh token; {path} was not changed"
            ),
            RenewalRefusal::AnswerInvalidIdToken => write!(
                f,
                "the token endpoint answered with an empty or non-string id token; {path} was not changed"
            ),
            RenewalRefusal::ChangedDuringRenewal => write!(
                f,
                "{path} changed while its renewal request was in flight, so the newer login was not overwritten"
            ),
            RenewalRefusal::WriteFailed => write!(
                f,
                "the renewed Codex login could not be written to {path}; run `codex` to log in"
            ),
        }
    }
}

impl std::error::Error for CodexRenewalError {}

/// A due login, read once: the text and its generation, where each value lives in it, the refresh
/// token, and the clock it was judged against.
struct Due {
    text: Zeroizing<String>,
    generation: [u8; 32],
    now: SystemTime,
    access: Range<usize>,
    refresh: Range<usize>,
    refresh_token: Zeroizing<String>,
    id: Option<Range<usize>>,
    last_refresh: Option<Range<usize>>,
}

enum Plan {
    NotDue,
    Undated,
    Due(Due),
}

#[derive(Serialize)]
struct RefreshRequest<'a> {
    client_id: &'a str,
    grant_type: &'static str,
    refresh_token: &'a str,
}

/// The endpoint's answer, every string of which is zeroized when it is dropped.
struct Answer(serde_json::Value);

impl Drop for Answer {
    fn drop(&mut self) {
        fn scrub(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::String(text) => text.zeroize(),
                serde_json::Value::Array(items) => items.iter_mut().for_each(scrub),
                serde_json::Value::Object(members) => members.values_mut().for_each(scrub),
                _ => {}
            }
        }
        scrub(&mut self.0);
    }
}

impl CodexAuthFile {
    /// Renews this login when it is due, and writes it back. See the module documentation.
    ///
    /// # Errors
    /// Refuses as [`RenewalRefusal`] says, naming the file; never with a token.
    pub async fn renew(
        &self,
        renewal: &CodexRenewal,
        cancel: &Cancel,
    ) -> Result<Renewal, CodexRenewalError> {
        match self.plan_renewal(renewal).await? {
            Plan::NotDue => Ok(Renewal::NotDue),
            Plan::Undated => Ok(Renewal::Undated),
            Plan::Due(due) => self
                .complete_renewal(renewal, due, cancel)
                .await
                .map(Renewal::Renewed),
        }
    }

    fn refused(&self, refusal: RenewalRefusal) -> CodexRenewalError {
        CodexRenewalError {
            refusal,
            path: self.path.clone(),
            exchange: None,
        }
    }

    /// Reads the login and decides whether it is due. Sends nothing.
    async fn plan_renewal(&self, renewal: &CodexRenewal) -> Result<Plan, CodexRenewalError> {
        if !self.path.is_absolute() {
            return Err(self.refused(RenewalRefusal::NotAbsolute));
        }
        let (path, now, margin) = (self.path.clone(), (self.clock)(), renewal.margin);
        local::blocking(self.permits.clone(), move || Ok(plan(&path, now, margin)))
            .await
            .unwrap_or(Err(RenewalRefusal::Unavailable))
            .map_err(|refusal| self.refused(refusal))
    }

    /// Presents the refresh token of a due login once and writes the answer back.
    async fn complete_renewal(
        &self,
        renewal: &CodexRenewal,
        due: Due,
        cancel: &Cancel,
    ) -> Result<Renewed, CodexRenewalError> {
        // Room for the worst-case escaping up front, so the buffer is never reallocated. It moves
        // into the HTTP client, which zeroizes it when it drops the request body.
        let mut body = Zeroizing::new(Vec::with_capacity(
            64 + 6 * (renewal.client_id.len() + due.refresh_token.len()),
        ));
        serde_json::to_writer(
            &mut *body,
            &RefreshRequest {
                client_id: &renewal.client_id,
                grant_type: "refresh_token",
                refresh_token: due.refresh_token.as_str(),
            },
        )
        .map_err(|_| self.refused(RenewalRefusal::Unavailable))?;
        let answer = Answer(
            renewal
                .client
                .post_json(
                    &renewal.url,
                    HeaderMap::new(),
                    std::mem::take(&mut *body),
                    cancel,
                )
                .await
                .map_err(|error| CodexRenewalError {
                    refusal: RenewalRefusal::ExchangeFailed,
                    path: self.path.clone(),
                    exchange: Some(error),
                })?,
        );

        let access = answer
            .0
            .get("access_token")
            .and_then(serde_json::Value::as_str)
            .filter(|token| !token.is_empty())
            .map(owned)
            .ok_or_else(|| self.refused(RenewalRefusal::AnswerWithoutAccessToken))?;
        let refresh = issued(&answer.0, "refresh_token")
            .map_err(|()| self.refused(RenewalRefusal::AnswerInvalidRefreshToken))?
            .map(owned);
        let id = issued(&answer.0, "id_token")
            .map_err(|()| self.refused(RenewalRefusal::AnswerInvalidIdToken))?
            .map(owned);
        drop(answer);

        let renewed = Renewed {
            expires_unix: expiry(&access).ok(),
            refresh_token_rotated: refresh
                .as_ref()
                .is_some_and(|new| new.as_str() != due.refresh_token.as_str()),
        };
        let mut edits = vec![(due.access.clone(), &ACCESS[..], access)];
        if let Some(refresh) = refresh {
            edits.push((due.refresh.clone(), &REFRESH[..], refresh));
        }
        if let (Some(span), Some(id)) = (&due.id, id) {
            edits.push((span.clone(), &ID[..], id));
        }
        if let Some(span) = &due.last_refresh {
            edits.push((
                span.clone(),
                &LAST_REFRESH[..],
                Zeroizing::new(rfc3339(due.now)),
            ));
        }
        let rewritten =
            splice(&due.text, &edits).ok_or_else(|| self.refused(RenewalRefusal::WriteFailed))?;
        drop(edits);

        let path = self.path.clone();
        local::blocking(self.permits.clone(), move || {
            Ok(write_if_unchanged(
                &path,
                due.text.as_bytes(),
                rewritten.as_bytes(),
            ))
        })
        .await
        .unwrap_or(Err(RenewalRefusal::WriteFailed))
        .map_err(|refusal| self.refused(refusal))?;
        Ok(renewed)
    }

    /// A resolver that renews this login when it is due before answering its access token.
    pub fn renewing(self, renewal: CodexRenewal) -> RenewingCodexAuthFile {
        RenewingCodexAuthFile {
            file: self,
            renewal,
            bound: tokio::sync::Mutex::new(None),
        }
    }
}

/// A token the answer may carry: absent is `None`; present but empty or not a string refuses.
fn issued<'a>(answer: &'a serde_json::Value, key: &str) -> Result<Option<&'a str>, ()> {
    match answer.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_str()
            .filter(|token| !token.is_empty())
            .map(Some)
            .ok_or(()),
    }
}

/// A grant that must not be presented again while the login holds the same bytes.
#[derive(Clone, Copy)]
struct Bound {
    generation: [u8; 32],
    kind: SecretError,
}

/// Resolves a Codex login's access token, renewing the login first when it is due.
///
/// Renewals through one resolver are serialised, so concurrent resolves renew once. A refused
/// renewal refuses the resolve with [`CodexRenewalError::kind`] rather than answering a token the
/// renewal established is about to expire. A grant the endpoint refused (`RefreshRejected`), or
/// whose outcome is uncertain (`RefreshUncertain`, including a resolve dropped while its grant was
/// in flight), is not presented again while the file holds the same bytes; the next resolve
/// refuses with that kind until the file changes. `Debug` shows the file and the renewal, never a
/// token.
pub struct RenewingCodexAuthFile {
    file: CodexAuthFile,
    renewal: CodexRenewal,
    bound: tokio::sync::Mutex<Option<Bound>>,
}

impl RenewingCodexAuthFile {
    /// Renews when due and reads the access token, with refusals that name the file.
    ///
    /// # Errors
    /// The renewal's refusal kind, a bound grant's kind, or the read-only resolver's.
    pub async fn resolve_now(&self, reference: &SecretRef) -> Result<ResolvedSecret, SecretError> {
        if *reference != self.file.reference {
            return Err(SecretError::Missing);
        }
        let mut bound = self.bound.lock().await;
        match self
            .file
            .plan_renewal(&self.renewal)
            .await
            .map_err(|error| error.kind())?
        {
            Plan::NotDue | Plan::Undated => *bound = None,
            Plan::Due(due) => {
                let generation = due.generation;
                if let Some(previous) = *bound
                    && previous.generation == generation
                {
                    return Err(previous.kind);
                }
                // Recorded before the grant is awaited: a resolve dropped from here on leaves the
                // outcome uncertain, and the next resolve must not present the same token again.
                *bound = Some(Bound {
                    generation,
                    kind: SecretError::RefreshUncertain,
                });
                match self
                    .file
                    .complete_renewal(&self.renewal, due, &Cancel::new())
                    .await
                {
                    Ok(_) => *bound = None,
                    Err(error) => {
                        let kind = error.kind();
                        *bound = matches!(
                            kind,
                            SecretError::RefreshRejected | SecretError::RefreshUncertain
                        )
                        .then_some(Bound { generation, kind });
                        return Err(kind);
                    }
                }
            }
        }
        self.file
            .read(reference)
            .await
            .map_err(|error: CodexAuthError| error.kind())
    }
}

impl fmt::Debug for RenewingCodexAuthFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenewingCodexAuthFile")
            .field("file", &self.file)
            .field("renewal", &self.renewal)
            .finish_non_exhaustive()
    }
}

impl SecretResolver for RenewingCodexAuthFile {
    fn resolve<'a>(
        &'a self,
        reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(self.resolve_now(reference))
    }
}

/// Reads the login once and decides whether it is due, and where each value lives.
fn plan(path: &Path, now: SystemTime, margin: Duration) -> Result<Plan, RenewalRefusal> {
    let link = fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            RenewalRefusal::Missing
        } else {
            RenewalRefusal::Unavailable
        }
    })?;
    if !link.is_file() {
        return Err(RenewalRefusal::NotARegularFile);
    }
    let file = open(path).map_err(RenewalRefusal::from_read)?;
    let metadata = file.metadata().map_err(|_| RenewalRefusal::Unavailable)?;
    if !metadata.is_file() {
        return Err(RenewalRefusal::NotARegularFile);
    }
    // A second name would keep the old document, and its retired refresh token, after the rename.
    #[cfg(unix)]
    if std::os::unix::fs::MetadataExt::nlink(&metadata) != 1 {
        return Err(RenewalRefusal::MultipleLinks);
    }
    let size = usize::try_from(metadata.len())
        .ok()
        .filter(|size| *size <= MAX_SECRET_BYTES)
        .ok_or(RenewalRefusal::TooLarge)?;
    let bytes = read_exact(&file, size).map_err(RenewalRefusal::from_read)?;
    let text = Zeroizing::new(
        std::str::from_utf8(&bytes)
            .map_err(|_| RenewalRefusal::Unavailable)?
            .to_owned(),
    );
    drop(bytes);

    let root: &RawValue = serde_json::from_str(&text).map_err(|_| RenewalRefusal::Unavailable)?;
    let access_raw = lookup(root, &ACCESS)
        .map_err(|()| RenewalRefusal::Unavailable)?
        .ok_or(RenewalRefusal::Missing)?;
    let access = string(access_raw).ok_or(RenewalRefusal::Unavailable)?;
    if access.is_empty() {
        return Err(RenewalRefusal::Missing);
    }
    let Ok(expires) = expiry(&access) else {
        return Ok(Plan::Undated);
    };
    if !is_due(expires, now, margin) {
        return Ok(Plan::NotDue);
    }
    let refresh_raw = lookup(root, &REFRESH)
        .map_err(|()| RenewalRefusal::Unavailable)?
        .ok_or(RenewalRefusal::NoRefreshToken)?;
    let refresh_token = string(refresh_raw)
        .filter(|token| !token.is_empty())
        .ok_or(RenewalRefusal::NoRefreshToken)?;
    let access = span(&text, access_raw);
    let refresh = span(&text, refresh_raw);
    let id = at(root, &ID)
        .filter(|raw| string(raw).is_some())
        .map(|raw| span(&text, raw));
    let last_refresh = at(root, &LAST_REFRESH)
        .filter(|raw| string(raw).is_some())
        .map(|raw| span(&text, raw));
    let generation = Sha256::digest(text.as_bytes()).into();
    Ok(Plan::Due(Due {
        text,
        generation,
        now,
        access,
        refresh,
        refresh_token,
        id,
        last_refresh,
    }))
}

/// Whether a token expiring at `expires` is due for renewal at `now`: at or before the clock plus
/// the margin.
fn is_due(expires: i128, now: SystemTime, margin: Duration) -> bool {
    expires <= unix_seconds(now) + i128::from(margin.as_secs())
}

/// The members of a JSON object in document order; `None` for anything but an object, or an
/// object that names one key twice, which a renewal cannot aim at.
fn members(raw: &RawValue) -> Option<Vec<(String, &RawValue)>> {
    struct Members;
    impl<'de> Visitor<'de> for Members {
        type Value = Vec<(String, &'de RawValue)>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a JSON object")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut out: Self::Value = Vec::new();
            while let Some((key, value)) = map.next_entry::<String, &'de RawValue>()? {
                if out.iter().any(|(seen, _)| *seen == key) {
                    return Err(A::Error::custom("a key appears twice"));
                }
                out.push((key, value));
            }
            Ok(out)
        }
    }
    let mut deserializer = serde_json::Deserializer::from_str(raw.get());
    (&mut deserializer).deserialize_map(Members).ok()
}

fn owned(value: &str) -> Zeroizing<String> {
    Zeroizing::new(value.to_owned())
}

/// The value at an object-key path, borrowed from the original text: `Ok(None)` when a key is
/// absent, `Err` when the path crosses anything but an object or an object naming a key twice, as
/// the read rule refuses the same document.
fn lookup<'a>(root: &'a RawValue, path: &[&str]) -> Result<Option<&'a RawValue>, ()> {
    let mut current = root;
    for key in path {
        match members(current)
            .ok_or(())?
            .into_iter()
            .find(|(name, _)| name == key)
        {
            Some((_, value)) => current = value,
            None => return Ok(None),
        }
    }
    Ok(Some(current))
}

/// The value at an object-key path, when the path leads to one.
fn at<'a>(root: &'a RawValue, path: &[&str]) -> Option<&'a RawValue> {
    lookup(root, path).ok().flatten()
}

/// The JSON string a raw value holds, unescaped.
fn string(raw: &RawValue) -> Option<Zeroizing<String>> {
    serde_json::from_str::<String>(raw.get())
        .ok()
        .map(Zeroizing::new)
}

/// Where `raw`, which borrows from `text`, lies in it.
fn span(text: &str, raw: &RawValue) -> Range<usize> {
    let start = (raw.get().as_ptr() as usize) - (text.as_ptr() as usize);
    start..start + raw.get().len()
}

/// The text with each edit's span replaced by its value as a JSON string, then parsed back and
/// checked: every edited pointer holds its new value. `None` when the check fails.
fn splice(
    text: &str,
    edits: &[(Range<usize>, &[&str], Zeroizing<String>)],
) -> Option<Zeroizing<String>> {
    let mut ordered: Vec<_> = edits.iter().collect();
    ordered.sort_by_key(|(span, _, _)| std::cmp::Reverse(span.start));
    let literals: Vec<Zeroizing<String>> = ordered
        .iter()
        .map(|(_, _, value)| {
            serde_json::to_string(value.as_str())
                .ok()
                .map(Zeroizing::new)
        })
        .collect::<Option<_>>()?;
    let extra: usize = literals.iter().map(|literal| literal.len()).sum();
    let mut out = Zeroizing::new(String::with_capacity(text.len() + extra));
    out.push_str(text);
    for ((span, _, _), literal) in ordered.iter().zip(&literals) {
        out.replace_range(span.clone(), literal);
    }
    let root: &RawValue = serde_json::from_str(&out).ok()?;
    edits
        .iter()
        .all(|(_, pointer, value)| {
            at(root, pointer)
                .and_then(string)
                .is_some_and(|found| found.as_str() == value.as_str())
        })
        .then_some(out)
}

/// Replaces `path` with `contents` only while it still holds `expected`, through a new file in the
/// same directory with the original's mode, flushed before the rename.
fn write_if_unchanged(path: &Path, expected: &[u8], contents: &[u8]) -> Result<(), RenewalRefusal> {
    let directory = path.parent().ok_or(RenewalRefusal::WriteFailed)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".auth.json.renewal.")
        .tempfile_in(directory)
        .map_err(|_| RenewalRefusal::WriteFailed)?;
    temporary
        .write_all(contents)
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|_| RenewalRefusal::WriteFailed)?;
    // The comparison is the last step before the rename, after the new file is ready, so the
    // window in which another writer can be overwritten is as short as this process can make it.
    let link = fs::symlink_metadata(path).map_err(|_| RenewalRefusal::ChangedDuringRenewal)?;
    if !link.is_file() {
        return Err(RenewalRefusal::ChangedDuringRenewal);
    }
    #[cfg(unix)]
    if std::os::unix::fs::MetadataExt::nlink(&link) != 1 {
        return Err(RenewalRefusal::ChangedDuringRenewal);
    }
    let current = Zeroizing::new(fs::read(path).map_err(|_| RenewalRefusal::ChangedDuringRenewal)?);
    if current.as_slice() != expected {
        return Err(RenewalRefusal::ChangedDuringRenewal);
    }
    // The original's mode, not the new file's 0600: a login that is group-readable on purpose is
    // not narrowed by being renewed, nor widened.
    #[cfg(unix)]
    let permissions = {
        use std::os::unix::fs::PermissionsExt as _;
        fs::Permissions::from_mode(link.permissions().mode() & 0o7777)
    };
    #[cfg(not(unix))]
    let permissions = link.permissions();
    temporary
        .as_file()
        .set_permissions(permissions)
        .map_err(|_| RenewalRefusal::WriteFailed)?;
    temporary
        .persist(path)
        .map_err(|_| RenewalRefusal::WriteFailed)?;
    // The rename is durable once the directory is; a failure here leaves the new file in place.
    if let Ok(directory) = fs::File::open(directory) {
        let _ = directory.sync_all();
    }
    Ok(())
}

/// `time` as RFC 3339 UTC to the second, as Codex's `last_refresh` is read.
fn rfc3339(time: SystemTime) -> String {
    let seconds = unix_seconds(time);
    let (days, of_day) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i128::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_matches_known_instants() {
        for (seconds, text) in [
            (0, "1970-01-01T00:00:00Z"),
            (1_790_985_600, "2026-10-03T00:00:00Z"),
            (951_782_400, "2000-02-29T00:00:00Z"),
            (4_102_444_799, "2099-12-31T23:59:59Z"),
        ] {
            assert_eq!(
                rfc3339(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)),
                text
            );
        }
    }

    #[test]
    fn due_is_at_or_inside_the_margin() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let margin = Duration::from_mins(15);
        assert!(!is_due(1_000_901, now, margin));
        assert!(is_due(1_000_900, now, margin));
        assert!(is_due(999_000, now, margin));
    }

    #[test]
    fn a_span_is_found_only_at_its_own_key() {
        let text = r#"{"a": "x", "tokens": {"b": "x", "access_token": "x"}}"#;
        let root: &RawValue = serde_json::from_str(text).unwrap();
        let raw = at(root, &ACCESS).unwrap();
        assert_eq!(&text[span(text, raw)], r#""x""#);
        assert_eq!(span(text, raw).start, text.rfind(r#""x""#).unwrap());
        assert!(at(root, &["tokens", "missing"]).is_none());
        let twice: &RawValue = serde_json::from_str(r#"{"tokens": {}, "tokens": {}}"#).unwrap();
        assert!(members(twice).is_none());
    }

    #[test]
    fn a_splice_escapes_what_it_writes_and_checks_it_landed() {
        let text = r#"{"tokens": {"access_token": "old"}}"#;
        let root: &RawValue = serde_json::from_str(text).unwrap();
        let access = span(text, at(root, &ACCESS).unwrap());
        let out = splice(text, &[(access, &ACCESS[..], owned("new \"quoted\""))]).unwrap();
        assert_eq!(
            out.as_str(),
            r#"{"tokens": {"access_token": "new \"quoted\""}}"#
        );
    }
}
