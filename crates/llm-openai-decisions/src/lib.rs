#![forbid(unsafe_code)]

//! The `OpenAI` decision endpoint: `POST /v1/decisions`.
//!
//! A decision request asks 1 to 400 typed questions ([`Question`]: predicate, choice, score)
//! about one input and gets one typed answer per question ([`DecisionAnswer`], or a refusal),
//! with probabilities and token usage. It is not a turn, so it is not a `Model`: the caller
//! selects a route target ([`OpenAiDecisions::select`]) and [`OpenAiDecisions`] sends the request
//! there over `llm-http` with the credential the target's account declares.
//! [`OpenAiDecisions::decide_recorded`] writes one [`llm_calls::CallRecord`] of kind
//! [`Kind::Classification`] per call, priced under the caller's price book. Nothing is retried
//! or fallen back.
//!
//! [`OpenAiDecisions`] is also the classifier's third model path: it implements
//! [`llm_decide::DecisionEndpoint`], which `llm_decide::EndpointClassifier` asks one choice
//! question whose choices are the label set.
//!
//! The wire is read from `openai/openai-openapi` at commit
//! `b4cad10d7cff7fc286f2dbf1992569989522c6f0` (path `/decisions`, operation `createDecision`).
//! A request sends only the reference's members; [`DecisionRequest::from_json`] refuses any
//! other by its path. An answer body is read whole from the transport, then as one JSON
//! document. A body the transport reports as cut (short of the length it declared, or a chunked
//! body without its end) is the transport's own failure: `transport`, dispatch `accepted`,
//! retriable, as for every client over `llm-http`. A body that arrived whole by the transport's
//! account and is not one JSON document is refused `protocol`, dispatch `accepted`: a document
//! that ends early (so also a body delimited only by the connection's end and cut there), bytes
//! after the document, a member given twice or a nesting deeper than [`MAX_DEPTH`]; and so is an
//! answer type the reference does not name, by its path. A usage counter the body does not
//! report stays unknown, never zero.

mod request;
mod response;
mod strict;

pub use request::{
    ChoiceOption, ChoiceValue, DecisionRequest, MAX_CHOICES, MAX_INPUT_CHARS, MAX_LEVELS,
    MAX_NAMED_MEMBER_BYTES, MAX_PROMPT_CACHE_KEY_CHARS, MAX_QUESTIONS, MAX_SAFETY_IDENTIFIER_CHARS,
    MAX_TEXT_CHARS, MIN_CHOICES, MIN_LEVELS, PromptCacheMode, PromptCacheOptions, PromptCacheTtl,
    Question, QuestionKind, ScoreLevel,
};
pub use response::{
    AnswerKind, ChoiceProbability, DecisionAnswer, DecisionResponse, ScoreProbability,
};
pub use strict::MAX_DEPTH;

use llm_calls::{CallFacts, CallIdentity, CallRecord, Cost, Kind, Outcome, Recording, Stopwatch};
use llm_core::{
    BoxFuture, Cancel, Dispatch, Error, ErrorCode, Id, MAX_REQUEST_BYTES, Protocol, TurnRequest,
    Usage,
};
use llm_cost::{AttemptUsage, PriceBook};
use llm_credentials::SecretResolver;
use llm_decide::{ChoiceAnswer, ChoiceCall, DecisionEndpoint, LabelProbability};
use llm_http::{HeaderMap, HeaderName, HeaderValue, HttpClient};
use llm_providers::Binding;
use llm_routing::{Catalog, Selection};
use response::Read;
use std::sync::Arc;

/// The media type a decision answer must name.
const JSON: &str = "application/json";

/// The decision endpoint client: an HTTP client and the resolver of each account's credential.
/// It holds no target; every call names the route target it goes to.
#[derive(Clone)]
pub struct OpenAiDecisions {
    http: HttpClient,
    resolver: Arc<dyn SecretResolver>,
}

impl std::fmt::Debug for OpenAiDecisions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiDecisions").finish_non_exhaustive()
    }
}

impl OpenAiDecisions {
    /// `resolver` resolves the target account's credential at each call; an anonymous account
    /// never asks it (pass `AnonymousResolver`).
    pub fn new(http: HttpClient, resolver: Arc<dyn SecretResolver>) -> Self {
        Self { http, resolver }
    }

    /// Selects the target a request is sent to, without any I/O: the first compatible target of
    /// the turn route `request.model` names, admitted against the caller's `input_tokens`
    /// estimate as routing admits a turn with no tools and no controls.
    ///
    /// # Errors
    /// As [`Catalog::resolve`]: an invalid or unknown alias (`invalid-request`), an embed route
    /// (`unsupported`), no compatible target.
    pub fn select<'c>(
        catalog: &'c Catalog,
        request: &DecisionRequest,
        input_tokens: Option<u64>,
    ) -> Result<Selection<'c>, Error> {
        catalog.resolve(
            &TurnRequest::new(request.model.clone(), Vec::new()),
            input_tokens,
        )
    }

    /// Sends `request` to `selection`'s target and returns its answers, one per question in
    /// question order, with the model and the usage the body reported. One attempt: nothing is
    /// retried or fallen back. The transport's limits and `cancel` bound the call.
    ///
    /// # Errors
    /// Before any I/O: a request outside the reference's bounds (`invalid-request`, see
    /// [`DecisionRequest::validate`]), a target served over another protocol than responses or
    /// chat-completions (`unsupported`), an encoded body over the transport's request bound of
    /// 16 MiB (`too-large`), a cancelled call (`cancelled`). Then the credential's and the
    /// transport's own errors, unchanged: among them `transport`, dispatch `accepted`,
    /// retriable, for a body cut short of its declared length or of its last chunk. And
    /// `protocol`, dispatch `accepted`, for a body the transport delivered whole that is not one
    /// whole JSON document or does not hold exactly one answer per question, each of its
    /// question's type or a refusal and carrying its question's name; the message names the
    /// path of the member refused and never a value.
    pub async fn decide(
        &self,
        selection: &Selection<'_>,
        request: &DecisionRequest,
        cancel: &Cancel,
    ) -> Result<DecisionResponse, Error> {
        self.run(selection, request, cancel, None).await
    }

    /// [`OpenAiDecisions::decide`], writing one record of kind [`Kind::Classification`] with
    /// `identity` to `recording`'s sink: the route, the target, the model the body named, one
    /// attempt, the usage as read and, under a price book, its cost. A call refused before its
    /// credential is resolved (the errors before any I/O above) writes none.
    ///
    /// # Errors
    /// As [`OpenAiDecisions::decide`].
    pub async fn decide_recorded(
        &self,
        selection: &Selection<'_>,
        request: &DecisionRequest,
        cancel: &Cancel,
        identity: CallIdentity,
        recording: Recording<'_>,
    ) -> Result<DecisionResponse, Error> {
        self.run(selection, request, cancel, Some((identity, recording)))
            .await
    }

    async fn run(
        &self,
        selection: &Selection<'_>,
        request: &DecisionRequest,
        cancel: &Cancel,
        recording: Option<(CallIdentity, Recording<'_>)>,
    ) -> Result<DecisionResponse, Error> {
        request.validate()?;
        let binding = selection.binding;
        if !matches!(
            binding.provenance().protocol,
            Protocol::Responses | Protocol::ChatCompletions
        ) {
            return Err(Error::unsupported(
                "the OpenAI decisions wire is served on a responses or chat-completions endpoint",
            ));
        }
        let body = serde_json::to_vec(&request.wire(binding.upstream_model()))
            .map_err(|_| Error::invalid("decision request cannot be encoded"))?;
        if body.len() > MAX_REQUEST_BYTES {
            return Err(Error::too_large(
                "decision request body exceeds the transport's request bound",
            ));
        }
        if cancel.is_cancelled() {
            return Err(Error::cancelled());
        }
        let stopwatch = recording
            .as_ref()
            .map(|(_, recording)| Stopwatch::start(recording.clock));
        let answered = self.send(binding, body, &request.questions, cancel).await;
        if let (Some((identity, recording)), Some(stopwatch)) = (recording, stopwatch) {
            let timing = stopwatch.stop(recording.clock);
            recording.sink.record(record(
                selection,
                identity,
                timing,
                &answered,
                recording.prices,
            ));
        }
        let Read {
            result,
            model,
            usage,
            ..
        } = answered;
        let answers = result?;
        // An answer is read only from a body that names its model.
        let model = model.ok_or_else(|| {
            Error::protocol("decision answer names no model").with_dispatch(Dispatch::Accepted)
        })?;
        Ok(DecisionResponse {
            model,
            answers,
            usage,
        })
    }

    /// Sends the body once and reads its whole answer.
    async fn send(
        &self,
        binding: &Binding,
        body: Vec<u8>,
        questions: &[Question],
        cancel: &Cancel,
    ) -> Read {
        let failed = |error: Error| Read {
            result: Err(error),
            model: None,
            usage: Usage::default(),
            whole: false,
        };
        let headers = match self.headers(binding, cancel).await {
            Ok(headers) => headers,
            Err(error) => return failed(error),
        };
        let url = format!(
            "{}decisions",
            binding.declaration().endpoint.base_url.as_str()
        );
        let mut stream = match self
            .http
            .post_stream_until(&url, headers, body, JSON, cancel, None)
            .await
        {
            Ok(stream) => stream,
            Err(error) => return failed(error),
        };
        // The transport bounds the body; nothing is read from it before it has ended.
        let mut answer = Vec::new();
        loop {
            match stream.next().await {
                Ok(Some(chunk)) => answer.extend_from_slice(&chunk),
                Ok(None) => break,
                Err(error) => return failed(error),
            }
        }
        response::read(&answer, questions)
    }

    /// The call's headers. Resolves the account's credential once.
    async fn headers(&self, binding: &Binding, cancel: &Cancel) -> Result<HeaderMap, Error> {
        let (mut headers, _) = binding
            .prepare_auth(self.resolver.as_ref(), cancel)
            .await?
            .into_parts();
        headers.insert(
            HeaderName::from_static("content-type"),
            HeaderValue::from_static(JSON),
        );
        headers.insert(
            HeaderName::from_static("accept"),
            HeaderValue::from_static(JSON),
        );
        Ok(headers)
    }
}

/// The record of one call that reached its credential.
fn record(
    selection: &Selection<'_>,
    identity: CallIdentity,
    timing: llm_calls::Timing,
    answered: &Read,
    prices: Option<&PriceBook>,
) -> CallRecord {
    let (outcome, error_code, dispatch) = match &answered.result {
        Ok(_) => (Outcome::Succeeded, None, Dispatch::Accepted),
        Err(error) => (
            if error.code == ErrorCode::Cancelled {
                Outcome::Cancelled
            } else {
                Outcome::Failed
            },
            Some(error.code),
            error.dispatch,
        ),
    };
    let upstream_model = answered
        .model
        .as_deref()
        .and_then(|model| Id::new(model).ok());
    // A call that was not sent cost nothing.
    let cost = prices
        .filter(|_| dispatch != Dispatch::NotSent)
        .and_then(|book| {
            let attempt = attempt(
                selection,
                &identity.id,
                answered,
                upstream_model.clone(),
                dispatch,
                book,
            );
            Cost::of_attempts(book, [attempt])
        });
    CallRecord::new(
        identity,
        Kind::Classification,
        selection.target.route_id.clone(),
        timing,
        CallFacts {
            route_target_id: Some(selection.target.id.clone()),
            upstream_model,
            outcome,
            error_code,
            dispatch: answered.result.as_ref().err().map(|_| dispatch),
            attempts: 1,
            usage: answered.usage.clone(),
            cost,
        },
    )
}

/// The call as `llm-cost` prices it, under the price the book declares for the serving model and
/// binding revision (the one naming the reported model first). A counter the body did not report
/// stays unknown here too, so its line is unknown and the cost incomplete.
fn attempt(
    selection: &Selection<'_>,
    id: &Id,
    answered: &Read,
    upstream_model: Option<Id>,
    dispatch: Dispatch,
    prices: &PriceBook,
) -> AttemptUsage {
    let binding = selection.binding;
    let provenance = binding.provenance();
    let serving_model_id = &selection.target.serving_model_id;
    let declared = || {
        prices.document().models.iter().filter(|price| {
            &price.serving_model_id == serving_model_id
                && price.binding_revision == provenance.binding_revision
        })
    };
    let price_id = declared()
        .find(|price| upstream_model.as_ref() == Some(&price.upstream_model))
        .or_else(|| declared().next())
        .map_or_else(|| serving_model_id.clone(), |price| price.id.clone());
    AttemptUsage {
        id: id.clone(),
        usage_record_id: id.clone(),
        request_id: id.clone(),
        route_target_id: selection.target.id.clone(),
        serving_model_id: serving_model_id.clone(),
        price_id,
        binding: provenance.clone(),
        upstream_model,
        billing: binding.declaration().account.billing_kind,
        dispatch,
        failed: answered.result.is_err(),
        // Counters are read only from a body read whole, so they are the call's final counts.
        final_usage: answered.whole,
        usage: answered.usage.clone(),
    }
}

impl DecisionEndpoint for OpenAiDecisions {
    /// One choice question, unnamed, whose choices are `call.choices` as text values in their
    /// order, sent through [`OpenAiDecisions::decide_recorded`].
    fn choose<'a>(&'a self, call: ChoiceCall<'a>) -> BoxFuture<'a, Result<ChoiceAnswer, Error>> {
        Box::pin(async move {
            let question = Question::Choice {
                name: None,
                instructions: call.instructions.to_owned(),
                choices: call.choices.iter().map(ChoiceOption::text).collect(),
            };
            let request = DecisionRequest::new(call.route, call.input, vec![question]);
            let response = self
                .run(
                    call.selection,
                    &request,
                    call.cancel,
                    Some((call.identity, call.recording)),
                )
                .await?;
            Ok(match response.answers.into_iter().next() {
                Some(DecisionAnswer::Choice {
                    choice: ChoiceValue::Text(choice),
                    probabilities,
                    ..
                }) => probabilities
                    .into_iter()
                    .map(|weighed| match weighed.value {
                        ChoiceValue::Text(label) => Some(LabelProbability {
                            label,
                            probability: weighed.probability,
                        }),
                        ChoiceValue::Flag(_) => None,
                    })
                    .collect::<Option<Vec<_>>>()
                    .map_or(ChoiceAnswer::NotText, |probabilities| {
                        ChoiceAnswer::Chosen {
                            choice,
                            probabilities,
                        }
                    }),
                Some(DecisionAnswer::Choice { .. }) => ChoiceAnswer::NotText,
                Some(DecisionAnswer::Refusal { .. }) => ChoiceAnswer::Refused,
                // The body was held to one answer of the question's type or a refusal, so no
                // other answer reaches here; it is refused, not read as either.
                _ => {
                    return Err(Error::protocol(
                        "decision answer is not a choice or a refusal: answers[0]",
                    )
                    .with_dispatch(Dispatch::Accepted));
                }
            })
        })
    }
}
