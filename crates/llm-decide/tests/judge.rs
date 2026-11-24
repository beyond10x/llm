//! The rubric judge through scripted `llm_core::Model`s routed by the example catalog cut to one
//! target, a scripted clock and an in-memory call log: no projection, no network.
//!
//! `pinned("remote-large")` leaves the route `code` its one target that declares temperature,
//! `pinned("local-small")` the one that does not.

use llm_calls::{CallLog, CallRecord, Clock, Kind, Outcome, Recording};
use llm_core::{
    BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item, Model, Provenance,
    StopReason, StreamEvent, StreamSink, TurnObservation, TurnOutcome, TurnRequest, Usage,
};
use llm_decide::{JudgeError, Ports, Rubric, RubricError, VERDICT_FORMAT_NAME, Verdict, judge};
use llm_routing::{Catalog, CatalogDocument, Models, Selection};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    future::Future,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll, Waker},
    time::Duration,
};

const EXAMPLE: &str = include_str!("../../../examples/catalog.toml");
const TEXT: &str =
    "Score from 1 to 5 how well the answer addresses the question, and say whether it passes.";
const SUBJECT: &str = "Question: what is the capital of France? Answer: Paris.";
/// The schema of [`schema`] as canonical JSON: keys sorted, no whitespace.
const CANONICAL_SCHEMA: &str = r#"{"additionalProperties":false,"properties":{"pass":{"type":"boolean"},"score":{"maximum":5,"minimum":1,"type":"integer"}},"required":["score","pass"],"type":"object"}"#;
/// `sha256sum` of: the length of [`TEXT`] (88) as eight big-endian bytes, [`TEXT`], then
/// [`CANONICAL_SCHEMA`]; computed outside this crate.
const DIGEST: &str = "020bdd6be2d891b692850e2842dc91ff573492dc260189e2cfd356f66c61840b";
const REPORTED: &str = "example/Large-Model-2026-09";
const GOOD: &str = r#"{"score":5,"pass":true}"#;

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

/// The example catalog with the route `code` cut to the one target that serves `serving`.
fn pinned(serving: &str) -> Catalog {
    let mut doc = CatalogDocument::parse(EXAMPLE).unwrap();
    doc.targets
        .retain(|target| target.serving_model_id.as_str() == serving);
    doc.targets[0].position = 0;
    doc.validate().unwrap()
}

/// The example catalog with both targets of the route `code`, and its fallback as given.
fn two_targets(fallback_enabled: bool) -> Catalog {
    let mut doc = CatalogDocument::parse(EXAMPLE).unwrap();
    doc.routes[0].fallback_enabled = fallback_enabled;
    doc.validate().unwrap()
}

/// The verdict schema, written with its keys in another order than the canonical one.
fn schema() -> Value {
    serde_json::from_str(
        r#"{"type":"object","properties":{"score":{"type":"integer","minimum":1,"maximum":5},"pass":{"type":"boolean"}},"required":["score","pass"],"additionalProperties":false}"#,
    )
    .unwrap()
}

fn rubric() -> Rubric {
    Rubric::new(id("helpfulness"), "v3", TEXT, &schema(), "code").unwrap()
}

/// A clock that moves only when a scripted model says time passed.
struct Stepped {
    unix_ms: u64,
    now_ms: AtomicU64,
}

impl Clock for Stepped {
    fn unix_ms(&self) -> u64 {
        self.unix_ms + self.now_ms.load(Ordering::SeqCst)
    }
    fn monotonic(&self) -> Duration {
        Duration::from_millis(self.now_ms.load(Ordering::SeqCst))
    }
}

/// What the far side does with the one request: the authored reply, nothing chosen here.
#[derive(Clone)]
struct Far {
    reply: Result<String, Error>,
    upstream_model: Option<&'static str>,
    takes_ms: u64,
    usage: Option<Usage>,
}

impl Far {
    fn answers(text: &str) -> Self {
        Self {
            reply: Ok(text.to_owned()),
            upstream_model: Some(REPORTED),
            takes_ms: 0,
            usage: None,
        }
    }
}

/// Replays one authored reply and keeps every request it was sent.
struct Fake<'c> {
    provenance: Provenance,
    capabilities: Capabilities,
    clock: &'c Stepped,
    far: Far,
    seen: Mutex<Vec<TurnRequest>>,
}

impl Model for Fake<'_> {
    fn provenance(&self) -> &Provenance {
        &self.provenance
    }
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }
    fn turn<'a>(
        &'a self,
        request: &'a TurnRequest,
        sink: &'a mut dyn StreamSink,
        _: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        Box::pin(async move {
            self.seen.lock().unwrap().push(request.clone());
            self.clock
                .now_ms
                .fetch_add(self.far.takes_ms, Ordering::SeqCst);
            let mut observation = TurnObservation::new(self.provenance.clone());
            observation.usage.clone_from(&self.far.usage);
            observation.upstream_model = self.far.upstream_model.map(id);
            match &self.far.reply {
                Err(error) => Err(error.clone().with_observation(observation)),
                Ok(text) => {
                    sink.emit(StreamEvent::TextDelta { text: text.clone() })
                        .await?;
                    observation.final_usage = true;
                    Ok(TurnOutcome {
                        stop_reason: StopReason::EndTurn,
                        items: vec![Item::assistant(text.clone())],
                        observation,
                    })
                }
            }
        })
    }
}

struct Fleet<'c>(BTreeMap<Id, Fake<'c>>);

impl Models for Fleet<'_> {
    fn model(&self, serving_model_id: &Id) -> Option<&dyn Model> {
        self.0
            .get(serving_model_id)
            .map(|model| model as &dyn Model)
    }
}

struct Ran {
    result: Result<Verdict, JudgeError>,
    records: Vec<CallRecord>,
    /// Every request any model of the catalog was sent, in serving-model order.
    seen: Vec<TurnRequest>,
}

impl Ran {
    fn verdict(&self) -> &Verdict {
        self.result.as_ref().unwrap()
    }
}

/// One judge run: every serving model of the catalog replays `far`, so a request that reached
/// any of them is seen.
fn run_as(
    catalog: &Catalog,
    rubric: &Rubric,
    subject: &str,
    request_id: &str,
    unix_ms: u64,
    far: &Far,
) -> Ran {
    let clock = Stepped {
        unix_ms,
        now_ms: AtomicU64::new(0),
    };
    let fleet = Fleet(
        catalog
            .bindings()
            .map(|(serving, binding)| {
                let model = Fake {
                    provenance: binding.provenance().clone(),
                    capabilities: binding.capabilities().clone(),
                    clock: &clock,
                    far: far.clone(),
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
        subject,
        &id(request_id),
        Some(100),
        Ports {
            catalog,
            models: &fleet,
            admit: &admit,
            cancel: &cancel,
            pause: &pause,
            recording: Recording {
                sink: &log,
                clock: &clock,
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

fn run(catalog: &Catalog, far: &Far) -> Ran {
    run_as(catalog, &rubric(), SUBJECT, "req-1", 1_762_430_400_000, far)
}

#[test]
fn a_verdict_names_the_rubric_version_digest_target_and_upstream_model() {
    let ran = run(&pinned("remote-large"), &Far::answers(GOOD));
    assert_eq!(
        ran.verdict(),
        &Verdict {
            rubric_id: id("helpfulness"),
            rubric_version: "v3".into(),
            rubric_digest: DIGEST.into(),
            route_target_id: id("coding-secondary"),
            upstream_model: id(REPORTED),
            verdict_json: Some(r#"{"pass":true,"score":5}"#.into()),
            error_code: None,
            call_record_id: id("req-1/helpfulness"),
        }
    );
    // The model named is the one the far side reported, not the catalog's declared name.
    assert_ne!(REPORTED, "example/Large-Model");
}

#[test]
fn the_rubric_digest_is_the_sha256_of_its_text_and_schema_and_of_nothing_else() {
    let base = rubric();
    assert_eq!(base.digest(), DIGEST);
    assert_eq!(base.schema().to_string(), CANONICAL_SCHEMA);
    // Neither the id, the version nor the route enters it.
    let renamed = Rubric::new(id("other"), "v9", TEXT, &schema(), "elsewhere").unwrap();
    assert_eq!(renamed.digest(), DIGEST);
    // The schema enters as canonical JSON, so the order its keys were written in does not.
    let reordered = Rubric::new(
        id("helpfulness"),
        "v3",
        TEXT,
        &serde_json::from_str(CANONICAL_SCHEMA).unwrap(),
        "code",
    )
    .unwrap();
    assert_eq!(reordered.digest(), DIGEST);
    // One more byte of text, or another bound in the schema, is another digest.
    let retexted = Rubric::new(
        id("helpfulness"),
        "v3",
        format!("{TEXT} "),
        &schema(),
        "code",
    );
    assert_ne!(retexted.unwrap().digest(), DIGEST);
    let mut wider = schema();
    wider["properties"]["score"]["maximum"] = json!(10);
    let rebounded = Rubric::new(id("helpfulness"), "v3", TEXT, &wider, "code").unwrap();
    assert_ne!(rebounded.digest(), DIGEST);
    // Text moved across the boundary between text and schema is another digest too.
    let shifted = Rubric::new(
        id("helpfulness"),
        "v3",
        format!("{TEXT}{{"),
        &schema(),
        "code",
    );
    assert_ne!(shifted.unwrap().digest(), DIGEST);
}

#[test]
fn an_answer_not_valid_against_the_schema_is_refused_and_not_repaired() {
    let catalog = pinned("remote-large");
    for answer in [
        // What a repairing reader would coerce, unwrap, complete or trim.
        r#"{"score":"5","pass":true}"#,
        r#"{"score":5.5,"pass":true}"#,
        r#"{"score":9,"pass":true}"#,
        r#"{"score":0,"pass":true}"#,
        r#"{"score":5,"pass":"true"}"#,
        r#"{"score":5}"#,
        r#"{"score":5,"pass":true,"note":"clear"}"#,
        r#"[{"score":5,"pass":true}]"#,
        "```json\n{\"score\":5,\"pass\":true}\n```",
        r#"Here is my verdict: {"score":5,"pass":true}"#,
        r#"{"score":5,"pass":true} I hope this helps."#,
        r#"{"score":5,"pass":true"#,
        r#""{\"score\":5,\"pass\":true}""#,
        "null",
        "",
    ] {
        let ran = run(&catalog, &Far::answers(answer));
        assert_eq!(
            ran.verdict(),
            &Verdict {
                rubric_id: id("helpfulness"),
                rubric_version: "v3".into(),
                rubric_digest: DIGEST.into(),
                route_target_id: id("coding-secondary"),
                upstream_model: id(REPORTED),
                verdict_json: None,
                error_code: Some(ErrorCode::Protocol),
                call_record_id: id("req-1/helpfulness"),
            },
            "{answer}"
        );
        // The call was made and answered: its one record says so.
        assert_eq!(ran.records.len(), 1, "{answer}");
        assert_eq!(ran.records[0].kind, Kind::Verdict, "{answer}");
        assert_eq!(ran.records[0].outcome, Outcome::Succeeded, "{answer}");
    }
}

#[test]
fn the_same_answer_gives_byte_identical_verdict_json_on_two_runs() {
    let catalog = pinned("remote-large");
    let first = run_as(
        &catalog,
        &rubric(),
        SUBJECT,
        "req-1",
        1_000,
        &Far::answers(GOOD),
    );
    let again = run_as(
        &catalog,
        &rubric(),
        SUBJECT,
        "req-2",
        9_000,
        &Far::answers(GOOD),
    );
    // The same answer with its keys in another order, spaced and broken over lines.
    let respaced = Far::answers("{\n  \"pass\": true,\n  \"score\": 5\n}\n");
    let other = run_as(&catalog, &rubric(), SUBJECT, "req-3", 9_000, &respaced);
    let canonical = Some(r#"{"pass":true,"score":5}"#.to_owned());
    assert_eq!(first.verdict().verdict_json, canonical);
    assert_eq!(again.verdict().verdict_json, canonical);
    assert_eq!(other.verdict().verdict_json, canonical);
}

#[test]
fn canonical_verdict_json_sorts_keys_at_every_depth_and_keeps_array_order() {
    let nested = json!({
        "type": "object",
        "properties": {
            "findings": {"type": "array", "items": {
                "type": "object",
                "properties": {"where": {"type": "string"}, "severity": {"enum": ["low", "high"]}},
                "required": ["where", "severity"],
                "additionalProperties": false
            }},
            "pass": {"type": "boolean"}
        },
        "required": ["findings", "pass"],
        "additionalProperties": false
    });
    let rubric = Rubric::new(id("review"), "v1", TEXT, &nested, "code").unwrap();
    let answer = r#"{"pass":false,"findings":[{"where":"b \"q\"","severity":"high"},{"severity":"low","where":"a"}]}"#;
    let ran = run_as(
        &pinned("remote-large"),
        &rubric,
        SUBJECT,
        "req-1",
        0,
        &Far::answers(answer),
    );
    assert_eq!(
        ran.verdict().verdict_json.as_deref(),
        Some(
            r#"{"findings":[{"severity":"high","where":"b \"q\""},{"severity":"low","where":"a"}],"pass":false}"#
        )
    );
}

/// The request a model is sent, as its JSON bytes.
fn bytes(request: &TurnRequest) -> String {
    serde_json::to_string(request).unwrap()
}

#[test]
fn two_runs_over_one_rubric_and_subject_send_byte_identical_requests() {
    let catalog = pinned("remote-large");
    let first = run_as(
        &catalog,
        &rubric(),
        SUBJECT,
        "req-1",
        1_000,
        &Far::answers(GOOD),
    );
    // Another request id, another time, another answer, and a rubric built again.
    let slow = Far {
        takes_ms: 700,
        ..Far::answers(r#"{"score":1,"pass":false}"#)
    };
    let again = run_as(&catalog, &rubric(), SUBJECT, "req-2", 86_400_000, &slow);
    assert_eq!(first.seen.len(), 1);
    assert_eq!(again.seen.len(), 1);
    assert_eq!(bytes(&first.seen[0]), bytes(&again.seen[0]));
    // Those bytes hold the rubric's text, the subject, the canonical schema as a strict format
    // and the fixed sampling; nothing else.
    let expected = format!(
        r#"{{"model":"large","instructions":"{TEXT}","items":[{{"kind":"user-text","text":"{SUBJECT}"}}],"tools":[],"sampling":{{"temperature":0.0}},"controls":{{"output_format":{{"name":"{VERDICT_FORMAT_NAME}","schema":{CANONICAL_SCHEMA},"strict":true}}}}}}"#
    );
    assert_eq!(bytes(&first.seen[0]), expected);
    assert_eq!(VERDICT_FORMAT_NAME, "verdict");
    // Another subject or another rubric text is another request.
    let other = run_as(
        &catalog,
        &rubric(),
        "another subject",
        "req-1",
        1_000,
        &slow,
    );
    assert_ne!(bytes(&other.seen[0]), expected);
}

#[test]
fn sampling_is_fixed_only_where_the_target_declares_temperature() {
    let declared = run(&pinned("remote-large"), &Far::answers(GOOD));
    assert_eq!(declared.seen[0].sampling.temperature, Some(0.0));
    assert_eq!(declared.seen[0].sampling.top_p, None);
    // `local-small` declares no temperature: the request carries none and is still sent.
    let undeclared = run(&pinned("local-small"), &Far::answers(GOOD));
    assert_eq!(
        bytes(&undeclared.seen[0]),
        format!(
            r#"{{"model":"small","instructions":"{TEXT}","items":[{{"kind":"user-text","text":"{SUBJECT}"}}],"tools":[],"controls":{{"output_format":{{"name":"verdict","schema":{CANONICAL_SCHEMA},"strict":true}}}}}}"#
        )
    );
    assert_eq!(
        undeclared.verdict().route_target_id,
        id("coding-primary"),
        "the one target answered"
    );
}

#[test]
fn a_route_with_more_than_one_target_or_fallback_is_refused_before_any_call() {
    let mut one_target_with_fallback = CatalogDocument::parse(EXAMPLE).unwrap();
    one_target_with_fallback.targets.truncate(1);
    one_target_with_fallback.routes[0].fallback_enabled = true;
    for (catalog, targets, fallback_enabled) in [
        (two_targets(true), 2, true),
        // The second target could never be reached, and the route still names two.
        (two_targets(false), 2, false),
        (one_target_with_fallback.validate().unwrap(), 1, true),
    ] {
        let ran = run(&catalog, &Far::answers(GOOD));
        let refusal = RubricError::RouteNotPinned {
            targets,
            fallback_enabled,
        };
        assert_eq!(refusal.code(), "route-not-pinned");
        assert_eq!(ran.result, Err(JudgeError::Rubric(refusal)));
        assert_eq!(ran.seen, [], "no request was sent");
        assert_eq!(ran.records, [], "no record was written");
    }
}

#[test]
fn a_verdict_call_writes_one_call_record_of_kind_verdict() {
    let usage = Usage {
        input_tokens: Some(120),
        output_tokens: Some(12),
        ..Usage::default()
    };
    let far = Far {
        takes_ms: 250,
        usage: Some(usage.clone()),
        ..Far::answers(GOOD)
    };
    let ran = run(&pinned("remote-large"), &far);
    assert_eq!(ran.records.len(), 1);
    let record = &ran.records[0];
    assert_eq!(record.kind, Kind::Verdict);
    assert_eq!(record.id, ran.verdict().call_record_id);
    assert_eq!(record.id, id("req-1/helpfulness"));
    assert_eq!(record.route_id, id("coding"));
    assert_eq!(record.route_target_id, Some(id("coding-secondary")));
    assert_eq!(record.upstream_model, Some(id(REPORTED)));
    assert_eq!(record.outcome, Outcome::Succeeded);
    assert_eq!(record.attempts, 1);
    assert_eq!(record.started_at_unix_ms, 1_762_430_400_000);
    assert_eq!(record.latency_ms, Some(250));
    assert_eq!(record.usage, usage);
}

#[test]
fn a_failed_call_is_its_error_with_a_verdict_record_and_no_verdict() {
    let refused = Error::new(ErrorCode::Refused, "scripted").with_dispatch(Dispatch::Rejected);
    let far = Far {
        reply: Err(refused),
        ..Far::answers(GOOD)
    };
    let ran = run(&pinned("remote-large"), &far);
    let Err(JudgeError::Call(error)) = &ran.result else {
        panic!("expected the call's error, got {:?}", ran.result);
    };
    assert_eq!(error.code, ErrorCode::Refused);
    assert_eq!(ran.seen.len(), 1, "one attempt, on the one target");
    assert_eq!(ran.records.len(), 1);
    assert_eq!(ran.records[0].kind, Kind::Verdict);
    assert_eq!(ran.records[0].outcome, Outcome::Failed);
    assert_eq!(ran.records[0].error_code, Some(ErrorCode::Refused));
}

#[test]
fn an_answer_whose_far_side_reports_no_upstream_model_is_refused_and_still_recorded() {
    let far = Far {
        upstream_model: None,
        ..Far::answers(GOOD)
    };
    let ran = run(&pinned("remote-large"), &far);
    let Err(JudgeError::Call(error)) = &ran.result else {
        panic!("expected a refusal, got {:?}", ran.result);
    };
    assert_eq!(error.code, ErrorCode::Protocol);
    assert_eq!(ran.records.len(), 1);
    assert_eq!(ran.records[0].kind, Kind::Verdict);
    assert_eq!(ran.records[0].upstream_model, None);
}

#[test]
fn a_rubric_naming_no_route_of_the_catalog_is_refused_as_routing_refuses_it() {
    let rubric = Rubric::new(id("helpfulness"), "v3", TEXT, &schema(), "missing").unwrap();
    let ran = run_as(
        &pinned("remote-large"),
        &rubric,
        SUBJECT,
        "req-1",
        0,
        &Far::answers(GOOD),
    );
    let Err(JudgeError::Call(error)) = &ran.result else {
        panic!("expected a refusal, got {:?}", ran.result);
    };
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert_eq!(ran.seen, []);
    assert_eq!(ran.records, []);
}

#[test]
fn a_rubric_without_a_version_or_a_text_is_refused() {
    let rubric_of =
        |version: &str, text: &str| Rubric::new(id("r"), version, text, &schema(), "code");
    assert_eq!(rubric_of("", TEXT).unwrap_err(), RubricError::EmptyVersion);
    assert_eq!(rubric_of("v1", "").unwrap_err(), RubricError::EmptyText);
    assert_eq!(RubricError::EmptyVersion.code(), "empty-version");
    assert_eq!(RubricError::EmptyText.code(), "empty-text");
    let kept = rubric_of("v1", TEXT).unwrap();
    assert_eq!(
        (kept.id(), kept.version(), kept.text(), kept.route()),
        (&id("r"), "v1", TEXT, "code")
    );
}

#[test]
fn a_schema_the_library_cannot_hold_an_answer_to_is_refused_when_the_rubric_is_built() {
    for unsupported in [
        // A keyword outside the checked set, at the root and nested at every place a schema sits.
        json!({"type": "string", "pattern": "^a"}),
        json!({"type": "string", "format": "date"}),
        json!({"type": "string", "minLength": 1}),
        json!({"type": "array", "minItems": 1}),
        json!({"$ref": "#/$defs/verdict", "$defs": {"verdict": {"type": "object"}}}),
        json!({"oneOf": [{"type": "string"}]}),
        json!({"not": {"type": "string"}}),
        json!({"type": "object", "properties": {"a": {"type": "string", "pattern": "^a"}}}),
        json!({"type": "object", "additionalProperties": {"pattern": "^a"}}),
        json!({"type": "array", "items": {"pattern": "^a"}}),
        json!({"anyOf": [{"type": "string"}, {"pattern": "^a"}]}),
        // A keyword of the set with a value it cannot have.
        json!({"type": "text"}),
        json!({"type": ["string", 1]}),
        json!({"type": []}),
        json!({"type": "object", "properties": []}),
        json!({"type": "object", "properties": {"a": true}}),
        json!({"type": "object", "required": "a"}),
        json!({"type": "object", "required": [1]}),
        json!({"type": "object", "additionalProperties": "no"}),
        json!({"type": "array", "items": [{"type": "string"}]}),
        json!({"enum": "a"}),
        json!({"type": "integer", "minimum": "1"}),
        json!({"type": "integer", "maximum": null}),
        json!({"anyOf": []}),
        json!({"anyOf": {"type": "string"}}),
        json!({"type": "string", "description": 1}),
        // Not a schema object at all.
        json!(true),
        json!([]),
        json!("object"),
    ] {
        let refused = Rubric::new(id("r"), "v1", TEXT, &unsupported, "code");
        assert_eq!(
            refused.unwrap_err(),
            RubricError::UnsupportedSchema,
            "{unsupported}"
        );
    }
    assert_eq!(RubricError::UnsupportedSchema.code(), "unsupported-schema");
    // A schema nested deeper than the bound is refused rather than walked.
    let mut deep = json!({"type": "string"});
    for _ in 0..200 {
        deep = json!({"type": "array", "items": deep});
    }
    let refused = Rubric::new(id("r"), "v1", TEXT, &deep, "code");
    assert_eq!(refused.unwrap_err(), RubricError::UnsupportedSchema);
}

#[test]
fn every_keyword_of_the_checked_set_holds_the_answer_to_it() {
    let catalog = pinned("remote-large");
    // Per schema: the answers it admits, then the answers it refuses.
    let cases: [(Value, &[&str], &[&str]); 12] = [
        (
            json!({"type": "string"}),
            &[r#""a""#, r#""""#],
            &["1", "null", "true", "[]", "{}"],
        ),
        (
            json!({"type": "integer"}),
            &["1", "-3", "0", "2.0"],
            &["1.5", r#""1""#, "null", "true"],
        ),
        (
            json!({"type": "number", "minimum": -1.5, "maximum": 2}),
            &["-1.5", "0", "2", "1.25"],
            &["-1.6", "2.01", r#""1""#],
        ),
        (
            json!({"type": ["boolean", "null"]}),
            &["true", "false", "null"],
            &["0", r#""true""#],
        ),
        (
            json!({"type": "array", "items": {"type": "integer"}}),
            &["[]", "[1,2,3]"],
            &[r#"[1,"2"]"#, "1", "{}"],
        ),
        (
            json!({"enum": ["low", "high", 3, null]}),
            &[r#""low""#, r#""high""#, "3", "null"],
            &[r#""medium""#, "4", r#""3""#, "false"],
        ),
        (
            json!({"const": {"a": [1, 2]}}),
            &[r#"{"a":[1,2]}"#],
            &[r#"{"a":[2,1]}"#, r#"{"a":[1,2],"b":1}"#, "null"],
        ),
        (
            json!({"anyOf": [{"type": "string"}, {"type": "integer", "minimum": 10}]}),
            &[r#""a""#, "10", "11"],
            &["9", "null", "10.5"],
        ),
        (
            json!({"type": "object", "properties": {"a": {"type": "integer"}}}),
            // Without `required` a property may be absent; without
            // `additionalProperties` another key may be present.
            &["{}", r#"{"a":1}"#, r#"{"a":1,"b":"x"}"#],
            &[r#"{"a":"1"}"#, "[]"],
        ),
        (
            json!({"type": "object", "properties": {"a": {"type": "integer"}}, "required": ["a", "b"]}),
            &[r#"{"a":1,"b":null}"#],
            &[r#"{"a":1}"#, r#"{"b":1}"#, "{}"],
        ),
        (
            json!({"type": "object", "properties": {"a": {}}, "additionalProperties": {"type": "boolean"}}),
            &[r#"{"a":"anything"}"#, r#"{"b":true,"c":false}"#],
            &[r#"{"b":1}"#],
        ),
        (
            json!({"title": "t", "description": "d", "type": "null"}),
            &["null"],
            &["0", r#""""#],
        ),
    ];
    for (schema, admitted, refused) in cases {
        let rubric = Rubric::new(id("r"), "v1", TEXT, &schema, "code").unwrap();
        for answer in admitted {
            let ran = run_as(
                &catalog,
                &rubric,
                SUBJECT,
                "req-1",
                0,
                &Far::answers(answer),
            );
            let verdict = ran.verdict();
            assert_eq!(verdict.error_code, None, "{schema} admits {answer}");
            let held: Value = serde_json::from_str(verdict.verdict_json.as_ref().unwrap()).unwrap();
            let sent: Value = serde_json::from_str(answer).unwrap();
            assert_eq!(held, sent, "{schema} keeps {answer}");
        }
        for answer in refused {
            let ran = run_as(
                &catalog,
                &rubric,
                SUBJECT,
                "req-1",
                0,
                &Far::answers(answer),
            );
            let verdict = ran.verdict();
            assert_eq!(
                (verdict.verdict_json.as_ref(), verdict.error_code),
                (None, Some(ErrorCode::Protocol)),
                "{schema} refuses {answer}"
            );
        }
    }
}

#[test]
fn a_verdict_round_trips_through_its_json_form() {
    let ran = run(&pinned("remote-large"), &Far::answers(GOOD));
    let text = serde_json::to_string(ran.verdict()).unwrap();
    assert_eq!(
        text,
        format!(
            r#"{{"rubric_id":"helpfulness","rubric_version":"v3","rubric_digest":"{DIGEST}","route_target_id":"coding-secondary","upstream_model":"{REPORTED}","verdict_json":"{{\"pass\":true,\"score\":5}}","error_code":null,"call_record_id":"req-1/helpfulness"}}"#
        )
    );
    assert_eq!(
        &serde_json::from_str::<Verdict>(&text).unwrap(),
        ran.verdict()
    );
}
