use crate::{Framing, SseDecoder, SseEvent};
use llm_core::{Cancel, Dispatch, Error, ErrorCode, MAX_REQUEST_BYTES};
use reqwest::{
    Client, Response, Url,
    header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderValue, RETRY_AFTER},
};
use std::{
    collections::VecDeque,
    time::{Duration, SystemTime},
};
use tokio::time::Instant;

const EVENT_STREAM: &str = "text/event-stream";

/// Whether any `accept` header of the request names `text/event-stream`, parameters ignored.
fn accepts_event_stream(headers: &HeaderMap) -> bool {
    headers
        .get_all(ACCEPT)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|range| range.split(';').next())
        .any(|range| range.trim().eq_ignore_ascii_case(EVENT_STREAM))
}

/// The default bound on establishing one connection, as Harness streams with
/// (`Settings::streaming`, connect 15 s).
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// The idle default is 180 s, Harness's per-read bound: long enough for a model that thinks
/// silently between two events.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub response_headers: Duration,
    pub idle: Duration,
    pub total: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            response_headers: Duration::from_secs(60),
            idle: Duration::from_secs(180),
            total: Duration::from_secs(600),
        }
    }
}

/// Cloning shares a connection pool, never retry policy or credentials.
#[derive(Clone)]
pub struct HttpClient {
    client: Client,
    limits: Limits,
    connect: Duration,
}
impl HttpClient {
    /// A client with the default connect bound, [`CONNECT_TIMEOUT`].
    ///
    /// # Errors
    /// Refuses zero/unbounded timeouts or a failed TLS/client initialization.
    pub fn new(limits: Limits) -> Result<Self, Error> {
        Self::with_connect_timeout(limits, CONNECT_TIMEOUT)
    }

    /// A client whose connection attempts end after `connect`. A connection not established in
    /// time is a request that failed before any response: `transport`, dispatch `unknown`,
    /// retriable.
    ///
    /// # Errors
    /// Refuses zero/unbounded timeouts or a failed TLS/client initialization.
    pub fn with_connect_timeout(limits: Limits, connect: Duration) -> Result<Self, Error> {
        if [limits.response_headers, limits.idle, limits.total, connect]
            .iter()
            .any(|v| v.is_zero() || *v > Duration::from_hours(24))
        {
            return Err(Error::invalid(
                "HTTP timeouts must be positive and at most one day",
            ));
        }
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(connect)
            // Credentials go to the caller-selected endpoint only; ambient proxy settings never
            // redirect them.
            .no_proxy()
            // Our absolute deadline spans headers and body. A second reqwest deadline races it
            // and can turn the same expiration into an unrelated transport error.
            .build()
            .map_err(|_| Error::new(ErrorCode::Transport, "HTTP client initialization failed"))?;
        Ok(Self {
            client,
            limits,
            connect,
        })
    }

    /// The limits this client enforces, so a caller can bound the rest of its turn on the same
    /// instant rather than restating the value and letting the two drift apart.
    pub const fn limits(&self) -> &Limits {
        &self.limits
    }

    /// How long establishing one connection may take.
    pub const fn connect_timeout(&self) -> Duration {
        self.connect
    }

    /// Sends exactly once, with a request-time header list supplied by the caller.
    ///
    /// A success with no `content-type` is read as an event stream when the caller's `accept`
    /// header asked for `text/event-stream`; a success naming any other media type is refused.
    /// # Errors
    /// Refuses invalid URLs/body bounds, cancellation, timeout, transport failure and non-SSE replies.
    pub async fn post_sse(
        &self,
        url: &str,
        headers: HeaderMap,
        body: Vec<u8>,
        framing: Framing,
        cancel: &Cancel,
    ) -> Result<SseStream, Error> {
        self.post_sse_until(url, headers, body, framing, cancel, None)
            .await
    }

    /// The same single attempt, bounded additionally by an absolute instant the caller already
    /// holds: the start of a turn, a parent deadline, a budget expiry.
    ///
    /// The exchange ends at whichever comes first, that instant or this client's own `total`, and
    /// the bound covers the response headers and every read of the returned [`SseStream`] — so a
    /// caller whose own limit is the shorter of the two does not have to restate it here and let
    /// the two drift apart. A caller holding no such instant passes `None`.
    ///
    /// # Errors
    /// As [`HttpClient::post_sse`]; the caller's instant expiring reports `Deadline`.
    pub async fn post_sse_until(
        &self,
        url: &str,
        mut headers: HeaderMap,
        body: Vec<u8>,
        framing: Framing,
        cancel: &Cancel,
        until: Option<std::time::Instant>,
    ) -> Result<SseStream, Error> {
        if cancel.is_cancelled() {
            return Err(Error::cancelled());
        }
        let url = Url::parse(url).map_err(|_| Error::invalid("invalid HTTP endpoint URL"))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(Error::invalid(
                "HTTP endpoint must have a host and no embedded credentials or fragment",
            ));
        }
        if body.len() > MAX_REQUEST_BYTES || headers.len() > 64 {
            return Err(Error::too_large(
                "HTTP request body or header count exceeds bound",
            ));
        }
        let asked_for_event_stream = accepts_event_stream(&headers);
        for value in headers.values_mut() {
            value.set_sensitive(true);
        }
        let started = Instant::now();
        // The shorter of the two ends the exchange: a caller cannot lengthen this client's own
        // bound by naming a later instant, and a caller's shorter one is not ignored.
        let deadline = match until {
            None => started + self.limits.total,
            Some(until) => (started + self.limits.total).min(Instant::from_std(until)),
        };
        let request = self
            .client
            .post(url)
            .headers(headers)
            .body(body)
            .build()
            .map_err(|_| Error::invalid("HTTP request could not be constructed"))?;
        let response = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(Error::cancelled().with_dispatch(Dispatch::Unknown)),
            reply = tokio::time::timeout_at(deadline.min(started + self.limits.response_headers), self.client.execute(request)) => {
                reply.map_err(|_| Error::new(ErrorCode::Deadline, "HTTP response-header deadline exceeded").with_dispatch(Dispatch::Unknown))?
                    .map_err(|_| Error::new(ErrorCode::Transport, "HTTP request failed").with_dispatch(Dispatch::Unknown).with_retriable(true))?
            }
        };
        if !response.status().is_success() {
            return Err(status_error(
                response.status().as_u16(),
                response.headers(),
                SystemTime::now(),
            ));
        }
        // A success that names a media type must name an event stream. One that names none is
        // read as the event stream the request asked for (the Codex backend answers that way);
        // a request that did not ask for one gets no such reading.
        let event_stream = match response.headers().get(CONTENT_TYPE) {
            Some(value) => value
                .to_str()
                .ok()
                .and_then(|v| v.split(';').next())
                .is_some_and(|v| v.trim().eq_ignore_ascii_case(EVENT_STREAM)),
            None => asked_for_event_stream,
        };
        if !event_stream {
            return Err(
                Error::protocol("HTTP success response is not text/event-stream")
                    .with_dispatch(Dispatch::Accepted),
            );
        }
        Ok(SseStream {
            response: Some(response),
            decoder: SseDecoder::new(framing),
            queued: VecDeque::new(),
            cancel: cancel.clone(),
            deadline,
            idle: self.limits.idle,
            failure: None,
        })
    }
}

/// A live response. Dropping this value drops the HTTP body; this says nothing about billed work.
pub struct SseStream {
    response: Option<Response>,
    decoder: SseDecoder,
    queued: VecDeque<Result<SseEvent, Error>>,
    cancel: Cancel,
    deadline: Instant,
    idle: Duration,
    failure: Option<Error>,
}
impl SseStream {
    /// # Errors
    /// Reports cancellation, deadline, framing and transport failures; never synthesizes completion.
    pub async fn next(&mut self) -> Result<Option<SseEvent>, Error> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let result = self.next_inner().await;
        if let Err(error) = &result {
            self.response = None;
            self.queued.clear();
            self.failure = Some(error.clone());
        }
        result
    }

    async fn next_inner(&mut self) -> Result<Option<SseEvent>, Error> {
        loop {
            if self.cancel.is_cancelled() {
                return Err(Error::cancelled().with_dispatch(Dispatch::Accepted));
            }
            if Instant::now() >= self.deadline {
                return Err(
                    Error::new(ErrorCode::Deadline, "HTTP stream deadline exceeded")
                        .with_dispatch(Dispatch::Accepted),
                );
            }
            if let Some(event) = self.queued.pop_front() {
                return event
                    .map(Some)
                    .map_err(|e| e.with_dispatch(Dispatch::Accepted));
            }
            let Some(response) = self.response.as_mut() else {
                return Ok(None);
            };
            let chunk = tokio::select! {
                biased;
                () = self.cancel.cancelled() => return Err(Error::cancelled().with_dispatch(Dispatch::Accepted)),
                chunk = tokio::time::timeout_at(self.deadline.min(Instant::now() + self.idle), response.chunk()) => {
                    chunk.map_err(|_| Error::new(ErrorCode::Deadline, "HTTP idle or total deadline exceeded").with_dispatch(Dispatch::Accepted))?
                        .map_err(|_| Error::new(ErrorCode::Transport, "HTTP response body failed").with_dispatch(Dispatch::Accepted).with_retriable(true))?
                }
            };
            if let Some(chunk) = chunk {
                self.queued.extend(self.decoder.push(&chunk));
            } else {
                self.response = None;
                self.decoder
                    .finish()
                    .map_err(|error| error.with_dispatch(Dispatch::Accepted))?;
                return Ok(None);
            }
        }
    }
}

/// HTTP status evidence only. Arbitrary error-body text is never copied into a diagnostic.
///
/// 408, 429 and 500-599 (529 included) are retriable, as in Harness `status_error`: the far side
/// did not get to answer. 409 and every other refusal are final: the identical request meets
/// them again. A 5xx keeps dispatch `unknown`, because the work may have run.
pub fn status_error(status: u16, headers: &HeaderMap, now: SystemTime) -> Error {
    let (code, dispatch, retriable) = match status {
        401 | 403 => (ErrorCode::Unauthorized, Dispatch::Rejected, false),
        429 => (ErrorCode::RateLimited, Dispatch::Rejected, true),
        408 | 500..=599 => (ErrorCode::Transport, Dispatch::Unknown, true),
        300..=399 => (ErrorCode::Refused, Dispatch::Unknown, false),
        _ => (ErrorCode::Refused, Dispatch::Rejected, false),
    };
    let mut error = Error::new(code, format!("HTTP endpoint returned status {status}"))
        .with_dispatch(dispatch)
        .with_retriable(retriable);
    error.retry_after_ms = retry_after(headers, now)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX));
    error
}

/// Parses Retry-After with a caller-supplied clock. A hint is a delay, never a reason to retry:
/// routing waits on it, capped, only for a failure whose class is already retriable.
pub fn retry_after(headers: &HeaderMap, now: SystemTime) -> Option<Duration> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) {
        return Some(Duration::from_secs(value.parse().unwrap_or(u64::MAX)));
    }
    Some(
        httpdate::parse_http_date(value)
            .ok()?
            .duration_since(now)
            .unwrap_or_default(),
    )
}

// One JSON document exchange, beside the streaming POST above. `post_sse_until` applies the same
// URL and request bounds inline.
impl HttpClient {
    /// One JSON document exchange: `body` (JSON the caller serialised) out as
    /// `application/json`, one JSON document back. For a credential exchange, not a turn.
    ///
    /// Sends exactly once with this client's redirect, proxy and deadline rules. The answer is
    /// read up to [`MAX_EXCHANGE_BYTES`] inclusive. Every refusal is final (`retriable` false),
    /// whatever the status table says for a turn: the body may be a non-idempotent document such
    /// as a refresh grant. Neither the request body nor the answer body reaches a diagnostic.
    /// The request body is zeroized when the client drops it and the answer bytes after parsing;
    /// copies inside the HTTP and TLS stack, and the returned document, are not.
    ///
    /// # Errors
    /// Refuses invalid URLs and bounds, cancellation, deadlines, transport failures, a failing
    /// status (as [`status_error`]), an answer over the bound and a success that is not JSON.
    pub async fn post_json(
        &self,
        url: &str,
        headers: HeaderMap,
        body: Vec<u8>,
        cancel: &Cancel,
    ) -> Result<serde_json::Value, Error> {
        self.post_json_once(url, headers, body, cancel)
            .await
            .map_err(|error| error.with_retriable(false))
    }

    async fn post_json_once(
        &self,
        url: &str,
        mut headers: HeaderMap,
        body: Vec<u8>,
        cancel: &Cancel,
    ) -> Result<serde_json::Value, Error> {
        if cancel.is_cancelled() {
            return Err(Error::cancelled());
        }
        let url = checked_url(url)?;
        check_request_bounds(&headers, &body)?;
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(JSON));
        headers.insert(ACCEPT, HeaderValue::from_static(JSON));
        for value in headers.values_mut() {
            value.set_sensitive(true);
        }
        let started = Instant::now();
        let deadline = started + self.limits.total;
        let request = self
            .client
            .post(url)
            .headers(headers)
            .body(reqwest::Body::from(bytes::Bytes::from_owner(
                zeroize::Zeroizing::new(body),
            )))
            .build()
            .map_err(|_| Error::invalid("HTTP request could not be constructed"))?;
        let mut response = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(Error::cancelled().with_dispatch(Dispatch::Unknown)),
            reply = tokio::time::timeout_at(deadline.min(started + self.limits.response_headers), self.client.execute(request)) => {
                reply.map_err(|_| Error::new(ErrorCode::Deadline, "HTTP response-header deadline exceeded").with_dispatch(Dispatch::Unknown))?
                    .map_err(|_| Error::new(ErrorCode::Transport, "HTTP request failed").with_dispatch(Dispatch::Unknown))?
            }
        };
        if !response.status().is_success() {
            // The body is never read: its sensitivity is unknown here.
            return Err(status_error(
                response.status().as_u16(),
                response.headers(),
                SystemTime::now(),
            ));
        }
        let too_large = || {
            Error::too_large(format!(
                "HTTP answer passed the {MAX_EXCHANGE_BYTES} byte bound"
            ))
            .with_dispatch(Dispatch::Accepted)
        };
        if response
            .content_length()
            .is_some_and(|length| length > MAX_EXCHANGE_BYTES as u64)
        {
            return Err(too_large());
        }
        // Allocated once at the bound, so it is never reallocated and every byte is zeroized.
        let mut answer = zeroize::Zeroizing::new(Vec::with_capacity(MAX_EXCHANGE_BYTES));
        loop {
            let chunk = tokio::select! {
                biased;
                () = cancel.cancelled() => return Err(Error::cancelled().with_dispatch(Dispatch::Accepted)),
                chunk = tokio::time::timeout_at(deadline.min(Instant::now() + self.limits.idle), response.chunk()) => {
                    chunk.map_err(|_| Error::new(ErrorCode::Deadline, "HTTP idle or total deadline exceeded").with_dispatch(Dispatch::Accepted))?
                        .map_err(|_| Error::new(ErrorCode::Transport, "HTTP response body failed").with_dispatch(Dispatch::Accepted))?
                }
            };
            let Some(chunk) = chunk else { break };
            if answer.len() + chunk.len() > MAX_EXCHANGE_BYTES {
                return Err(too_large());
            }
            answer.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&answer).map_err(|_| {
            Error::protocol(format!(
                "HTTP success answer of {} byte(s) is not JSON",
                answer.len()
            ))
            .with_dispatch(Dispatch::Accepted)
        })
    }
}

/// The largest answer [`HttpClient::post_json`] reads: 64 KiB, inclusive.
pub const MAX_EXCHANGE_BYTES: usize = 64 * 1024;

const JSON: &str = "application/json";

/// An absolute `http`/`https` URL with a host, no embedded credentials and no fragment.
fn checked_url(url: &str) -> Result<Url, Error> {
    let url = Url::parse(url).map_err(|_| Error::invalid("invalid HTTP endpoint URL"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::invalid(
            "HTTP endpoint must have a host and no embedded credentials or fragment",
        ));
    }
    Ok(url)
}

fn check_request_bounds(headers: &HeaderMap, body: &[u8]) -> Result<(), Error> {
    if body.len() > MAX_REQUEST_BYTES || headers.len() > 64 {
        return Err(Error::too_large(
            "HTTP request body or header count exceeds bound",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ACCEPT, Framing, HeaderMap, HttpClient, Limits};
    use llm_core::{Cancel, Dispatch, ErrorCode};
    use reqwest::header::HeaderValue;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    /// Answers one request with a `200` that names no `content-type`, then one event.
    async fn untyped_success(accept: Option<&'static str>) -> Result<(), llm_core::Error> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a local port");
        let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("a connection");
            let mut seen = Vec::new();
            let mut buffer = [0; 1024];
            while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                let length = socket.read(&mut buffer).await.expect("a readable socket");
                assert!(length > 0, "the client closed before sending a request");
                seen.extend_from_slice(&buffer[..length]);
            }
            let _ = socket
                .write_all(b"HTTP/1.1 200 OK\r\nconnection: close\r\n\r\ndata: {\"n\":1}\n\n")
                .await;
            let _ = socket.shutdown().await;
        });
        let mut headers = HeaderMap::new();
        if let Some(accept) = accept {
            headers.insert(ACCEPT, HeaderValue::from_static(accept));
        }
        let client = HttpClient::new(Limits::default()).expect("bounded transport");
        let outcome = client
            .post_sse(
                &url,
                headers,
                Vec::new(),
                Framing::PayloadsOnly,
                &Cancel::new(),
            )
            .await
            .map(|_| ());
        server.await.expect("the fixture server");
        outcome
    }

    /// The other half of the Codex reading: an untyped success is an event stream only for a
    /// request whose `accept` asked for one.
    #[tokio::test]
    async fn an_untyped_success_is_an_event_stream_only_when_one_was_asked_for() {
        untyped_success(Some("application/json, text/event-stream; q=0.9"))
            .await
            .expect("a request that asked for an event stream reads an untyped success as one");
        for accept in [None, Some("application/json")] {
            let error = untyped_success(accept)
                .await
                .expect_err("a request that did not ask for an event stream gets no such reading");
            assert_eq!(error.code, ErrorCode::Protocol, "{error}");
            assert_eq!(error.dispatch, Dispatch::Accepted, "{error}");
        }
    }
}
