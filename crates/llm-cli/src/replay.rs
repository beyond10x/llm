//! The candidate `llm replay` sends a corpus to: each case as the calls `llm-calls` and
//! `llm-decide` make for its kind, on the candidate route.
//!
//! It lives here because `llm_calls::replay` cannot name the classifier and the judge: they are
//! in `llm-decide`, which depends on `llm-calls`.

use llm_calls::{CallIdentity, Candidate, CaseAsk, CaseCall, record_turn};
use llm_core::{BoxFuture, Cancel, Error, ErrorCode, Id, Item, StreamEvent, StreamSink};
use llm_decide::{
    AbstentionCause, Ask, Classification, Classifier, JudgeError, LabelSet, ModelClassifier, Ports,
    Rubric, judge,
};
use llm_routing::{Catalog, FallbackPolicy, Models, Pause, Selection};
use std::time::Duration;

/// The id of the classifier a classification case is asked through: its call's record is
/// `<request id>/classify`.
pub const CLASSIFIER_ID: &str = "classify";

/// A turn or a classification case makes one attempt: a failure is not retried, so a case costs
/// one call.
const ONE_ATTEMPT: FallbackPolicy = FallbackPolicy::disabled();

/// Answers each case with one routed, recorded call on the candidate route.
///
/// A turn case is its recorded request sent to the candidate route by
/// [`llm_calls::record_turn`]; its answer is the text of its assistant messages, in order. A
/// classification case is one [`ModelClassifier`] call held to the label set's JSON schema; its
/// answer is the label decided. Neither retries a failure. A verdict case is one
/// [`llm_decide::judge`] call; its answer is the verdict's canonical JSON. An answer that names
/// no label of the set, or is no verdict the rubric's schema holds, is no answer: the case
/// differs. A call that failed fails the case with its error code, a classification answered
/// past its deadline with `deadline`, and a label set or rubric `llm-decide` refuses with
/// `invalid-request`, before any call.
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
        let id = Id::new(CLASSIFIER_ID).map_err(|_| Error::invalid("invalid classifier id"))?;
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
                    let refused = |error: &llm_decide::RubricError| {
                        Error::invalid(format!("rubric: {}", error.code()))
                    };
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
                        Err(_) => Err(Error::invalid("the verdict call was refused")),
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    //! `Routed` under the real `llm_calls::replay`, `ModelClassifier` and `judge`, over one
    //! scripted model, a clock the model moves and an in-memory record sink. No network.

    use super::*;
    use llm_calls::{CallLog, CaseResult, Clock, Corpus, Recording, Replay, ReplayRun, replay};
    use llm_core::{
        Capabilities, Dispatch, Model, Provenance, StopReason, TurnObservation, TurnOutcome,
        TurnRequest, Usage,
    };
    use llm_cost::{Amount, Currency, ModelPrice, PriceBook, PriceDocument, Rate};
    use llm_routing::{CatalogDocument, Route, RouteKind, RouteTarget};
    use serde_json::{Value, json};
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    const EXAMPLE: &str = include_str!("../../../examples/catalog.toml");
    const DEADLINE: Duration = Duration::from_secs(30);

    fn id(value: &str) -> Id {
        Id::new(value).unwrap()
    }

    /// The example's route `coding` has two targets; `coding-candidate` (alias
    /// `code-candidate`) pins `remote-large`.
    fn catalog() -> Catalog {
        let mut doc = CatalogDocument::parse(EXAMPLE).unwrap();
        doc.routes.push(Route {
            id: id("coding-candidate"),
            alias: id("code-candidate"),
            fallback_enabled: false,
            kind: RouteKind::Turn,
        });
        doc.targets.push(RouteTarget {
            id: id("candidate-large"),
            route_id: id("coding-candidate"),
            serving_model_id: id("remote-large"),
            position: 0,
        });
        doc.validate().unwrap()
    }

    /// `remote-large` at 3 and 5 per 1000 tokens: 10 input and 5 output tokens cost 0.055.
    fn prices(catalog: &Catalog) -> PriceBook {
        let rate = |amount: &str| {
            Some(Rate {
                amount: Amount::parse(amount).unwrap(),
                per_units: 1000,
            })
        };
        let serving = id("remote-large");
        let binding_revision = catalog
            .binding(&serving)
            .unwrap()
            .provenance()
            .binding_revision
            .clone();
        PriceBook::from_document(PriceDocument::new(
            id("prices-r1"),
            "fixture price list",
            1,
            Currency::new("USD").unwrap(),
            vec![ModelPrice {
                id: id("price-large"),
                serving_model_id: serving,
                binding_revision,
                upstream_model: id("example/Large-Model"),
                input: rate("3"),
                cached_input: None,
                cache_creation_input: None,
                output: rate("5"),
            }],
        ))
        .unwrap()
    }

    /// A clock only the scripted model moves.
    struct Hand(AtomicU64);

    impl Clock for Hand {
        fn unix_ms(&self) -> u64 {
            1_762_344_000_000 + self.0.load(Ordering::SeqCst)
        }
        fn monotonic(&self) -> Duration {
            Duration::from_millis(self.0.load(Ordering::SeqCst))
        }
    }

    /// What the scripted model does with each call.
    #[derive(Clone)]
    enum Step {
        /// Answers with these assistant messages, in order.
        Answer(Vec<&'static str>),
        /// Answers after the whole classification deadline has passed on the clock.
        Late(&'static str),
        /// Fails as rejected and retriable.
        Retriable,
    }

    struct Stepped<'c> {
        provenance: Provenance,
        capabilities: Capabilities,
        clock: &'c Hand,
        step: Step,
        asked: AtomicUsize,
    }

    impl Model for Stepped<'_> {
        fn provenance(&self) -> &Provenance {
            &self.provenance
        }
        fn capabilities(&self) -> &Capabilities {
            &self.capabilities
        }
        fn turn<'a>(
            &'a self,
            _: &'a TurnRequest,
            _: &'a mut dyn StreamSink,
            _: &'a Cancel,
        ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
            Box::pin(async move {
                self.asked.fetch_add(1, Ordering::SeqCst);
                let mut observation = TurnObservation::new(self.provenance.clone());
                observation.upstream_model = Some(id("example/Large-Model"));
                observation.final_usage = true;
                observation.usage = Some(Usage {
                    input_tokens: Some(10),
                    output_tokens: Some(5),
                    cached_input_tokens: Some(0),
                    cache_creation_input_tokens: Some(0),
                    reasoning_output_tokens: None,
                });
                let texts: Vec<&str> = match &self.step {
                    Step::Answer(texts) => texts.clone(),
                    Step::Late(text) => {
                        let late = u64::try_from(DEADLINE.as_millis()).unwrap();
                        self.clock.0.fetch_add(late, Ordering::SeqCst);
                        vec![*text]
                    }
                    Step::Retriable => {
                        return Err(Error::new(ErrorCode::RateLimited, "slow down")
                            .with_dispatch(Dispatch::Rejected)
                            .with_retriable(true)
                            .with_observation(observation));
                    }
                };
                Ok(TurnOutcome {
                    stop_reason: StopReason::EndTurn,
                    items: texts.iter().map(|text| Item::assistant(*text)).collect(),
                    observation,
                })
            })
        }
    }

    impl Models for Stepped<'_> {
        fn model(&self, _: &Id) -> Option<&dyn Model> {
            Some(self)
        }
    }

    fn turn_case(recorded: &str) -> Value {
        json!({
            "id": "c-1", "kind": "turn", "input_tokens": 100,
            "turn": {"format": "llm.turn/3", "request": {
                "model": "recorded-route", "instructions": "",
                "items": [{"kind": "user-text", "text": "hello"}],
                "tools": [], "max_output_tokens": 128
            }},
            "recorded": {"text": recorded}
        })
    }

    fn classification_case(labels: &[&str]) -> Value {
        json!({
            "id": "c-1", "kind": "classification", "input_tokens": 100,
            "label_set": {"id": "intent", "version": "3", "labels": labels},
            "instructions": "Classify the request.", "input": "I want my money back.",
            "recorded": {"label": "refund"}
        })
    }

    fn verdict_case(schema: &Value) -> Value {
        json!({
            "id": "c-1", "kind": "verdict", "input_tokens": 100,
            "rubric": {"id": "helpful", "version": "2", "text": "Score the answer.",
                "schema": schema},
            "subject": "Question and answer.",
            "recorded": {"verdict": {"score": 4}}
        })
    }

    fn score_schema() -> Value {
        json!({
            "type": "object",
            "properties": {"score": {"type": "integer", "minimum": 1, "maximum": 5}},
            "required": ["score"],
            "additionalProperties": false
        })
    }

    /// One run of one case on `route` under a cap of 1 with 0.1 held: the run, how many records
    /// it wrote and how often the model was asked.
    fn played(route: &str, case: &Value, step: Step) -> (ReplayRun, usize, usize) {
        let catalog = catalog();
        let prices = prices(&catalog);
        let clock = Hand(AtomicU64::new(0));
        let log = CallLog::new();
        let binding = catalog.binding(&id("remote-large")).unwrap();
        let model = Stepped {
            provenance: binding.provenance().clone(),
            capabilities: binding.capabilities().clone(),
            clock: &clock,
            step,
            asked: AtomicUsize::new(0),
        };
        let cancel = Cancel::new();
        let pause = |_: Duration| -> BoxFuture<'static, ()> { Box::pin(async {}) };
        let candidate = Routed {
            catalog: &catalog,
            models: &model,
            cancel: &cancel,
            pause: &pause,
            classification_deadline: DEADLINE,
        };
        let document = json!({"format": "llm.corpus/1", "cases": [case]}).to_string();
        let corpus = Corpus::parse(document.as_bytes()).unwrap();
        let run = Replay {
            id: id("run-1"),
            candidate_route_id: id(route),
            ledger_id: id("replay-spend"),
            currency: Currency::new("USD").unwrap(),
            money_cap: Amount::parse("1").unwrap(),
            reserve: Amount::parse("0.1").unwrap(),
        };
        let recording = Recording {
            sink: &log,
            clock: &clock,
            prices: Some(&prices),
        };
        let run = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(replay(&catalog, &corpus, &run, &candidate, recording))
            .unwrap();
        (run, log.records().len(), model.asked.load(Ordering::SeqCst))
    }

    fn result(run: &ReplayRun) -> (CaseResult, Option<ErrorCode>) {
        (run.results[0].result, run.results[0].error_code)
    }

    const REFUND: &str = r#"{"label":"refund","confidences":{"refund":9000,"other":1000}}"#;

    #[test]
    fn a_label_set_or_rubric_llm_decide_refuses_fails_its_case_before_any_call() {
        let unchecked = json!({"type": "object", "$ref": "#/definitions/score"});
        let rows = [
            (
                "a label set of one label",
                "coding-candidate",
                classification_case(&["refund"]),
            ),
            (
                "a label given twice",
                "coding-candidate",
                classification_case(&["refund", "refund"]),
            ),
            (
                "a schema keyword no answer is checked against",
                "coding-candidate",
                verdict_case(&unchecked),
            ),
            (
                "a verdict on a route with two targets",
                "coding",
                verdict_case(&score_schema()),
            ),
        ];
        for (name, route, case) in rows {
            let (run, records, asked) = played(route, &case, Step::Answer(vec![REFUND]));
            assert_eq!(
                result(&run),
                (CaseResult::Failed, Some(ErrorCode::InvalidRequest)),
                "{name}"
            );
            assert_eq!((records, asked, run.spent), (0, 0, Amount::ZERO), "{name}");
        }
    }

    #[test]
    fn a_classification_answered_past_its_deadline_fails_with_deadline() {
        let case = classification_case(&["refund", "other"]);
        let (on_time, _, _) = played("coding-candidate", &case, Step::Answer(vec![REFUND]));
        assert_eq!(result(&on_time), (CaseResult::Same, None));
        let (late, records, asked) = played("coding-candidate", &case, Step::Late(REFUND));
        assert_eq!(
            result(&late),
            (CaseResult::Failed, Some(ErrorCode::Deadline))
        );
        // The late call was made and is paid for.
        assert_eq!((records, asked), (1, 1));
        assert_eq!(late.spent, Amount::parse("0.055").unwrap());
    }

    #[test]
    fn a_verdict_the_schema_does_not_hold_differs() {
        let case = verdict_case(&score_schema());
        let rows = [
            ("the recorded verdict", r#"{"score":4}"#, CaseResult::Same),
            ("another score", r#"{"score":3}"#, CaseResult::Differs),
            (
                "a score above the schema's maximum",
                r#"{"score":9}"#,
                CaseResult::Differs,
            ),
            ("no JSON", "four", CaseResult::Differs),
        ];
        for (name, answer, expected) in rows {
            let (run, records, _) = played("coding-candidate", &case, Step::Answer(vec![answer]));
            assert_eq!(result(&run), (expected, None), "{name}");
            assert_eq!(records, 1, "{name}");
        }
    }

    #[test]
    fn a_turn_or_classification_that_fails_retriably_is_asked_once() {
        let rows = [
            ("a turn", turn_case("one")),
            (
                "a classification",
                classification_case(&["refund", "other"]),
            ),
        ];
        for (name, case) in rows {
            let (run, records, asked) = played("coding-candidate", &case, Step::Retriable);
            assert_eq!(
                result(&run),
                (CaseResult::Failed, Some(ErrorCode::RateLimited)),
                "{name}"
            );
            assert_eq!((records, asked), (1, 1), "{name}");
        }
    }

    #[test]
    fn a_turns_answer_is_the_text_of_its_assistant_messages_in_order() {
        let (run, _, _) = played(
            "coding-candidate",
            &turn_case("one two"),
            Step::Answer(vec!["one", " two"]),
        );
        assert_eq!(result(&run), (CaseResult::Same, None));
        let (swapped, _, _) = played(
            "coding-candidate",
            &turn_case("one two"),
            Step::Answer(vec![" two", "one"]),
        );
        assert_eq!(result(&swapped), (CaseResult::Differs, None));
    }
}
