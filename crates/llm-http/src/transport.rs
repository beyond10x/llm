use crate::{Framing, SseDecoder, SseEvent};
use llm_core::{Cancel, Dispatch, Error, ErrorCode, MAX_REQUEST_BYTES};
use reqwest::{
    Client, Response, Url,
    header::{ACCEPT, CONTENT_TYPE, HeaderMap, RETRY_AFTER},
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
            idle: Duration::from_secs(60),
            total: Duration::from_secs(600),
        }
    }
}

/// Cloning shares a connection pool, never retry policy or credentials.
#[derive(Clone)]
pub struct HttpClient {
    client: Client,
    limits: Limits,
}
impl HttpClient {
    /// # Errors
    /// Refuses zero/unbounded timeouts or a failed TLS/client initialization.
    pub fn new(limits: Limits) -> Result<Self, Error> {
        if [limits.response_headers, limits.idle, limits.total]
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
            // Credentials go to the caller-selected endpoint only; ambient proxy settings never
            // redirect them.
            .no_proxy()
            // Our absolute deadline spans headers and body. A second reqwest deadline races it
            // and can turn the same expiration into an unrelated transport error.
            .build()
            .map_err(|_| Error::new(ErrorCode::Transport, "HTTP client initialization failed"))?;
        Ok(Self { client, limits })
    }

    /// The limits this client enforces, so a caller can bound the rest of its turn on the same
    /// instant rather than restating the value and letting the two drift apart.
    pub const fn limits(&self) -> &Limits {
        &self.limits
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
                    .map_err(|_| Error::new(ErrorCode::Transport, "HTTP request failed").with_dispatch(Dispatch::Unknown))?
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
                        .map_err(|_| Error::new(ErrorCode::Transport, "HTTP response body failed").with_dispatch(Dispatch::Accepted))?
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
pub fn status_error(status: u16, headers: &HeaderMap, now: SystemTime) -> Error {
    let (code, dispatch) = match status {
        401 | 403 => (ErrorCode::Unauthorized, Dispatch::Rejected),
        429 => (ErrorCode::RateLimited, Dispatch::Rejected),
        408 | 500..=599 => (ErrorCode::Transport, Dispatch::Unknown),
        300..=399 => (ErrorCode::Refused, Dispatch::Unknown),
        _ => (ErrorCode::Refused, Dispatch::Rejected),
    };
    let mut error =
        Error::new(code, format!("HTTP endpoint returned status {status}")).with_dispatch(dispatch);
    error.retry_after_ms = retry_after(headers, now)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX));
    error
}

/// Parses Retry-After with a caller-supplied clock; a hint never authorizes another attempt.
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
