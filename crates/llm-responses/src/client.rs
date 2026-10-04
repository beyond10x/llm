//! One bound Responses endpoint, reached once per turn.

use std::{sync::Arc, time::Instant};

use llm_core::{
    BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, MAX_REQUEST_BYTES, Model,
    Provenance, StreamEvent, StreamSink, TurnObservation, TurnOutcome, TurnRequest,
};
use llm_credentials::SecretResolver;
use llm_http::{Framing, HeaderMap, HeaderName, HeaderValue, HttpClient, SseEvent};
use serde_json::Value;

use crate::{PROTOCOL, decode_stream, project_request};

/// The stream events after which this wire sends nothing the decoder reads.
///
/// Reading stops at the first of them rather than at end of body, so a server that keeps the
/// connection open after its terminal object does not hold the turn until the idle limit.
const FINAL_EVENTS: &[&str] = &[
    "response.completed",
    "response.incomplete",
    "response.failed",
    "error",
];

/// A single-attempt client for one bound Responses endpoint.
///
/// A turn projects the neutral request with [`project_request`], resolves the account's
/// credential through the injected resolver, sends one streaming `POST {base_url}responses`
/// through `llm-http`, and decodes what arrives with [`decode_stream`]. The request carries the
/// content headers and the authentication header the binding declares, and nothing else.
///
/// It never retries, never refreshes a credential, never changes account or endpoint, and never
/// falls back: those belong to routing and to the caller.
///
/// The decoder reads a whole stream, so the turn's [`StreamEvent`]s reach the sink once the
/// stream has ended, in wire order, and not while it is still arriving.
pub struct ResponsesClient {
    binding: llm_providers::Binding,
    projection: crate::Binding,
    http: HttpClient,
    resolver: Arc<dyn SecretResolver>,
}

impl ResponsesClient {
    /// Binds a client to one serving binding, one transport and one credential resolver.
    ///
    /// The whole turn, credential resolution included, is bounded by the transport's own
    /// `total` limit, taken once when the turn starts.
    ///
    /// # Errors
    /// Refuses a binding whose protocol is not Responses.
    pub fn new(
        binding: llm_providers::Binding,
        http: HttpClient,
        resolver: Arc<dyn SecretResolver>,
    ) -> Result<Self, Error> {
        if binding.provenance().protocol != PROTOCOL {
            return Err(Error::unsupported(
                "a Responses client requires a Responses binding",
            ));
        }
        let upstream = Id::new(binding.upstream_model())
            .map_err(|_| Error::invalid("the binding's upstream model is not an identifier"))?;
        let projection = crate::Binding::new(binding.provenance().clone(), upstream);
        Ok(Self {
            binding,
            projection,
            http,
            resolver,
        })
    }

    async fn run(
        &self,
        request: &TurnRequest,
        sink: &mut dyn StreamSink,
        cancel: &Cancel,
    ) -> Result<TurnOutcome, Error> {
        // Refused before any I/O, and before a credential is resolved: the neutral contract for
        // this binding, then the projection, then the bound the transport would refuse anyway.
        request.validate_for(self.binding.provenance(), self.binding.capabilities())?;
        let body = serde_json::to_vec(&project_request(&self.projection, request)?)
            .map_err(|_| Error::invalid("the projected request cannot be encoded"))?;
        if body.len() > MAX_REQUEST_BYTES {
            return Err(Error::too_large(
                "the projected Responses request exceeds its bound",
            ));
        }
        let deadline = Instant::now() + self.http.limits().total;
        // Resolved per request, so a rotated credential is used without restarting the caller.
        let auth = bounded(
            deadline,
            self.binding.prepare_auth(&*self.resolver, cancel),
            Dispatch::NotSent,
        )
        .await?;
        let (mut headers, _) = auth.into_parts();
        set(&mut headers, "content-type", "application/json");
        set(&mut headers, "accept", "text/event-stream");
        let payloads = self.exchange(headers, body, cancel, deadline).await?;
        let decoding = decode_stream(&self.projection, &payloads);
        // The whole stream is read before the first event is handed over, so the provider's
        // counters are already known: a sink refusal or a cancel during hand-over keeps them.
        let evidence = self.evidence(&decoding.result);
        for event in decoding.events {
            bounded(deadline, emit(sink, event, cancel), Dispatch::Accepted)
                .await
                .map_err(|error| keep(error, &evidence))?;
        }
        let outcome = decoding.result.map_err(|error| self.attach(error))?;
        // The decoder checks the wire; this checks the turn: a forced tool, a published tool,
        // caller-owned content. A refusal here comes after the provider accepted the request.
        outcome
            .validate_for(request, self.binding.provenance())
            .map_err(|error| keep(error.with_dispatch(Dispatch::Accepted), &evidence))?;
        Ok(outcome)
    }

    /// Sends the one request and reads its stream up to the first final event.
    async fn exchange(
        &self,
        headers: HeaderMap,
        body: Vec<u8>,
        cancel: &Cancel,
        deadline: Instant,
    ) -> Result<Vec<Value>, Error> {
        // This route sends no `[DONE]` sentinel: terminal truth is the response object.
        let mut stream = self
            .http
            .post_sse_until(
                self.binding.request_url(),
                headers,
                body,
                Framing::PayloadsOnly,
                cancel,
                Some(deadline),
            )
            .await
            .map_err(|error| self.attach(error))?;
        let mut payloads = Vec::new();
        // End of body ends the loop too. The decoder refuses unless a terminal object arrived.
        while let Some(SseEvent::Payload { data, .. }) =
            stream.next().await.map_err(|error| self.attach(error))?
        {
            let last = data
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| FINAL_EVENTS.contains(&kind));
            payloads.push(data);
            if last {
                break;
            }
        }
        Ok(payloads)
    }

    fn attach(&self, error: Error) -> Error {
        attach(error, self.binding.provenance())
    }

    /// The best evidence a decoded stream holds: the outcome's observation, else the
    /// refusal's, else the binding alone. Evidence this binding would refuse is not carried.
    fn evidence(&self, result: &Result<TurnOutcome, Error>) -> TurnObservation {
        let target = self.binding.provenance();
        let decoded = match result {
            Ok(outcome) => Some(&outcome.observation),
            Err(error) => error.observation.as_deref(),
        };
        decoded
            .filter(|observation| observation.validate_for(target).is_ok())
            .cloned()
            .unwrap_or_else(|| TurnObservation::new(target.clone()))
    }
}

/// Attaches the binding as the only evidence a failure before decoding has.
///
/// A failure that sent nothing carries no observation: there is nothing upstream to observe,
/// and the core contract refuses an unsent failure that claims otherwise.
fn attach(error: Error, binding: &Provenance) -> Error {
    keep(error, &TurnObservation::new(binding.clone()))
}

/// Attaches `evidence` to a failure after dispatch that carries none of its own.
fn keep(error: Error, evidence: &TurnObservation) -> Error {
    if error.observation.is_some() || error.dispatch == Dispatch::NotSent {
        return error;
    }
    error.with_observation(evidence.clone())
}

fn set(headers: &mut HeaderMap, name: &'static str, value: &'static str) {
    headers.insert(
        HeaderName::from_static(name),
        HeaderValue::from_static(value),
    );
}

/// Hands one event to the caller, unless the caller cancels first.
async fn emit(sink: &mut dyn StreamSink, event: StreamEvent, cancel: &Cancel) -> Result<(), Error> {
    tokio::select! {
        biased;
        () = cancel.cancelled() => Err(Error::cancelled().with_dispatch(Dispatch::Accepted)),
        sent = sink.emit(event) => sent.map_err(|error| error.with_dispatch(Dispatch::Accepted)),
    }
}

/// Runs one wait under the turn's absolute deadline.
async fn bounded<T>(
    deadline: Instant,
    work: impl Future<Output = Result<T, Error>>,
    dispatch: Dispatch,
) -> Result<T, Error> {
    tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), work)
        .await
        .unwrap_or_else(|_| {
            Err(
                Error::new(ErrorCode::Deadline, "the Responses turn deadline expired")
                    .with_dispatch(dispatch),
            )
        })
}

impl Model for ResponsesClient {
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
mod tests {
    use super::{Dispatch, Error, Id, Provenance, attach};
    use llm_core::Protocol;

    fn provenance() -> Provenance {
        let id = |value: &str| Id::new(value).expect("fixture identifier");
        Provenance {
            protocol: Protocol::Responses,
            provider: id("my-lab"),
            account: id("local"),
            endpoint: id("local-endpoint"),
            model: id("small"),
            binding_revision: id("rev-1"),
        }
    }

    /// Every transport refusal raised before the request left is `NotSent`; one carrying an
    /// observation is refused by `Error::validate_for`, which routing's fallback treats as an
    /// ambiguous dispatch.
    #[test]
    fn an_unsent_failure_is_returned_without_an_observation() {
        let error = attach(Error::too_large("over the bound"), &provenance());
        assert_eq!(error.dispatch, Dispatch::NotSent);
        assert!(error.observation.is_none(), "{error:?}");
        assert_eq!(error.validate_for(&provenance()), Ok(()));
    }

    #[test]
    fn a_failure_after_dispatch_carries_the_binding() {
        let error = attach(
            Error::protocol("cut off").with_dispatch(Dispatch::Accepted),
            &provenance(),
        );
        let observation = error
            .observation
            .as_deref()
            .expect("the binding is attached");
        assert_eq!(observation.binding, provenance());
        assert_eq!(error.validate_for(&provenance()), Ok(()));
    }
}
