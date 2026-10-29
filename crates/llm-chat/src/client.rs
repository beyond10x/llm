//! A single-attempt Chat Completions client over the shared bounded HTTP/SSE transport.
use crate::{
    incoming::StreamProjection,
    outgoing::{omitted_advisories, project_request},
};
use http::{
    HeaderValue,
    header::{ACCEPT, CONTENT_TYPE},
};
use llm_core::{
    BoxFuture, Cancel, Capabilities, Dispatch, Error, Model, Provenance, RefusalCause, StreamSink,
    TurnOutcome, TurnRequest, advisory_warning,
};
use llm_credentials::SecretResolver;
use llm_http::{Framing, HeaderMap, HttpClient, require_tokio_runtime, within_tokio_runtime};
use llm_providers::Binding;
use serde_json::Value;
use std::sync::Arc;

/// The name the advisory warning gives this wire.
const WIRE: &str = "Chat Completions";

/// This wire's [`llm_http::RefusalRule`]. Chat Completions shares the `OpenAI` error document: a
/// 400 whose `error.code` is `context_length_exceeded`, exactly, is a request over the model's
/// context window. Every other 400, a code in another spelling or outside `error` included, names
/// no cause, and the provider's prose is never read.
fn named_cause(_: &HeaderMap, document: &Value) -> Option<RefusalCause> {
    match document
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
    {
        Some("context_length_exceeded") => Some(RefusalCause::ContextLengthExceeded),
        _ => None,
    }
}

/// One immutable binding served over Chat Completions. Retries, alternates and fallback
/// belong to routing; this client sends exactly once.
///
/// A 400 whose bounded error body names `context_length_exceeded` as its `error.code` stays the
/// final `refused` every 400 is and carries [`RefusalCause::ContextLengthExceeded`] with fixed
/// text naming it; every other 400 names no cause. The transport reads that body for this
/// client's rule ([`HttpClient::with_refusal_rule`]), and no text of it reaches a diagnostic.
pub struct ChatClient {
    binding: Binding,
    http: HttpClient,
    resolver: Arc<dyn SecretResolver>,
}

impl ChatClient {
    pub fn new(binding: Binding, http: HttpClient, resolver: Arc<dyn SecretResolver>) -> Self {
        Self {
            binding,
            http: http.with_refusal_rule(named_cause),
            resolver,
        }
    }

    pub fn binding(&self) -> &Binding {
        &self.binding
    }
}

impl Model for ChatClient {
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
        Box::pin(async move {
            // The credential may be read on Tokio's blocking pool and the exchange runs on Tokio:
            // outside a runtime, refused before either.
            require_tokio_runtime()?;
            let target = self.binding.provenance();
            let body = project_request(request, &self.binding, true)?;
            let body = serde_json::to_vec(&body)
                .map_err(|_| Error::invalid("chat request could not be encoded"))?;
            // A resolver may need Tokio after a wait of its own: each poll asks for the runtime.
            let (mut headers, _generation) =
                within_tokio_runtime(self.binding.prepare_auth(self.resolver.as_ref(), cancel))
                    .await??
                    .into_parts();
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
            let mut stream = self
                .http
                .post_sse(
                    self.binding.request_url(),
                    headers,
                    body,
                    Framing::DoneSentinel,
                    cancel,
                )
                .await?;

            let mut projection = StreamProjection::new(target.clone());
            loop {
                match stream.next().await {
                    Ok(Some(event)) => {
                        let produced = projection
                            .accept(&event)
                            .map_err(|error| retain(error, &projection, target))?;
                        for event in produced {
                            sink.emit(event)
                                .await
                                .map_err(|error| retain(error, &projection, target))?;
                        }
                    }
                    Ok(None) => break,
                    // A cut. Once the stream has carried output, shown to the caller or not (a
                    // call whose name has not arrived, a finish reason), it is final: another
                    // attempt would answer, and bill, again. As on Responses and Messages.
                    Err(error) => {
                        let error = if projection.has_answered() {
                            error.with_retriable(false)
                        } else {
                            error
                        };
                        return Err(retain(error, &projection, target));
                    }
                }
            }
            let outcome = projection
                .finish()
                .map_err(|error| retain(error, &projection, target))?;
            // Output already reached the caller's sink, so a refusal here is also a failure
            // after dispatch and keeps the same evidence.
            outcome
                .validate_for(request, target)
                .map_err(|error| retain(error, &projection, target))?;
            // Only now: a warning on the sink is visible output, and an attempt that showed
            // output before it failed would no longer be eligible for fallback.
            if let Some(warning) = advisory_warning(WIRE, &omitted_advisories(request)) {
                sink.emit(warning)
                    .await
                    .map_err(|error| retain(error, &projection, target))?;
            }
            Ok(outcome)
        })
    }
}

/// A failure after dispatch keeps the last valid bound evidence, including the partial usage
/// of an interrupted stream.
///
/// Every caller of this is past `post_sse` returning, so the request was served whatever
/// marker the error arrived carrying. A sink refusing an event, an outcome refusing a model's
/// unpublished tool call and a projection refusal are all raised locally and default to
/// `not-sent`; reading that marker as proof nothing was dispatched — which a previous
/// revision did — throws the dispatch away and the evidence with it. The transport stamps
/// every error it raises, so nothing reaching here legitimately claims `not-sent`.
///
/// `Error::validate_for` re-validates whatever is attached here, and contradictory reported
/// counters are exactly how that fails — so a snapshot that cannot pass it travels not at
/// all. Losing the snapshot costs a reader the counters; attaching an invalid one would cost
/// them the whole failure.
fn retain(error: Error, projection: &StreamProjection, target: &Provenance) -> Error {
    let error = if error.dispatch == Dispatch::NotSent {
        error.with_dispatch(Dispatch::Accepted)
    } else {
        error
    };
    let observation = projection.observation();
    if observation.validate_for(target).is_err() {
        return error;
    }
    error.with_observation(observation)
}
