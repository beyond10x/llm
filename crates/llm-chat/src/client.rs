//! A single-attempt Chat Completions client over the shared bounded HTTP/SSE transport.
use crate::{incoming::StreamProjection, outgoing::project_request};
use http::{
    HeaderValue,
    header::{ACCEPT, CONTENT_TYPE},
};
use llm_core::{
    BoxFuture, Cancel, Capabilities, Dispatch, Error, Model, Provenance, StreamSink, TurnOutcome,
    TurnRequest,
};
use llm_credentials::SecretResolver;
use llm_http::{Framing, HttpClient};
use llm_providers::Binding;
use std::sync::Arc;

/// One immutable binding served over Chat Completions. Retries, alternates and fallback
/// belong to routing; this client sends exactly once.
pub struct ChatClient {
    binding: Binding,
    http: HttpClient,
    resolver: Arc<dyn SecretResolver>,
}

impl ChatClient {
    pub fn new(binding: Binding, http: HttpClient, resolver: Arc<dyn SecretResolver>) -> Self {
        Self {
            binding,
            http,
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
            let target = self.binding.provenance();
            let body = project_request(request, &self.binding, true)?;
            let body = serde_json::to_vec(&body)
                .map_err(|_| Error::invalid("chat request could not be encoded"))?;
            let (mut headers, _generation) = self
                .binding
                .prepare_auth(self.resolver.as_ref(), cancel)
                .await?
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
                    Err(error) => return Err(retain(error, &projection, target)),
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
