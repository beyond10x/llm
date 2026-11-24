//! Adversary cases for the rubric judge (wave 27, unit 1): what "never repaired" and "no key is
//! dropped, no value coerced" hold an answer to, how `enum`, `const`, `minimum` and `maximum`
//! compare numbers, whether the request carries the schema bytes the digest covers, and guards
//! for the same-target retry and the depth bound.
//!
//! The scripted model replays the authored items and stop reason; nothing is chosen here.

use llm_calls::{CallLog, CallRecord, Clock, Kind, Recording};
use llm_core::{
    BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item, Model, Provenance,
    StopReason, StreamSink, TurnObservation, TurnOutcome, TurnRequest,
};
use llm_decide::{JudgeError, Ports, Rubric, RubricError, Verdict, judge};
use llm_routing::{Catalog, CatalogDocument, Models, Selection};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    future::Future,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
    time::Duration,
};

const EXAMPLE: &str = include_str!("../../../examples/catalog.toml");
const TEXT: &str = "Score the answer.";
const SUBJECT: &str = "Question: what is the capital of France? Answer: Paris.";
const REPORTED: &str = "example/Large-Model-2026-09";

fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
    }
}

/// The example catalog with the route `code` cut to its one target that declares temperature.
fn pinned() -> Catalog {
    let mut doc = CatalogDocument::parse(EXAMPLE).unwrap();
    doc.targets
        .retain(|target| target.serving_model_id.as_str() == "remote-large");
    doc.targets[0].position = 0;
    doc.validate().unwrap()
}

struct Still;

impl Clock for Still {
    fn unix_ms(&self) -> u64 {
        1_762_430_400_000
    }
    fn monotonic(&self) -> Duration {
        Duration::ZERO
    }
}

/// What the far side returns: an optional failure of the first call, then these items.
#[derive(Clone)]
struct Far {
    fail_first: Option<Error>,
    items: Vec<Item>,
    stop_reason: StopReason,
}

impl Far {
    fn answers(text: &str) -> Self {
        Self {
            fail_first: None,
            items: vec![Item::assistant(text)],
            stop_reason: StopReason::EndTurn,
        }
    }
}

struct Fake {
    provenance: Provenance,
    capabilities: Capabilities,
    far: Far,
    calls: AtomicUsize,
    /// Every request received, as its JSON bytes.
    seen: Mutex<Vec<String>>,
}

impl Model for Fake {
    fn provenance(&self) -> &Provenance {
        &self.provenance
    }
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }
    fn turn<'a>(
        &'a self,
        request: &'a TurnRequest,
        _: &'a mut dyn StreamSink,
        _: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        Box::pin(async move {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            self.seen
                .lock()
                .unwrap()
                .push(serde_json::to_string(request).unwrap());
            let mut observation = TurnObservation::new(self.provenance.clone());
            observation.upstream_model = Some(id(REPORTED));
            if call == 0
                && let Some(error) = &self.far.fail_first
            {
                return Err(error.clone().with_observation(observation));
            }
            observation.final_usage = true;
            Ok(TurnOutcome {
                stop_reason: self.far.stop_reason.clone(),
                items: self.far.items.clone(),
                observation,
            })
        })
    }
}

struct Fleet(BTreeMap<Id, Fake>);

impl Models for Fleet {
    fn model(&self, serving_model_id: &Id) -> Option<&dyn Model> {
        self.0
            .get(serving_model_id)
            .map(|model| model as &dyn Model)
    }
}

struct Ran {
    result: Result<Verdict, JudgeError>,
    records: Vec<CallRecord>,
    seen: Vec<String>,
}

fn run(rubric: &Rubric, far: &Far) -> Ran {
    let catalog = pinned();
    let fleet = Fleet(
        catalog
            .bindings()
            .map(|(serving, binding)| {
                let model = Fake {
                    provenance: binding.provenance().clone(),
                    capabilities: binding.capabilities().clone(),
                    far: far.clone(),
                    calls: AtomicUsize::new(0),
                    seen: Mutex::new(Vec::new()),
                };
                (serving.clone(), model)
            })
            .collect(),
    );
    let log = CallLog::new();
    let cancel = Cancel::new();
    let admit = |_: &Selection<'_>| Ok(());
    let pause = |_: Duration| -> BoxFuture<'static, ()> { Box::pin(async {}) };
    let result = block_on(judge(
        rubric,
        SUBJECT,
        &id("req-1"),
        Some(100),
        Ports {
            catalog: &catalog,
            models: &fleet,
            admit: &admit,
            cancel: &cancel,
            pause: &pause,
            recording: Recording {
                sink: &log,
                clock: &Still,
                prices: None,
            },
        },
    ));
    let seen = fleet
        .0
        .values()
        .flat_map(|model| model.seen.lock().unwrap().clone())
        .collect();
    Ran {
        result,
        records: log.records(),
        seen,
    }
}

fn rubric_of(schema: &Value) -> Rubric {
    Rubric::new(id("r"), "v1", TEXT, schema, "code").unwrap()
}

/// The verdict JSON and error code of one answer judged against `schema`.
fn judged(schema: &Value, answer: &str) -> (Option<String>, Option<ErrorCode>) {
    let ran = run(&rubric_of(schema), &Far::answers(answer));
    let verdict = ran.result.unwrap();
    (verdict.verdict_json, verdict.error_code)
}

fn score_and_pass() -> Value {
    json!({
        "type": "object",
        "properties": {
            "score": {"type": "integer", "minimum": 1, "maximum": 5},
            "pass": {"type": "boolean"}
        },
        "required": ["score", "pass"],
        "additionalProperties": false
    })
}

/// `docs/design.md`, Verdict: "The answer is taken as it arrived: no key is dropped, no value
/// coerced". An answer that gives one key twice has no single reading; keeping the last value is
/// a repair, and it can turn an answer that says both `false` and `true` into a passing verdict.
#[test]
fn an_answer_that_repeats_a_key_is_refused_not_cut_to_its_last_value() {
    for answer in [
        r#"{"pass":false,"pass":true,"score":5}"#,
        r#"{"score":1,"score":5,"pass":true}"#,
        r#"{"score":"five","score":5,"pass":true}"#,
    ] {
        assert_eq!(
            judged(&score_and_pass(), answer),
            (None, Some(ErrorCode::Protocol)),
            "{answer}"
        );
    }
}

/// "No value coerced": a number of the answer reaches the verdict JSON with the value it had.
/// Each answer below is the shortest decimal form of its `f64`, so canonical JSON writes it back
/// digit for digit when it was read exactly.
#[test]
fn a_number_of_the_answer_reaches_the_verdict_with_every_digit() {
    let schema = json!({
        "type": "object",
        "properties": {"score": {"type": "number"}},
        "required": ["score"],
        "additionalProperties": false
    });
    // The literals are their own shortest form: writing the exact `f64` gives them back.
    assert_eq!(
        json!(51.248_178_375_505_404_f64).to_string(),
        "51.248178375505404"
    );
    assert_eq!(
        json!(97.453_653_200_346_85_f64).to_string(),
        "97.45365320034685"
    );
    assert_eq!(
        json!(-93.311_370_376_880_33_f64).to_string(),
        "-93.31137037688033"
    );
    for answer in [
        r#"{"score":51.248178375505404}"#,
        r#"{"score":97.45365320034685}"#,
        r#"{"score":-93.31137037688033}"#,
    ] {
        assert_eq!(
            judged(&schema, answer),
            (Some(answer.to_owned()), None),
            "{answer}"
        );
    }
}

/// `Rubric::new` keeps the schema "so the request carries the bytes the digest covers however
/// the caller wrote the schema" (`judge.rs`). The digest covers the canonical JSON of the schema
/// the caller gave; the request carries that text read back.
#[test]
fn the_request_carries_the_schema_bytes_the_digest_covers() {
    let schema = json!({"type": "number", "maximum": 51.248_178_375_505_404_f64});
    let canonical = r#"{"maximum":51.248178375505404,"type":"number"}"#;
    let rubric = rubric_of(&schema);
    assert_eq!(rubric.schema().to_string(), canonical);
    let ran = run(&rubric, &Far::answers("1"));
    assert_eq!(ran.seen.len(), 1);
    assert!(
        ran.seen[0].contains(canonical),
        "the request sent another schema than the digest covers: {}",
        ran.seen[0]
    );
}

/// `schema.rs`: "Every keyword is read as JSON Schema reads it", and "An integer is a number
/// without a fractional part, however it is written". JSON Schema compares two numbers of
/// `enum` and `const` by mathematical value, at every depth.
#[test]
fn enum_and_const_compare_numbers_by_value_however_they_are_written() {
    let cases: [(Value, &str); 6] = [
        (json!({"type": "number", "enum": [0, 0.5, 1]}), "1.0"),
        (json!({"type": "number", "enum": [0, 0.5, 1]}), "0.0"),
        (json!({"type": "integer", "enum": [1, 2, 3]}), "2.0"),
        (json!({"type": "integer", "const": 5}), "5.0"),
        (json!({"enum": [1.0, 2.0]}), "1"),
        (json!({"const": {"a": [1, 2]}}), r#"{"a":[1.0,2]}"#),
    ];
    for (schema, answer) in cases {
        let (verdict_json, error_code) = judged(&schema, answer);
        assert_eq!(error_code, None, "{schema} admits {answer}");
        let held: Value = serde_json::from_str(&verdict_json.unwrap()).unwrap();
        let sent: Value = serde_json::from_str(answer).unwrap();
        assert_eq!(held, sent, "{schema} keeps {answer}");
    }
}

/// A bound holds for every integer JSON can carry, not only below 2^53.
#[test]
fn a_bound_is_compared_exactly_beyond_two_to_the_53rd() {
    assert_eq!(
        judged(
            &json!({"type": "integer", "maximum": 9_007_199_254_740_992_u64}),
            "9007199254740993"
        ),
        (None, Some(ErrorCode::Protocol)),
        "2^53 + 1 is over a maximum of 2^53"
    );
    assert_eq!(
        judged(
            &json!({"type": "integer", "minimum": 9_007_199_254_740_993_u64}),
            "9007199254740992"
        ),
        (None, Some(ErrorCode::Protocol)),
        "2^53 is under a minimum of 2^53 + 1"
    );
}

/// An answer the far side says was cut off at the output limit is not the model's verdict, even
/// when the part that arrived reads as a value of the schema.
#[test]
fn an_answer_cut_off_at_the_output_limit_is_not_a_verdict() {
    let far = Far {
        stop_reason: StopReason::MaxOutputTokens,
        ..Far::answers("12")
    };
    let ran = run(&rubric_of(&json!({"type": "integer", "minimum": 1})), &far);
    let verdict = ran.result.unwrap();
    assert_eq!(
        (verdict.verdict_json, verdict.error_code),
        (None, Some(ErrorCode::Protocol))
    );
}

/// Guard, green on the unit as delivered: "the same-target retry of every routed call stays".
/// A retriable failure is tried again on the one target with the same bytes, and the one record
/// counts both attempts.
#[test]
fn a_retriable_failure_is_retried_on_the_one_target_with_the_same_bytes() {
    let far = Far {
        fail_first: Some(
            Error::new(ErrorCode::RateLimited, "scripted")
                .with_dispatch(Dispatch::Accepted)
                .with_retriable(true),
        ),
        ..Far::answers(r#"{"score":5,"pass":true}"#)
    };
    let ran = run(&rubric_of(&score_and_pass()), &far);
    let verdict = ran.result.unwrap();
    assert_eq!(
        verdict.verdict_json.as_deref(),
        Some(r#"{"pass":true,"score":5}"#)
    );
    assert_eq!(ran.seen.len(), 2);
    assert_eq!(ran.seen[0], ran.seen[1]);
    assert_eq!(ran.records.len(), 1);
    assert_eq!(ran.records[0].kind, Kind::Verdict);
    assert_eq!(ran.records[0].attempts, 2);
}

/// Guard, green on the unit as delivered: a schema nests 64 levels and no deeper, an answer is
/// still held to a schema at the bound, and a literal inside `const` and `enum` is bounded too.
#[test]
fn a_schema_and_a_literal_nest_to_the_bound_and_no_deeper() {
    let nest = |levels: usize| {
        let mut schema = json!({"type": "string"});
        for _ in 0..levels {
            schema = json!({"type": "array", "items": schema});
        }
        schema
    };
    let around =
        |levels: usize, inner: &str| format!("{}{inner}{}", "[".repeat(levels), "]".repeat(levels));
    let refused = Rubric::new(id("r"), "v1", TEXT, &nest(65), "code");
    assert_eq!(refused.unwrap_err(), RubricError::UnsupportedSchema);
    let at_bound = nest(64);
    assert_eq!(
        judged(&at_bound, &around(64, r#""a""#)),
        (Some(around(64, r#""a""#)), None)
    );
    assert_eq!(
        judged(&at_bound, &around(64, "1")),
        (None, Some(ErrorCode::Protocol))
    );
    // 100 levels: over the bound of 64, and under the 128 levels past which reading the canonical
    // schema back fails by itself, so only the bound on a literal refuses these.
    let literal = |levels: usize| {
        let mut literal = json!("a");
        for _ in 0..levels {
            literal = json!([literal]);
        }
        literal
    };
    for schema in [
        json!({"const": literal(100)}),
        json!({"enum": [literal(100)]}),
    ] {
        let refused = Rubric::new(id("r"), "v1", TEXT, &schema, "code");
        assert_eq!(refused.unwrap_err(), RubricError::UnsupportedSchema);
    }
    for schema in [
        json!({"const": literal(10)}),
        json!({"enum": [literal(10)]}),
    ] {
        assert_eq!(
            judged(&schema, &around(10, r#""a""#)),
            (Some(around(10, r#""a""#)), None)
        );
    }
}
