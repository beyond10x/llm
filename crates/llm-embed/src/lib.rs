#![forbid(unsafe_code)]

//! Embeddings over an embed route: the `OpenAI` `POST /v1/embeddings` wire.
//!
//! [`OpenAiEmbedder`] serves an embed route's first target ([`Catalog::resolve_embed`]) as an
//! [`Embedder`]: texts in, one vector per text out, in input order, with input-token usage, within
//! the request's timeout. A request larger than the serving model's declared
//! `embed_batch_limit` is sent as ordered batches, one after the other; the answer of each is
//! reassembled by its `index`. [`OpenAiEmbedder::embed_recorded`] writes one
//! [`llm_calls::CallRecord`] of kind [`Kind::EmbedBatch`] per batch sent. Nothing is retried or
//! fallen back: a failed batch ends the request with its own code, dispatch and retry class.

use llm_calls::{CallFacts, CallIdentity, CallRecord, Cost, Kind, Outcome, Recording, Stopwatch};
use llm_core::{
    BoxFuture, Cancel, Dispatch, EmbedRequest, Embedder, Embeddings, Error, ErrorCode, Id,
    MAX_EMBED_DIMENSIONS, Protocol, Usage,
};
use llm_cost::AttemptUsage;
use llm_credentials::SecretResolver;
use llm_http::{HeaderMap, HeaderValue, HttpClient};
use llm_providers::Binding;
use llm_routing::Catalog;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::time::Instant;

/// The media type an embeddings answer must name.
const JSON: &str = "application/json";

/// The embed route's first target, bound to its endpoint, credential and HTTP client.
pub struct OpenAiEmbedder {
    binding: Binding,
    route_id: Id,
    target_id: Id,
    serving_model_id: Id,
    url: String,
    http: HttpClient,
    resolver: Arc<dyn SecretResolver>,
}

impl std::fmt::Debug for OpenAiEmbedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiEmbedder")
            .field("route_id", &self.route_id)
            .field("target_id", &self.target_id)
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

impl OpenAiEmbedder {
    /// Binds the embed route `alias` names. `resolver` resolves the account's credential at
    /// each batch; an anonymous account never asks it (pass `AnonymousResolver`).
    ///
    /// # Errors
    /// Refuses, before any I/O: an invalid or unknown alias (`invalid-request`), a turn route
    /// (`unsupported`), and a target served over Messages, which has no embeddings endpoint
    /// (`unsupported`).
    pub fn for_route(
        catalog: &Catalog,
        alias: &str,
        http: HttpClient,
        resolver: Arc<dyn SecretResolver>,
    ) -> Result<Self, Error> {
        let selection = catalog.resolve_embed(alias)?;
        let binding = selection.binding.clone();
        if !matches!(
            binding.provenance().protocol,
            Protocol::Responses | Protocol::ChatCompletions
        ) {
            return Err(Error::unsupported(
                "the OpenAI embeddings wire is served on a responses or chat-completions endpoint",
            ));
        }
        let url = format!(
            "{}embeddings",
            binding.declaration().endpoint.base_url.as_str()
        );
        Ok(Self {
            route_id: selection.route_id.clone(),
            target_id: selection.target.id.clone(),
            serving_model_id: selection.target.serving_model_id.clone(),
            binding,
            url,
            http,
            resolver,
        })
    }

    /// The most texts one request carries: the serving model's declaration, else every text.
    pub fn batch_limit(&self) -> Option<usize> {
        self.binding
            .capabilities()
            .embed_batch_limit
            .and_then(|limit| usize::try_from(limit).ok())
    }

    /// [`Embedder::embed`], writing one record per batch sent to `recording`'s sink, with the id
    /// `<identity id>/<batch position>`, timed by its clock and priced under its price book. An
    /// invalid request refused before any batch writes none; so does an identity id too long for
    /// its longest batch record id, refused `invalid-request` and not sent.
    ///
    /// # Errors
    /// As [`Embedder::embed`].
    pub async fn embed_recorded(
        &self,
        request: &EmbedRequest,
        cancel: &Cancel,
        identity: CallIdentity,
        recording: Recording<'_>,
    ) -> Result<Embeddings, Error> {
        self.run(request, cancel, Some((&identity, recording)))
            .await
    }

    async fn run(
        &self,
        request: &EmbedRequest,
        cancel: &Cancel,
        recording: Option<(&CallIdentity, Recording<'_>)>,
    ) -> Result<Embeddings, Error> {
        request.validate()?;
        let limit = self.batch_limit().unwrap_or(request.texts.len()).max(1);
        // Every record id is known before the first batch, so none can be shared.
        let record_ids = recording
            .map(|(identity, _)| batch_record_ids(identity, request.texts.len().div_ceil(limit)))
            .transpose()?;
        if cancel.is_cancelled() {
            return Err(Error::cancelled());
        }
        let deadline = Instant::now() + request.timeout;
        let mut vectors = Vec::with_capacity(request.texts.len());
        let mut input_tokens = Some(0_u64);
        let mut upstream_model: Option<Id> = None;
        let mut width = request
            .dimensions
            .and_then(|dimensions| usize::try_from(dimensions).ok());
        for (position, texts) in request.texts.chunks(limit).enumerate() {
            let stopwatch = recording.map(|(_, recording)| Stopwatch::start(recording.clock));
            let sent = AtomicBool::new(false);
            let batch = Batch {
                texts,
                dimensions: request.dimensions,
                width,
            };
            let mut answered =
                match tokio::time::timeout_at(deadline, self.send(&batch, cancel, &sent)).await {
                    Ok(answered) => answered,
                    Err(_) => Answered::failed(past_timeout(sent.load(Ordering::SeqCst))),
                };
            // Vectors of two models are not comparable: one request is one model's.
            if let (Ok(_), Some(first), Some(this)) =
                (&answered.result, &upstream_model, &answered.upstream_model)
                && first != this
            {
                answered.result = Err(refused(
                    "batches of one embed request were answered by different models",
                ));
            }
            if let (Some((identity, recording)), Some(stopwatch), Some(id)) = (
                recording,
                stopwatch,
                record_ids.as_ref().and_then(|ids| ids.get(position)),
            ) {
                let timing = stopwatch.stop(recording.clock);
                recording.sink.record(self.record(
                    identity,
                    id.clone(),
                    timing,
                    &answered,
                    recording.prices,
                ));
            }
            let batch_vectors = answered.result?;
            if upstream_model.is_none() {
                upstream_model = answered.upstream_model;
            }
            width = width.or_else(|| batch_vectors.first().map(Vec::len));
            input_tokens = input_tokens
                .zip(answered.usage.input_tokens)
                .and_then(|(total, batch)| total.checked_add(batch));
            vectors.extend(batch_vectors);
        }
        Ok(Embeddings {
            vectors,
            usage: Usage {
                input_tokens,
                ..Usage::default()
            },
            upstream_model,
        })
    }

    /// Sends one batch and reads its whole answer; `sent` is set once the request may be on the
    /// wire.
    async fn send(&self, batch: &Batch<'_>, cancel: &Cancel, sent: &AtomicBool) -> Answered {
        let request = match self.request(batch, cancel).await {
            Ok(request) => request,
            Err(error) => return Answered::failed(error),
        };
        sent.store(true, Ordering::SeqCst);
        let (headers, body) = request;
        let mut stream = match self
            .http
            .post_stream_until(&self.url, headers, body, JSON, cancel, None)
            .await
        {
            Ok(stream) => stream,
            Err(error) => return Answered::failed(error),
        };
        let mut answer = Vec::new();
        loop {
            match stream.next().await {
                Ok(Some(chunk)) => answer.extend_from_slice(&chunk),
                Ok(None) => break,
                Err(error) => return Answered::failed(error),
            }
        }
        read_answer(&answer, batch)
    }

    /// The batch's headers and body. Resolves the account's credential once.
    async fn request(
        &self,
        batch: &Batch<'_>,
        cancel: &Cancel,
    ) -> Result<(HeaderMap, Vec<u8>), Error> {
        let (mut headers, _) = self
            .binding
            .prepare_auth(self.resolver.as_ref(), cancel)
            .await?
            .into_parts();
        headers.insert(
            llm_http::HeaderName::from_static("content-type"),
            HeaderValue::from_static(JSON),
        );
        headers.insert(
            llm_http::HeaderName::from_static("accept"),
            HeaderValue::from_static(JSON),
        );
        let body = serde_json::to_vec(&WireRequest {
            model: self.binding.upstream_model(),
            input: batch.texts,
            encoding_format: "float",
            dimensions: batch.dimensions,
        })
        .map_err(|_| Error::invalid("embed request cannot be encoded"))?;
        Ok((headers, body))
    }

    fn record(
        &self,
        identity: &CallIdentity,
        id: Id,
        timing: llm_calls::Timing,
        answered: &Answered,
        prices: Option<&llm_cost::PriceBook>,
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
        // A batch that was not sent and reported nothing cost nothing.
        let cost = prices
            .filter(|_| dispatch != Dispatch::NotSent)
            .and_then(|book| {
                Cost::of_attempts(book, [self.attempt(&id, answered, dispatch, book)])
            });
        CallRecord::new(
            CallIdentity {
                id,
                role: identity.role,
                primary_record_id: identity.primary_record_id.clone(),
            },
            Kind::EmbedBatch,
            self.route_id.clone(),
            timing,
            CallFacts {
                route_target_id: Some(self.target_id.clone()),
                upstream_model: answered.upstream_model.clone(),
                outcome,
                error_code,
                dispatch: answered.result.as_ref().err().map(|_| dispatch),
                attempts: 1,
                usage: answered.usage.clone(),
                cost,
            },
        )
    }

    /// The batch as `llm-cost` prices it, under the price the book declares for the serving
    /// model and binding revision (the one naming the reported model first).
    fn attempt(
        &self,
        id: &Id,
        answered: &Answered,
        dispatch: Dispatch,
        prices: &llm_cost::PriceBook,
    ) -> AttemptUsage {
        let provenance = self.binding.provenance();
        let declared = || {
            prices.document().models.iter().filter(|price| {
                price.serving_model_id == self.serving_model_id
                    && price.binding_revision == provenance.binding_revision
            })
        };
        let price_id = declared()
            .find(|price| answered.upstream_model.as_ref() == Some(&price.upstream_model))
            .or_else(|| declared().next())
            .map_or_else(|| self.serving_model_id.clone(), |price| price.id.clone());
        AttemptUsage {
            id: id.clone(),
            usage_record_id: id.clone(),
            request_id: id.clone(),
            route_target_id: self.target_id.clone(),
            serving_model_id: self.serving_model_id.clone(),
            price_id,
            binding: provenance.clone(),
            upstream_model: answered.upstream_model.clone(),
            billing: self.binding.declaration().account.billing_kind,
            dispatch,
            failed: answered.result.is_err(),
            // A count is read only from an answer read whole, so it is that batch's final count.
            final_usage: answered.usage.input_tokens.is_some(),
            // The embeddings wire has no output and no cache classes: they are known zeros for
            // pricing, never unknown, while the record keeps only what the answer reported.
            usage: Usage {
                input_tokens: answered.usage.input_tokens,
                output_tokens: Some(0),
                cached_input_tokens: Some(0),
                cache_creation_input_tokens: Some(0),
                reasoning_output_tokens: None,
            },
        }
    }
}

impl Embedder for OpenAiEmbedder {
    fn embed<'a>(
        &'a self,
        request: &'a EmbedRequest,
        cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<Embeddings, Error>> {
        Box::pin(self.run(request, cancel, None))
    }
}

/// One batch: its texts, the dimensions asked for, and the vector length every vector of the
/// request must have, once known.
struct Batch<'b> {
    texts: &'b [String],
    dimensions: Option<u32>,
    width: Option<usize>,
}

/// What one batch returned: its vectors in batch order, or the failure that ended it, and
/// whatever usage and model the answer reported either way.
struct Answered {
    result: Result<Vec<Vec<f32>>, Error>,
    usage: Usage,
    upstream_model: Option<Id>,
}

impl Answered {
    fn failed(error: Error) -> Self {
        Self {
            result: Err(error),
            usage: Usage::default(),
            upstream_model: None,
        }
    }
}

/// Every batch's record id, `<request id>/<position>`. When the longest does not fit an [`Id`] the
/// request is refused as a whole, `invalid-request` and not sent, before any batch: two batches never
/// share one record id. The message is fixed.
fn batch_record_ids(identity: &CallIdentity, batches: usize) -> Result<Vec<Id>, Error> {
    (0..batches)
        .map(|position| {
            Id::new(format!("{}/{position}", identity.id.as_str())).map_err(|_| {
                Error::invalid("the request id leaves no room for its batch record ids")
            })
        })
        .collect()
}

/// The caller's timeout passed. Dispatch is `unknown` once the batch may have been on the wire.
fn past_timeout(sent: bool) -> Error {
    Error::new(ErrorCode::Deadline, "the embed request passed its timeout").with_dispatch(if sent {
        Dispatch::Unknown
    } else {
        Dispatch::NotSent
    })
}

/// An answer the wire cannot use: `protocol`, dispatch `accepted`, final. The message is fixed.
fn refused(message: &'static str) -> Error {
    Error::protocol(message).with_dispatch(Dispatch::Accepted)
}

#[derive(Serialize)]
struct WireRequest<'r> {
    model: &'r str,
    input: &'r [String],
    encoding_format: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    dimensions: Option<u32>,
}

/// The vectors of an answer; its `model` and `usage` are read beside it ([`read_answer`]), and
/// every other member is left alone.
#[derive(Deserialize)]
struct WireAnswer {
    data: Vec<WireEmbedding>,
}

#[derive(Deserialize)]
struct WireEmbedding {
    embedding: Vec<f64>,
    index: u64,
}

/// Decodes one answer and holds it to its batch: exactly one vector per text, each index of the
/// batch once, every vector non-empty, finite as `f32`, and of the request's one length.
fn read_answer(answer: &[u8], batch: &Batch<'_>) -> Answered {
    // The usage and model are read apart from the vectors, so an answer refused for its vectors
    // still records the tokens the far side reported for it.
    let reported: Option<Value> = serde_json::from_slice(answer).ok();
    let usage = Usage {
        input_tokens: reported
            .as_ref()
            .and_then(|value| value.get("usage")?.get("prompt_tokens")?.as_u64()),
        ..Usage::default()
    };
    let upstream_model = reported
        .as_ref()
        .and_then(|value| value.get("model")?.as_str())
        .and_then(|model| Id::new(model).ok());
    let result = reported
        .ok_or_else(|| refused("embeddings answer is not JSON"))
        .and_then(|value| {
            serde_json::from_value::<WireAnswer>(value)
                .map_err(|_| refused("embeddings answer is not the document expected"))
        })
        .and_then(|wire| vectors(wire, batch));
    Answered {
        result,
        usage,
        upstream_model,
    }
}

fn vectors(wire: WireAnswer, batch: &Batch<'_>) -> Result<Vec<Vec<f32>>, Error> {
    if wire.data.len() != batch.texts.len() {
        return Err(refused(
            "embeddings answer does not hold one vector per text of its batch",
        ));
    }
    let mut slots: Vec<Option<Vec<f32>>> = vec![None; batch.texts.len()];
    let mut width = batch.width;
    for item in wire.data {
        let slot = usize::try_from(item.index)
            .ok()
            .and_then(|index| slots.get_mut(index))
            .ok_or_else(|| refused("embeddings answer names an index outside its batch"))?;
        if slot.is_some() {
            return Err(refused("embeddings answer names one index twice"));
        }
        if item.embedding.is_empty() || item.embedding.len() > MAX_EMBED_DIMENSIONS {
            return Err(refused(
                "embeddings answer holds an empty or oversized vector",
            ));
        }
        if width.is_some_and(|width| width != item.embedding.len()) {
            return Err(refused(
                "embeddings answer holds vectors of another length than the request's",
            ));
        }
        width = Some(item.embedding.len());
        let vector = item
            .embedding
            .into_iter()
            .map(|component| {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "an embedding component is an f32 the wire wrote in decimal"
                )]
                let narrowed = component as f32;
                narrowed
                    .is_finite()
                    .then_some(narrowed)
                    .ok_or_else(|| refused("embeddings answer holds a component past f32"))
            })
            .collect::<Result<Vec<f32>, Error>>()?;
        *slot = Some(vector);
    }
    // Every slot is filled: as many items as slots, none twice.
    Ok(slots.into_iter().flatten().collect())
}
