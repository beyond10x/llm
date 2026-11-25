//! `Routed` under the real `llm_calls::replay`, `ModelClassifier` and `judge`, over one scripted
//! model, a clock the model moves and an in-memory record sink. No network.

use super::block_on;
use llm_calls::{
    CallLog, CallRecord, CaseResult, Clock, Corpus, Kind, MAX_RUBRIC_ID_BYTES, Outcome,
    REPLAY_CLASSIFIER_ID, Recording, Replay, ReplayRun, replay,
};
use llm_core::{
    BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item, Model, Provenance,
    StopReason, StreamSink, TurnObservation, TurnOutcome, TurnRequest, Usage,
};
use llm_cost::budget::BudgetError;
use llm_cost::{Amount, Currency, ModelPrice, PriceBook, PriceDocument, Rate};
use llm_decide::Routed;
use llm_routing::{Catalog, CatalogDocument, Models, Route, RouteKind, RouteTarget};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    sync::atomic::{AtomicU64, AtomicUsize, Ordering},
    time::Duration,
};

const EXAMPLE: &str = include_str!("../../../../examples/catalog.toml");
const DEADLINE: Duration = Duration::from_secs(30);

fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}

/// The example's route `coding` has two targets; `coding-candidate` (alias `code-candidate`)
/// pins `remote-large`.
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
    /// Answers with this one assistant message of a turn that stopped at the output limit.
    Cut(&'static str),
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
            let mut stop_reason = StopReason::EndTurn;
            let texts: Vec<&str> = match &self.step {
                Step::Answer(texts) => texts.clone(),
                Step::Cut(text) => {
                    stop_reason = StopReason::MaxOutputTokens;
                    vec![*text]
                }
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
                stop_reason,
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

fn turn_case(case_id: &str, recorded: &str) -> Value {
    json!({
        "id": case_id, "kind": "turn", "input_tokens": 100,
        "turn": {"format": "llm.turn/3", "request": {
            "model": "recorded-route", "instructions": "",
            "items": [{"kind": "user-text", "text": "hello"}],
            "tools": [], "max_output_tokens": 128
        }},
        "recorded": {"text": recorded}
    })
}

fn classification_case(case_id: &str, labels: &[&str]) -> Value {
    json!({
        "id": case_id, "kind": "classification", "input_tokens": 100,
        "label_set": {"id": "intent", "version": "3", "labels": labels},
        "instructions": "Classify the request.", "input": "I want my money back.",
        "recorded": {"label": "refund"}
    })
}

fn verdict_case(case_id: &str, rubric_id: &str, schema: &Value) -> Value {
    json!({
        "id": case_id, "kind": "verdict", "input_tokens": 100,
        "rubric": {"id": rubric_id, "version": "2", "text": "Score the answer.",
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

/// One run `run-1` of `cases` on `route` under `cap` with 0.1 held per case, every call
/// answered as `step` says: the run, its records and how often the model was asked.
fn run_of(
    route: &str,
    cases: &[Value],
    step: Step,
    cap: &str,
) -> (ReplayRun, Vec<CallRecord>, usize) {
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
    let document = json!({"format": "llm.corpus/1", "cases": cases}).to_string();
    let corpus = Corpus::parse(document.as_bytes()).unwrap();
    let run = Replay {
        id: id("run-1"),
        candidate_route_id: id(route),
        ledger_id: id("replay-spend"),
        currency: Currency::new("USD").unwrap(),
        money_cap: Amount::parse(cap).unwrap(),
        reserve: Amount::parse("0.1").unwrap(),
    };
    let recording = Recording {
        sink: &log,
        clock: &clock,
        prices: Some(&prices),
    };
    let run = block_on(replay(&catalog, &corpus, &run, &candidate, recording)).unwrap();
    (run, log.records(), model.asked.load(Ordering::SeqCst))
}

/// One run of one case under a cap of 1: the run, how many records it wrote and how often the
/// model was asked.
fn played(route: &str, case: &Value, step: Step) -> (ReplayRun, usize, usize) {
    let (run, records, asked) = run_of(route, std::slice::from_ref(case), step, "1");
    (run, records.len(), asked)
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
            classification_case("c-1", &["refund"]),
        ),
        (
            "a label given twice",
            "coding-candidate",
            classification_case("c-1", &["refund", "refund"]),
        ),
        (
            "a schema keyword no answer is checked against",
            "coding-candidate",
            verdict_case("c-1", "helpful", &unchecked),
        ),
        (
            "a verdict on a route with two targets",
            "coding",
            verdict_case("c-1", "helpful", &score_schema()),
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
    let case = classification_case("c-1", &["refund", "other"]);
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

/// `Routed::label` asks the JSON-schema classifier, so a recorded classification is `same` only
/// on a whole answer: one that is cut, split or gives a key twice names no label and differs.
#[test]
fn a_classification_answered_cut_split_or_with_a_repeated_key_names_no_label_and_differs() {
    const HEAD: &str = r#"{"label":"refund","confiden"#;
    const TAIL: &str = r#"ces":{"refund":9000,"other":1000}}"#;
    const TWICE: &str =
        r#"{"label":"other","label":"refund","confidences":{"refund":9000,"other":1000}}"#;
    assert_eq!([HEAD, TAIL].concat(), REFUND);
    let case = classification_case("c-1", &["refund", "other"]);
    let rows = [
        (
            "the whole recorded answer",
            Step::Answer(vec![REFUND]),
            CaseResult::Same,
        ),
        (
            "the same text of a turn cut at the output limit",
            Step::Cut(REFUND),
            CaseResult::Differs,
        ),
        (
            "the same text in two assistant messages",
            Step::Answer(vec![HEAD, TAIL]),
            CaseResult::Differs,
        ),
        (
            "the recorded label as the second of two",
            Step::Answer(vec![TWICE]),
            CaseResult::Differs,
        ),
    ];
    for (name, step, expected) in rows {
        let (run, records, asked) = played("coding-candidate", &case, step);
        assert_eq!(result(&run), (expected, None), "{name}");
        assert_eq!((records, asked), (1, 1), "{name}");
    }
}

#[test]
fn a_verdict_the_schema_does_not_hold_differs() {
    let case = verdict_case("c-1", "helpful", &score_schema());
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
fn a_case_whose_recorded_verdict_its_schema_does_not_hold_is_taken_and_differs() {
    // The reader takes the recorded verdict as given: a score the schema's maximum excludes.
    let mut case = verdict_case("c-1", "helpful", &score_schema());
    case["recorded"]["verdict"] = json!({"score": 9});
    let rows = [
        ("the recorded verdict itself", r#"{"score":9}"#),
        ("a verdict the schema holds", r#"{"score":4}"#),
    ];
    for (name, answer) in rows {
        let (run, records, asked) = played("coding-candidate", &case, Step::Answer(vec![answer]));
        assert_eq!(result(&run), (CaseResult::Differs, None), "{name}");
        assert_eq!((records, asked), (1, 1), "{name}");
    }
}

#[test]
fn a_turn_or_classification_that_fails_retriably_is_asked_once() {
    let rows = [
        ("a turn", turn_case("c-1", "one")),
        (
            "a classification",
            classification_case("c-1", &["refund", "other"]),
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
        &turn_case("c-1", "one two"),
        Step::Answer(vec!["one", " two"]),
    );
    assert_eq!(result(&run), (CaseResult::Same, None));
    let (swapped, _, _) = played(
        "coding-candidate",
        &turn_case("c-1", "one two"),
        Step::Answer(vec![" two", "one"]),
    );
    assert_eq!(result(&swapped), (CaseResult::Differs, None));
}

#[test]
fn no_two_calls_of_one_run_share_a_record_id() {
    // Case and rubric ids that name each other's suffixes, and ids long enough for the digest
    // form of each kind. Eight cases are sent at 0.055 each; the ninth would need 0.44 + 0.1.
    let schema = score_schema();
    let labels = ["refund", "other"];
    let long = |length: usize| "c".repeat(length);
    let cases = [
        turn_case("a", "one"),
        classification_case("b", &labels),
        verdict_case("c", "helpful", &schema),
        turn_case(REPLAY_CLASSIFIER_ID, "one"),
        verdict_case("d", REPLAY_CLASSIFIER_ID, &schema),
        turn_case(&long(251), "one"),
        classification_case(&long(242), &labels),
        verdict_case(&long(243), "helpful", &schema),
        turn_case("z", "one"),
    ];
    let answer = Step::Answer(vec![r#"{"score":4}"#]);
    let (run, records, asked) = run_of("coding-candidate", &cases, answer, "0.5");
    assert_eq!((run.cases, run.not_sent, asked), (9, 1, 8));
    assert_eq!(run.stopped, Some(BudgetError::LimitExceeded));
    assert_eq!(
        records
            .iter()
            .map(|record| (record.kind, record.outcome))
            .collect::<Vec<_>>(),
        [
            (Kind::Turn, Outcome::Succeeded),
            (Kind::Classification, Outcome::Succeeded),
            (Kind::Verdict, Outcome::Succeeded),
            (Kind::Turn, Outcome::Succeeded),
            (Kind::Verdict, Outcome::Succeeded),
            (Kind::Turn, Outcome::Succeeded),
            (Kind::Classification, Outcome::Succeeded),
            (Kind::Verdict, Outcome::Succeeded),
            (Kind::Turn, Outcome::Skipped),
        ]
    );
    let ids: Vec<&str> = records.iter().map(|record| record.id.as_str()).collect();
    assert_eq!(
        [ids[0], ids[1], ids[2], ids[3], ids[4], ids[8]],
        [
            "run-1/a",
            "run-1/b/classify",
            "run-1/c/helpful",
            "run-1/classify",
            "run-1/d/classify",
            "run-1/z"
        ]
    );
    for (digest, suffix) in [(ids[5], ""), (ids[6], "/classify"), (ids[7], "/helpful")] {
        assert!(digest.starts_with("replay-sha256-"), "{digest}");
        assert!(digest.ends_with(suffix), "{digest}");
        assert_eq!(digest.len(), 78 + suffix.len(), "{digest}");
    }
    let distinct: BTreeSet<&str> = ids.iter().copied().collect();
    assert_eq!(distinct.len(), 9, "{ids:?}");
}

#[test]
fn a_case_id_that_would_meet_another_cases_record_id_is_refused_at_parse() {
    // Without the rule a classification `a` and a turn `a/classify` would both write
    // `run-1/a/classify`.
    let cases = [
        classification_case("a", &["refund", "other"]),
        turn_case("a/classify", "one"),
    ];
    let document = json!({"format": "llm.corpus/1", "cases": cases}).to_string();
    let error = Corpus::parse(document.as_bytes()).unwrap_err();
    assert_eq!(
        (error.code, error.path.as_str()),
        (ErrorCode::InvalidRequest, "cases[1].id")
    );
}

#[test]
fn a_classification_or_verdict_case_is_sent_whatever_the_length_of_its_ids() {
    let labels = ["refund", "other"];
    for length in [1, 241, 242, 245, 256] {
        let case = classification_case(&"c".repeat(length), &labels);
        let (run, records, asked) =
            run_of("coding-candidate", &[case], Step::Answer(vec![REFUND]), "1");
        assert_eq!(result(&run), (CaseResult::Same, None), "{length}");
        assert_eq!((records.len(), asked), (1, 1), "{length}");
        assert!(records[0].id.as_str().ends_with("/classify"), "{length}");
    }
    // The case id and the rubric id on each side of where their record id stops being an id,
    // and each at its longest.
    let schema = score_schema();
    let rows = [
        (1, 7),
        (242, 7),
        (243, 7),
        (256, 7),
        (1, MAX_RUBRIC_ID_BYTES),
        (72, MAX_RUBRIC_ID_BYTES),
        (73, MAX_RUBRIC_ID_BYTES),
        (256, MAX_RUBRIC_ID_BYTES),
    ];
    for (case_length, rubric_length) in rows {
        let name = format!("{case_length} and {rubric_length}");
        let rubric_id = "r".repeat(rubric_length);
        let case = verdict_case(&"c".repeat(case_length), &rubric_id, &schema);
        let (run, records, asked) = run_of(
            "coding-candidate",
            &[case],
            Step::Answer(vec![r#"{"score":4}"#]),
            "1",
        );
        assert_eq!(result(&run), (CaseResult::Same, None), "{name}");
        assert_eq!((records.len(), asked), (1, 1), "{name}");
        let record_id = records[0].id.as_str();
        assert!(record_id.ends_with(&format!("/{rubric_id}")), "{name}");
        let readable = "run-1/".len() + case_length + 1 + rubric_length <= 256;
        assert_eq!(record_id.starts_with("run-1/"), readable, "{name}");
    }
}
