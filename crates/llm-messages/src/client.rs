use crate::{StreamDecoder, decode::emit, encode_request, omitted_advisories};
use http::{
    HeaderMap, HeaderName, HeaderValue,
    header::{ACCEPT, CONTENT_TYPE},
};
use llm_core::{
    AuthKind, BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Model, Protocol,
    Provenance, StreamSink, TurnOutcome, TurnRequest, advisory_warning,
};
use llm_credentials::SecretResolver;
use llm_http::{Framing, HttpClient, require_tokio_runtime, within_tokio_runtime};
use llm_providers::{BaseUrl, Binding};
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// The wire name an `advisory-omitted` warning carries.
const WIRE: &str = "Messages";

/// The API version this projection is pinned to. Sent on every request.
///
/// A constant rather than a setting: a version the caller could change is a contract this
/// repository would no longer be pinning.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Names the API version header.
pub const VERSION_HEADER: &str = "anthropic-version";
/// Names the beta-feature header.
pub const BETA_HEADER: &str = "anthropic-beta";
/// The beta this route requires before it accepts a subscription token as a bearer. Sent only
/// for a `subscription-oauth` account; without it the route refuses the token as unauthenticated.
pub const OAUTH_BETA: &str = "oauth-2025-04-20";
/// The header `OpenCode` Go routes a conversation by. Its page (<https://opencode.ai/docs/go/>):
///
/// > Send a stable session ID in `x-opencode-session` for each conversation so we can optimize
/// > routing and prompt caching
///
/// Its Messages endpoint refuses a request without it, HTTP 400 `MissingSessionID`. A constant of
/// this wire, not a header a caller can configure.
pub const OPENCODE_SESSION_HEADER: &str = "x-opencode-session";

/// Every `OpenCode` Go base URL starts with this, as [`BaseUrl`] writes one.
const OPENCODE_GO_PREFIX: &str = "https://opencode.ai/inference/go/";

/// Whether `base_url` is `OpenCode` Go: scheme `https`, host `opencode.ai`, the default port and a
/// path under `/inference/go/`.
///
/// [`BaseUrl`] holds the `url` crate's serialisation: scheme and host lowercased, the scheme's
/// default port dropped, dot segments resolved, no userinfo, and a path ending in `/`. On that
/// form the prefix is exact: `http`, `opencode.ai.example`, `www.opencode.ai`, an explicit port
/// such as `:8443` and a path such as `/zen/` or `/inference/gox/` all differ from it.
pub fn is_opencode_go(base_url: &BaseUrl) -> bool {
    base_url.as_str().starts_with(OPENCODE_GO_PREFIX)
}

/// How many sessions this process has minted, so two clients built in the same nanosecond still
/// differ.
static MINTED: AtomicU64 = AtomicU64::new(0);

/// A new session identifier, `llm-<process id>-<wall clock nanoseconds>-<sequence>` in lowercase
/// hexadecimal: distinct between clients of one process by the sequence, and between processes
/// by the process id and the clock. Printable ASCII and at most 62 bytes, so a valid [`Id`].
fn mint_session() -> Session {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let sequence = MINTED.fetch_add(1, Ordering::Relaxed);
    Session::Minted(format!(
        "llm-{:x}-{nanos:x}-{sequence:x}",
        std::process::id()
    ))
}

/// The `OpenCode` Go session a client sends: printable ASCII either way.
enum Session {
    /// Minted by [`mint_session`].
    Minted(String),
    /// Named by the caller with [`MessagesClient::with_session`].
    Named(Id),
}

impl Session {
    fn as_str(&self) -> &str {
        match self {
            Self::Minted(session) => session,
            Self::Named(session) => session.as_str(),
        }
    }
}

/// This wire's [`llm_http::QuotaRule`]: a 429 whose error document says `rate_limit_error` as its
/// `error.type` and `enforced_spend_limit_reached` as its `error.details.error_code`, whatever
/// its headers. Anthropic's rate-limits page names that code as the way to tell its tier spend
/// cap from a rate limit, and says the spend cap keeps failing, the SDK's own retries included,
/// until access resumes (quoted in `docs/design.md`, provider failures retried before output).
fn spend_cap(_: &HeaderMap, document: &serde_json::Value) -> bool {
    let error = document.get("error");
    let says = |field: Option<&serde_json::Value>, value: &str| {
        field.and_then(serde_json::Value::as_str) == Some(value)
    };
    says(
        error.and_then(|error| error.get("type")),
        "rate_limit_error",
    ) && says(
        error
            .and_then(|error| error.get("details"))
            .and_then(|details| details.get("error_code")),
        "enforced_spend_limit_reached",
    )
}

/// One bound Messages endpoint.
///
/// One attempt. It never retries, never refreshes a credential, never changes account and
/// never infers billing from the protocol: those belong to routing and to the binding.
///
/// A 429 whose bounded error body says `rate_limit_error` with the code
/// `enforced_spend_limit_reached` is Anthropic's spend cap, which keeps failing until access
/// resumes: a final `refused`, dispatch `rejected`, with or without a `retry-after`. Every other
/// 429 stays a retriable `rate-limited`. The transport reads that body for this client's rule
/// ([`HttpClient::with_quota_rule`]), and no text of it reaches a diagnostic.
///
/// A client whose binding's base URL is `OpenCode` Go ([`is_opencode_go`]) mints one session
/// identifier when it is built and sends it as [`OPENCODE_SESSION_HEADER`] on every request;
/// [`MessagesClient::with_session`] replaces it with the caller's own. A client for any other
/// endpoint sends no such header.
pub struct MessagesClient {
    binding: Binding,
    http: HttpClient,
    resolver: Arc<dyn SecretResolver>,
    turn_limit: Option<Duration>,
    /// The `OpenCode` Go session, present exactly when the endpoint is `OpenCode` Go: minted, or
    /// an [`Id`] the caller named, so printable ASCII either way.
    session: Option<Session>,
}

impl MessagesClient {
    /// # Errors
    /// Refuses a binding whose protocol is not Messages.
    pub fn new(
        binding: Binding,
        http: HttpClient,
        resolver: Arc<dyn SecretResolver>,
    ) -> Result<Self, Error> {
        if binding.provenance().protocol != Protocol::Messages {
            return Err(Error::unsupported(
                "a Messages client requires a Messages binding",
            ));
        }
        let session = if is_opencode_go(&binding.declaration().endpoint.base_url) {
            Some(mint_session())
        } else {
            None
        };
        Ok(Self {
            binding,
            resolver,
            session,
            // One absolute instant for the whole turn, taken from the transport's own total
            // limit so the two cannot drift apart. `with_turn_limit` overrides it.
            turn_limit: Some(http.limits().total),
            http: http.with_quota_rule(spend_cap),
        })
    }

    /// Bound the whole turn — credential resolution, the HTTP exchange and a sink that is not
    /// accepting — by one absolute instant taken when the turn starts.
    ///
    /// State the same `total` the [`HttpClient`] was built with; the transport keeps enforcing
    /// its own, and the shorter of the two ends the turn.
    ///
    /// # Errors
    /// Refuses a zero or absurd limit, on the same rule the transport applies.
    pub fn with_turn_limit(mut self, total: Duration) -> Result<Self, Error> {
        if total.is_zero() || total > Duration::from_hours(24) {
            return Err(Error::invalid(
                "a turn limit must be positive and at most one day",
            ));
        }
        self.turn_limit = Some(total);
        Ok(self)
    }

    /// Names the conversation this client serves: an `OpenCode` Go client sends `session` as its
    /// [`OPENCODE_SESSION_HEADER`] from now on, in place of the one it minted. A client for any
    /// other endpoint sends no session, named or not, so this changes nothing there.
    #[must_use]
    pub fn with_session(mut self, session: Id) -> Self {
        if self.session.is_some() {
            self.session = Some(Session::Named(session));
        }
        self
    }

    /// The session header every request of this client carries, as `(name, value)`, or `None`
    /// when its endpoint is not `OpenCode` Go ([`is_opencode_go`]). The value is the same on every
    /// request, every turn and every attempt.
    pub fn session_header(&self) -> Option<(&'static str, &str)> {
        self.session
            .as_ref()
            .map(|session| (OPENCODE_SESSION_HEADER, session.as_str()))
    }

    async fn run(
        &self,
        request: &TurnRequest,
        sink: &mut dyn StreamSink,
        cancel: &Cancel,
    ) -> Result<TurnOutcome, Error> {
        // The turn deadline below is a Tokio timer: outside a runtime, refused before anything.
        require_tokio_runtime()?;
        // Refused before any I/O, and before a credential is resolved.
        let body = encode_request(request, &self.binding)?;
        let mut betas = Vec::new();
        if self.binding.declaration().account.auth_kind == AuthKind::SubscriptionOauth {
            betas.push(OAUTH_BETA);
        }
        let betas = beta_header(betas, &request.controls.hints.anthropic_beta)?;
        let deadline = self.turn_limit.map(|total| Instant::now() + total);
        // Resolved per request, so a rotated credential is used without restarting the caller.
        // A store that never answers ends the turn on the deadline, not on the caller's patience.
        let (headers, _) = bounded(
            deadline,
            self.binding.prepare_auth(&*self.resolver, cancel),
            Dispatch::NotSent,
        )
        .await?
        .into_parts();
        let headers = self.wire_headers(headers, betas)?;
        self.stream(headers, body, request, sink, cancel, deadline)
            .await
    }

    /// Every header a request carries, added to the authentication headers `headers`: the content
    /// headers, the API version, the `anthropic-beta` header when one is due, and the
    /// [`MessagesClient::session_header`], read from the accessor so what it reports is what is
    /// sent.
    fn wire_headers(
        &self,
        mut headers: HeaderMap,
        betas: Option<HeaderValue>,
    ) -> Result<HeaderMap, Error> {
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
        headers.insert(
            version_header(),
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );
        if let Some(betas) = betas {
            headers.insert(HeaderName::from_static(BETA_HEADER), betas);
        }
        if let Some((name, value)) = self.session_header() {
            // The session is printable ASCII (`Session`), so this refusal is a guard rather than
            // a reachable path, as on Responses.
            let value = HeaderValue::from_str(value)
                .map_err(|_| Error::invalid("the OpenCode session is not a header value"))?;
            headers.insert(HeaderName::from_static(name), value);
        }
        Ok(headers)
    }

    async fn stream(
        &self,
        headers: HeaderMap,
        body: Vec<u8>,
        request: &TurnRequest,
        sink: &mut dyn StreamSink,
        cancel: &Cancel,
        deadline: Option<Instant>,
    ) -> Result<TurnOutcome, Error> {
        // This route has no `[DONE]` sentinel: its terminal marker is a `message_stop` payload.
        //
        // The turn's own instant goes to the transport, which bounds the response headers and
        // every read of the stream by it. Without that the exchange would run on a clock the
        // transport starts for itself, and a turn stating a shorter bound would outlive it.
        let mut stream = self
            .http
            .post_sse_until(
                self.binding.request_url(),
                headers,
                body,
                Framing::PayloadsOnly,
                cancel,
                deadline,
            )
            .await?;
        let mut decoder = StreamDecoder::new(self.binding.provenance().clone());
        if let Some(deadline) = deadline {
            decoder = decoder.with_deadline(deadline);
        }
        loop {
            match stream.next().await {
                Ok(Some(event)) => decoder
                    .apply(&event, sink, cancel)
                    .await
                    .map_err(|error| decoder.attach(error))?,
                // End of body. `finish` refuses unless the terminal event actually arrived.
                Ok(None) => break,
                // A cut. Once the decoder has answered, shown to the caller or not (a block that
                // opened empty), it is final: another attempt would answer, and bill, again. As
                // on Responses, this is stricter than routing's rule, which sees only events.
                Err(error) => {
                    let error = if decoder.has_answered() {
                        error.with_retriable(false)
                    } else {
                        error
                    };
                    return Err(decoder.attach(error));
                }
            }
            if decoder.is_complete() {
                break;
            }
        }
        let outcome = decoder.finish(request)?;
        // Only now: a warning on the sink is visible output, and an attempt that showed output
        // before it failed would no longer be eligible for fallback.
        if let Some(warning) = advisory_warning(WIRE, &omitted_advisories(request)) {
            emit(sink, warning, cancel, deadline).await?;
        }
        Ok(outcome)
    }
}

/// Runs one wait under the turn's absolute deadline, when the caller configured one. Each poll
/// asks for the runtime first, so a wait polled on a thread in no runtime is refused with
/// `dispatch` before the timer is created and whenever it is moved out part-way.
async fn bounded<T>(
    deadline: Option<Instant>,
    work: impl Future<Output = Result<T, Error>>,
    dispatch: Dispatch,
) -> Result<T, Error> {
    within_tokio_runtime(async move {
        let Some(deadline) = deadline else {
            return work.await;
        };
        tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), work)
            .await
            .unwrap_or_else(|_| {
                Err(
                    Error::new(ErrorCode::Deadline, "the Messages turn deadline expired")
                        .with_dispatch(dispatch),
                )
            })
    })
    .await
    .map_err(|refusal| refusal.with_dispatch(dispatch))?
}

/// The one `anthropic-beta` header a turn sends: the betas the route needs (the [`OAUTH_BETA`]
/// of a subscription), then the turn's own betas, each name once, joined by commas. `None` when
/// there is nothing to send.
fn beta_header<'a>(
    mut names: Vec<&'a str>,
    requested: &'a [String],
) -> Result<Option<HeaderValue>, Error> {
    for name in requested {
        if !names.contains(&name.as_str()) {
            names.push(name);
        }
    }
    if names.is_empty() {
        return Ok(None);
    }
    HeaderValue::from_str(&names.join(","))
        .map(Some)
        .map_err(|_| Error::invalid("`anthropic-beta` names do not form a header value"))
}

fn version_header() -> HeaderName {
    HeaderName::from_static(VERSION_HEADER)
}

impl Model for MessagesClient {
    fn provenance(&self) -> &Provenance {
        self.binding.provenance()
    }
    fn capabilities(&self) -> &Capabilities {
        self.binding.capabilities()
    }
    fn turn<'a>(
        &'a self,
        request: &'a TurnRequest,
        sink: &'a mut dyn StreamSink,
        cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        Box::pin(self.run(request, sink, cancel))
    }
}

#[cfg(test)]
mod tests;
