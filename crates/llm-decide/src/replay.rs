//! The candidate an offline replay sends a corpus to: each case as the call `llm-calls` or this
//! crate makes for its kind, on the candidate route.
//!
//! It lives here because `llm_calls::replay` cannot name the classifier and the judge: they are
//! in this crate, which depends on `llm-calls`.

use crate::{
    AbstentionCause, Ask, Classification, Classifier, JudgeError, LabelSet, ModelClassifier, Ports,
    Rubric, RubricError, judge,
};
use llm_calls::{CallIdentity, Candidate, CaseAsk, CaseCall, REPLAY_CLASSIFIER_ID, record_turn};
use llm_core::{BoxFuture, Cancel, Error, ErrorCode, Id, Item, StreamEvent, StreamSink};
use llm_routing::{Catalog, FallbackPolicy, Models, Pause, Selection};
use std::time::Duration;

/// A turn or a classification case makes one attempt: a failure is not retried, so a case costs
/// one call.
const ONE_ATTEMPT: FallbackPolicy = FallbackPolicy::disabled();

/// Answers each case of a replay with one routed, recorded call on the candidate route.
///
/// A turn case is its recorded request sent to the candidate route by
/// [`llm_calls::record_turn`]; its answer is the text of its assistant messages, in order, and
/// its record has the case's request id. A classification case is one [`ModelClassifier`] call
/// held to the label set's JSON schema; its answer is the label decided, and its record is
/// `<request id>/classify`. Neither retries a failure. A verdict case is one [`judge`] call; its
/// answer is the verdict's canonical JSON, and its record is `<request id>/<rubric id>`. An
/// answer that names no label of the set, or is no verdict the rubric's schema holds, is no
/// answer: the case differs. A call that failed fails the case with its error code, a
/// classification answered past its deadline with `deadline`, and a label set or rubric this
/// crate refuses with `invalid-request`, before any call.
#[derive(Clone, Copy)]
pub struct Routed<'a> {
    pub catalog: &'a Catalog,
    pub models: &'a dyn Models,
    pub cancel: &'a Cancel,
    pub pause: Pause<'a>,
    /// How long one classification call may take before it counts failed with `deadline`.
    pub classification_deadline: Duration,
}

struct Unseen;

impl StreamSink for Unseen {
    fn emit(&mut self, _: StreamEvent) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async { Ok(()) })
    }
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "the signature of the admission hook"
)]
fn admitted(_: &Selection<'_>) -> Result<(), Error> {
    Ok(())
}

impl Routed<'_> {
    fn ports<'a>(&'a self, call: &CaseCall<'a>) -> Ports<'a> {
        Ports {
            catalog: self.catalog,
            models: self.models,
            admit: &admitted,
            cancel: self.cancel,
            pause: self.pause,
            recording: call.recording,
        }
    }

    async fn turn(
        &self,
        call: CaseCall<'_>,
        request: &llm_core::TurnRequest,
    ) -> Result<Option<String>, Error> {
        let mut request = request.clone();
        call.route.clone_into(&mut request.model);
        let mut admit = admitted;
        let run = record_turn(
            self.catalog,
            &request,
            Some(call.case.input_tokens),
            ONE_ATTEMPT,
            llm_routing::Ports {
                models: self.models,
                admit: &mut admit,
                sink: &mut Unseen,
                cancel: self.cancel,
                pause: self.pause,
            },
            CallIdentity::primary(call.request_id.clone()),
            call.recording,
        )
        .await?;
        let outcome = run.result?;
        let text: String = outcome
            .items
            .iter()
            .filter_map(|item| match item {
                Item::AssistantText { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        Ok(Some(text))
    }

    async fn label(
        &self,
        call: CaseCall<'_>,
        label_set: &LabelSet,
        instructions: &str,
        input: &str,
    ) -> Result<Option<String>, Error> {
        let id =
            Id::new(REPLAY_CLASSIFIER_ID).map_err(|_| Error::invalid("invalid classifier id"))?;
        let classifier = ModelClassifier::json_schema(id, call.route)
            .with_instructions(instructions)
            .with_policy(ONE_ATTEMPT);
        let classification = classifier
            .classify(Ask {
                label_set,
                input,
                remaining: self.classification_deadline,
                request_id: call.request_id,
                input_tokens: Some(call.case.input_tokens),
                ports: self.ports(&call),
            })
            .await;
        match classification {
            Classification::Decided(decision) => Ok(Some(decision.label)),
            Classification::Abstained(abstention) => match abstention.cause {
                AbstentionCause::OffLabel => Ok(None),
                AbstentionCause::ProviderError(code) => {
                    Err(Error::new(code, "the classification call failed"))
                }
                AbstentionCause::Timeout => Err(Error::new(
                    ErrorCode::Deadline,
                    "the classification call passed its deadline",
                )),
            },
        }
    }
}

impl Candidate for Routed<'_> {
    fn answer<'a>(&'a self, call: CaseCall<'a>) -> BoxFuture<'a, Result<Option<String>, Error>> {
        Box::pin(async move {
            match &call.case.ask {
                CaseAsk::Turn { request } => self.turn(call, request).await,
                CaseAsk::Classification {
                    label_set_id,
                    label_set_version,
                    labels,
                    instructions,
                    input,
                } => {
                    let label_set = LabelSet::new(
                        label_set_id.clone(),
                        label_set_version.clone(),
                        labels.clone(),
                    )
                    .map_err(|error| Error::invalid(format!("label set: {}", error.code())))?;
                    self.label(call, &label_set, instructions, input).await
                }
                CaseAsk::Verdict {
                    rubric_id,
                    rubric_version,
                    rubric_text,
                    schema,
                    subject,
                } => {
                    let refused =
                        |error: &RubricError| Error::invalid(format!("rubric: {}", error.code()));
                    let rubric = Rubric::new(
                        rubric_id.clone(),
                        rubric_version.clone(),
                        rubric_text.clone(),
                        schema,
                        call.route,
                    )
                    .map_err(|error| refused(&error))?;
                    let ports = self.ports(&call);
                    let input_tokens = Some(call.case.input_tokens);
                    match judge(&rubric, subject, call.request_id, input_tokens, ports).await {
                        Ok(verdict) => Ok(verdict.verdict_json),
                        Err(JudgeError::Rubric(error)) => Err(refused(&error)),
                        Err(JudgeError::Call(error)) => Err(error),
                    }
                }
            }
        })
    }
}
