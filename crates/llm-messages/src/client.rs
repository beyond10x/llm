use crate::{StreamDecoder, encode_request};
use http::{
    HeaderMap, HeaderName, HeaderValue,
    header::{ACCEPT, CONTENT_TYPE},
};
use llm_core::{
    BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Model, Protocol, Provenance,
    StreamSink, TurnOutcome, TurnRequest,
};
use llm_credentials::SecretResolver;
use llm_http::{Framing, HttpClient};
use llm_providers::Binding;
use std::{
    future::Future,
    sync::Arc,
    time::{Duration, Instant},
};

/// The API version this projection is pinned to. Sent on every request.
///
/// A constant rather than a setting: a version the caller could change is a contract this
/// repository would no longer be pinning.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Names the API version header.
pub const VERSION_HEADER: &str = "anthropic-version";

/// One bound Messages endpoint.
///
/// One attempt. It never retries, never refreshes a credential, never changes account and
/// never infers billing from the protocol: those belong to routing and to the binding.
pub struct MessagesClient {
    binding: Binding,
    http: HttpClient,
    resolver: Arc<dyn SecretResolver>,
    turn_limit: Option<Duration>,
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
        Ok(Self {
            binding,
            resolver,
            // One absolute instant for the whole turn, taken from the transport's own total
            // limit so the two cannot drift apart. `with_turn_limit` overrides it.
            turn_limit: Some(http.limits().total),
            http,
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

    async fn run(
        &self,
        request: &TurnRequest,
        sink: &mut dyn StreamSink,
        cancel: &Cancel,
    ) -> Result<TurnOutcome, Error> {
        // Refused before any I/O, and before a credential is resolved.
        let body = encode_request(request, &self.binding)?;
        let deadline = self.turn_limit.map(|total| Instant::now() + total);
        // Resolved per request, so a rotated credential is used without restarting the caller.
        // A store that never answers ends the turn on the deadline, not on the caller's patience.
        let (mut headers, _) = bounded(
            deadline,
            self.binding.prepare_auth(&*self.resolver, cancel),
            Dispatch::NotSent,
        )
        .await?
        .into_parts();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
        headers.insert(
            version_header(),
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );
        self.stream(headers, body, request, sink, cancel, deadline)
            .await
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
                Err(error) => return Err(decoder.attach(error)),
            }
            if decoder.is_complete() {
                break;
            }
        }
        decoder.finish(request)
    }
}

/// Runs one wait under the turn's absolute deadline, when the caller configured one.
async fn bounded<T>(
    deadline: Option<Instant>,
    work: impl Future<Output = Result<T, Error>>,
    dispatch: Dispatch,
) -> Result<T, Error> {
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
