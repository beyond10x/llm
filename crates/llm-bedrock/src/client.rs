//! One bound Bedrock Runtime model: what it is, where it is served, and one signed attempt.

use crate::{
    converse,
    credentials::CredentialSource,
    errors,
    eventstream::{Message, MessageDecoder},
    invoke,
    sigv4::{self, SigningParams},
};
use http::{
    HeaderMap, HeaderValue, Method,
    header::{ACCEPT, CONTENT_TYPE},
};
use llm_core::{
    BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Model, Protocol, Provenance,
    StreamSink, TurnOutcome, TurnRequest, advisory_warning,
};
use llm_http::{HttpClient, require_tokio_runtime, within_tokio_runtime};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use std::{
    fmt,
    future::Future,
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};
use url::{Host, Url};

/// The `SigV4` signing name of Bedrock Runtime: `bedrock`, not the `bedrock-runtime` host label.
const SERVICE: &str = "bedrock";
/// The media type of a streamed answer: AWS event-stream frames.
const EVENT_STREAM: &str = "application/vnd.amazon.eventstream";
const JSON: &str = "application/json";
/// The wire name an `advisory-omitted` warning carries for [`Api::AnthropicInvoke`].
const INVOKE_WIRE: &str = "Bedrock InvokeModel";
/// The longest model id Bedrock Runtime accepts in a path: an ARN of an inference profile or a
/// provisioned model included.
const MAX_MODEL_ID_BYTES: usize = 2048;
const MAX_REGION_BYTES: usize = 32;

/// What a model id keeps unencoded in the request path: the RFC 3986 unreserved characters, as
/// the AWS SDKs encode a path label. `:` and `/`, which a versioned model id and an ARN carry,
/// are encoded.
const MODEL_ID: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// The Bedrock Runtime wire a target speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Api {
    /// Converse (`/converse-stream`, `/converse`): Bedrock's own request shape, for any model
    /// that supports it. Provenance protocol [`Protocol::BedrockConverse`].
    Converse,
    /// The Anthropic Messages body on `InvokeModel` (`/invoke-with-response-stream`, `/invoke`),
    /// for Anthropic models. Provenance protocol [`Protocol::Messages`].
    AnthropicInvoke,
}

impl Api {
    /// The provenance protocol this wire's turns are bound to.
    const fn protocol(self) -> Protocol {
        match self {
            Self::Converse => Protocol::BedrockConverse,
            Self::AnthropicInvoke => Protocol::Messages,
        }
    }

    /// The last path segment of the operation.
    const fn operation(self, stream: bool) -> &'static str {
        match (self, stream) {
            (Self::Converse, true) => "converse-stream",
            (Self::Converse, false) => "converse",
            (Self::AnthropicInvoke, true) => "invoke-with-response-stream",
            (Self::AnthropicInvoke, false) => "invoke",
        }
    }
}

/// One Bedrock Runtime model: its binding identity, the wire, the Region and the model id the
/// request path names.
///
/// `model_id` is what Bedrock is asked for: a model id, an inference profile id or an ARN. It is
/// independent of `provenance.model`, the neutral name a turn's `model` must match. `endpoint`
/// replaces `https://bedrock-runtime.{region}.amazonaws.com` (a VPC endpoint, a fixture); the
/// request is still signed for `region`.
#[derive(Debug, Clone)]
pub struct BedrockTarget {
    pub provenance: Provenance,
    pub capabilities: Capabilities,
    pub region: String,
    pub model_id: String,
    pub api: Api,
    /// Whether the turn uses the streaming operation. It must match
    /// `capabilities.output_streaming`: only a streamed answer reaches the sink as it is made.
    pub stream: bool,
    pub endpoint: Option<Url>,
}

impl BedrockTarget {
    /// A validated target.
    ///
    /// # Errors
    /// As [`BedrockTarget::validate`].
    pub fn new(
        provenance: Provenance,
        capabilities: Capabilities,
        region: impl Into<String>,
        model_id: impl Into<String>,
        api: Api,
        stream: bool,
        endpoint: Option<Url>,
    ) -> Result<Self, Error> {
        let target = Self {
            provenance,
            capabilities,
            region: region.into(),
            model_id: model_id.into(),
            api,
            stream,
            endpoint,
        };
        target.validate()?;
        Ok(target)
    }

    /// Checks the target before any request is built. Messages are fixed and name no value.
    ///
    /// # Errors
    /// Refuses impossible capabilities, input during output, reasoning efforts on a Converse
    /// target (`capabilities.reasoning_efforts`), a `stream` that differs from
    /// `capabilities.output_streaming`, a provenance protocol other than the one `api` speaks, a
    /// Region that is not a lower-case AWS Region name, a model id that is empty, longer than
    /// 2048 bytes, not printable ASCII or only dots, and an endpoint that is not an origin (no
    /// credentials, path, query or fragment) on `https`, or on `http` to a loopback address
    /// (`127.0.0.0/8`, `[::1]`). A host name is not an address, `localhost` included.
    pub fn validate(&self) -> Result<(), Error> {
        self.capabilities.validate()?;
        if self.capabilities.input_during_output {
            return Err(Error::invalid(
                "Bedrock Runtime carries no input during output",
            ));
        }
        if self.api == Api::Converse && !self.capabilities.reasoning_efforts.is_empty() {
            return Err(Error::invalid(
                "a Bedrock Converse target declares no capabilities.reasoning_efforts: the wire \
                 carries no reasoning effort",
            ));
        }
        if self.capabilities.output_streaming != self.stream {
            return Err(Error::invalid(
                "a Bedrock target streams exactly when its capabilities declare streamed output",
            ));
        }
        if self.provenance.protocol != self.api.protocol() {
            return Err(Error::unsupported(match self.api {
                Api::Converse => "a Bedrock Converse target requires a bedrock-converse provenance",
                Api::AnthropicInvoke => {
                    "a Bedrock InvokeModel target requires a Messages provenance"
                }
            }));
        }
        if !region(&self.region) {
            return Err(Error::invalid("a Bedrock Region is not an AWS Region name"));
        }
        if self.model_id.is_empty()
            || self.model_id.len() > MAX_MODEL_ID_BYTES
            || !self.model_id.bytes().all(|byte| byte.is_ascii_graphic())
            || self.model_id.bytes().all(|byte| byte == b'.')
        {
            return Err(Error::invalid(
                "a Bedrock model id is empty, too long, not printable ASCII or a dot segment",
            ));
        }
        if let Some(endpoint) = &self.endpoint {
            let scheme = match (endpoint.scheme(), endpoint.host()) {
                ("https", Some(_)) => true,
                ("http", Some(Host::Ipv4(address))) => address.is_loopback(),
                ("http", Some(Host::Ipv6(address))) => address.is_loopback(),
                _ => false,
            };
            let origin = scheme
                && endpoint.username().is_empty()
                && endpoint.password().is_none()
                && endpoint.path() == "/"
                && endpoint.query().is_none()
                && endpoint.fragment().is_none();
            if !origin {
                return Err(Error::invalid(
                    "a Bedrock endpoint must be an https origin, or an http origin on a loopback \
                     address, without credentials or path",
                ));
            }
        }
        Ok(())
    }

    /// The operation's URL: `{endpoint}/model/{percent-encoded model id}/{operation}`.
    fn url(&self) -> Result<Url, Error> {
        let mut url = match &self.endpoint {
            Some(endpoint) => endpoint.clone(),
            None => Url::parse(&format!(
                "https://bedrock-runtime.{}.amazonaws.com",
                self.region
            ))
            .map_err(|_| Error::invalid("a Bedrock Region does not form an endpoint"))?,
        };
        url.set_path(&format!(
            "/model/{}/{}",
            utf8_percent_encode(&self.model_id, MODEL_ID),
            self.api.operation(self.stream)
        ));
        Ok(url)
    }
}

/// An AWS Region name as it appears in a host name and a credential scope: `us-east-1`,
/// `us-gov-west-1`. Two or more non-empty parts of lower-case letters and digits, joined by `-`.
fn region(name: &str) -> bool {
    name.len() <= MAX_REGION_BYTES
        && name.split('-').count() >= 2
        && name.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

/// One bound Bedrock Runtime model behind the [`Model`] port.
///
/// One attempt. It never retries, never refreshes a credential on its own and never changes
/// account or Region: those belong to routing and to the [`CredentialSource`]. Each turn
/// resolves credentials under the turn deadline (a failure there is `not-sent`), signs the request
/// with `SigV4` at the current time, sends it once and decodes the answer with the target's wire.
///
/// A streamed answer that fails after it has answered is final, a cut included: another attempt
/// would answer, and bill, again. A whole answer arrives only after the model generated it, so
/// every failure while it is read is final. A 429 that Bedrock names
/// `ServiceQuotaExceededException` in `x-amzn-errortype` is a final `refused`; every other 429
/// stays a retriable `rate-limited`.
pub struct BedrockClient {
    target: BedrockTarget,
    url: Url,
    http: HttpClient,
    credentials: Arc<dyn CredentialSource>,
    turn_limit: Option<Duration>,
}

impl fmt::Debug for BedrockClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BedrockClient")
            .field("target", &self.target)
            .field("url", &self.url.as_str())
            .field("turn_limit", &self.turn_limit)
            .finish_non_exhaustive()
    }
}

impl BedrockClient {
    /// # Errors
    /// Refuses a target [`BedrockTarget::validate`] refuses.
    pub fn new(
        target: BedrockTarget,
        http: HttpClient,
        credentials: Arc<dyn CredentialSource>,
    ) -> Result<Self, Error> {
        target.validate()?;
        let url = target.url()?;
        Ok(Self {
            target,
            url,
            // One absolute instant for the whole turn, taken from the transport's own total
            // limit so the two cannot drift apart. `with_turn_limit` overrides it.
            turn_limit: Some(http.limits().total),
            http: http.with_quota_rule(errors::quota),
            credentials,
        })
    }

    /// Bound the whole turn (credential resolution, the HTTP exchange and a sink that is not
    /// accepting) by one absolute instant taken when the turn starts.
    ///
    /// # Errors
    /// Refuses a zero limit and one longer than a day, on the transport's rule.
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
        // The turn deadline below is a Tokio timer: outside a runtime, refused before anything.
        require_tokio_runtime()?;
        // Refused before any I/O, and before a credential is resolved.
        let body = match self.target.api {
            Api::Converse => converse::encode_request(request, &self.target)?,
            Api::AnthropicInvoke => invoke::encode_request(request, &self.target)?,
        };
        let deadline = self.turn_limit.map(|total| Instant::now() + total);
        let headers = self.signed(&body, cancel, deadline).await?;
        let outcome = if self.target.stream {
            self.stream(headers, body, request, sink, cancel, deadline)
                .await?
        } else {
            self.whole(headers, body, request, sink, cancel, deadline)
                .await?
        };
        // Only now: a warning on the sink is visible output, and an attempt that showed output
        // before it failed would no longer be eligible for fallback.
        let (wire, omitted) = match self.target.api {
            Api::Converse => (converse::WIRE, converse::omitted_advisories(request)),
            Api::AnthropicInvoke => (INVOKE_WIRE, llm_messages::omitted_advisories(request)),
        };
        if let Some(warning) = advisory_warning(wire, &omitted) {
            converse::emit(sink, warning, cancel, deadline).await?;
        }
        Ok(outcome)
    }

    /// The request headers, signed. Credentials are resolved per request, so a rotated key is
    /// used without restarting the caller, and a source that never answers ends the turn on its
    /// deadline. They are dropped, and their secret parts zeroed, once the request is signed.
    async fn signed(
        &self,
        body: &[u8],
        cancel: &Cancel,
        deadline: Option<Instant>,
    ) -> Result<HeaderMap, Error> {
        let credentials = bounded(
            deadline,
            self.credentials.credentials(cancel),
            Dispatch::NotSent,
        )
        .await?;
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(JSON));
        headers.insert(
            ACCEPT,
            HeaderValue::from_static(if self.target.stream {
                EVENT_STREAM
            } else {
                JSON
            }),
        );
        let params = SigningParams {
            credentials: &credentials,
            region: &self.target.region,
            service: SERVICE,
            time: SystemTime::now(),
        };
        sigv4::sign(&Method::POST, &self.url, &mut headers, body, &params)?;
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
        // The turn's own instant goes to the transport, which bounds the response headers and
        // every read of the body by it.
        let mut answer = self
            .http
            .post_stream_until(
                self.url.as_str(),
                headers,
                body,
                EVENT_STREAM,
                cancel,
                deadline,
            )
            .await?;
        let mut decoder = Decoder::new(self.target.api, self.target.provenance.clone(), deadline);
        let mut frames = MessageDecoder::new();
        'read: while !decoder.is_complete() {
            let chunk = match answer.next().await {
                Ok(Some(chunk)) => chunk,
                // End of body: a frame left half read is a cut, and `finish` refuses a stream
                // whose terminal events never arrived.
                Ok(None) => {
                    frames
                        .finish()
                        .map_err(|error| decoder.cut(error.with_dispatch(Dispatch::Accepted)))?;
                    break;
                }
                Err(error) => return Err(decoder.cut(error)),
            };
            for frame in frames.push(&chunk) {
                let frame =
                    frame.map_err(|error| decoder.cut(error.with_dispatch(Dispatch::Accepted)))?;
                decoder
                    .apply(&frame, sink, cancel)
                    .await
                    .map_err(|error| decoder.attach(error))?;
                // Nothing after the terminal events is read: the turn has ended.
                if decoder.is_complete() {
                    break 'read;
                }
            }
        }
        decoder.finish(request)
    }

    async fn whole(
        &self,
        headers: HeaderMap,
        body: Vec<u8>,
        request: &TurnRequest,
        sink: &mut dyn StreamSink,
        cancel: &Cancel,
        deadline: Option<Instant>,
    ) -> Result<TurnOutcome, Error> {
        let mut answer = self
            .http
            .post_stream_until(self.url.as_str(), headers, body, JSON, cancel, deadline)
            .await?;
        let mut bytes = Vec::new();
        // The answer was generated, and billed, before its first byte: a failure reading it is
        // final, whatever the transport's class.
        while let Some(chunk) = answer
            .next()
            .await
            .map_err(|error| error.with_retriable(false))?
        {
            bytes.extend_from_slice(&chunk);
        }
        let provenance = &self.target.provenance;
        let decode = async {
            match self.target.api {
                Api::Converse => {
                    converse::decode_response(&bytes, request, provenance, sink, cancel).await
                }
                Api::AnthropicInvoke => {
                    invoke::decode_response(&bytes, request, provenance, sink, cancel).await
                }
            }
        };
        bounded(deadline, decode, Dispatch::Accepted).await
    }
}

/// The stream decoder of a target's wire. Both are boxed: one lives for a turn, and the two
/// differ in size.
enum Decoder {
    Converse(Box<converse::StreamDecoder>),
    Invoke(Box<invoke::StreamDecoder>),
}

impl Decoder {
    fn new(api: Api, target: Provenance, deadline: Option<Instant>) -> Self {
        match api {
            Api::Converse => {
                let decoder = converse::StreamDecoder::new(target);
                Self::Converse(Box::new(match deadline {
                    Some(at) => decoder.with_deadline(at),
                    None => decoder,
                }))
            }
            Api::AnthropicInvoke => {
                let decoder = invoke::StreamDecoder::new(target);
                Self::Invoke(Box::new(match deadline {
                    Some(at) => decoder.with_deadline(at),
                    None => decoder,
                }))
            }
        }
    }

    fn is_complete(&self) -> bool {
        match self {
            Self::Converse(decoder) => decoder.is_complete(),
            Self::Invoke(decoder) => decoder.is_complete(),
        }
    }

    fn has_answered(&self) -> bool {
        match self {
            Self::Converse(decoder) => decoder.has_answered(),
            Self::Invoke(decoder) => decoder.has_answered(),
        }
    }

    fn attach(&self, error: Error) -> Error {
        match self {
            Self::Converse(decoder) => decoder.attach(error),
            Self::Invoke(decoder) => decoder.attach(error),
        }
    }

    /// A body that broke off. Once the turn has answered, shown to the caller or not, it is
    /// final: another attempt would answer, and bill, again. Before, it keeps its class.
    fn cut(&self, error: Error) -> Error {
        let error = if self.has_answered() {
            error.with_retriable(false)
        } else {
            error
        };
        self.attach(error)
    }

    async fn apply(
        &mut self,
        message: &Message,
        sink: &mut dyn StreamSink,
        cancel: &Cancel,
    ) -> Result<(), Error> {
        match self {
            Self::Converse(decoder) => decoder.apply(message, sink, cancel).await,
            Self::Invoke(decoder) => decoder.apply(message, sink, cancel).await,
        }
    }

    fn finish(self, request: &TurnRequest) -> Result<TurnOutcome, Error> {
        match self {
            Self::Converse(decoder) => decoder.finish(request),
            Self::Invoke(decoder) => decoder.finish(request),
        }
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
                    Error::new(ErrorCode::Deadline, "the Bedrock turn deadline expired")
                        .with_dispatch(dispatch),
                )
            })
    })
    .await
    .map_err(|refusal| refusal.with_dispatch(dispatch))?
}

impl Model for BedrockClient {
    fn provenance(&self) -> &Provenance {
        &self.target.provenance
    }
    fn capabilities(&self) -> &Capabilities {
        &self.target.capabilities
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
