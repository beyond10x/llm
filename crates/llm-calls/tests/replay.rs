//! Offline replay: a corpus read whole, each case sent to a scripted candidate through the real
//! `record_turn`, a scripted clock, a price book with two prices and an in-memory record sink.
//! No projection, no network.

use llm_calls::{
    CallLog, CallRecord, CallSink, Candidate, CaseAsk, CaseCall, CaseResult, Clock, Corpus, Kind,
    MAX_CASES, MAX_CORPUS_BYTES, MAX_CORPUS_DEPTH, MAX_RUBRIC_ID_BYTES, Outcome,
    REPLAY_CLASSIFIER_ID, Recording, Replay, ReplayRun, Role, replay,
};
use llm_core::{
    BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item, Model, Provenance,
    StopReason, StreamEvent, StreamSink, TurnObservation, TurnOutcome, TurnRequest, Usage,
};
use llm_cost::budget::BudgetError;
use llm_cost::{Amount, Currency, ModelPrice, PriceBook, PriceDocument, Rate};
use llm_routing::{
    Catalog, CatalogDocument, FallbackPolicy, Models, Ports, Route, RouteTarget, Selection,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    future::Future,
    sync::atomic::{AtomicU64, AtomicUsize, Ordering},
    task::{Context, Poll, Waker},
    time::Duration,
};

const EXAMPLE: &str = include_str!("../../../examples/catalog.toml");

const INSTRUCTIONS: &str = "instructions-marker-heron-4410";
const INPUT: &str = "input-marker-otter-9052";
const SUBJECT: &str = "subject-marker-lynx-2377";
const RUBRIC_TEXT: &str = "rubric-marker-crane-6608";
const PROMPT: &str = "prompt-marker-finch-1953";
const RECORDED_TEXT: &str = "recorded-answer-marker-ibis-7741 \u{e9}\u{4e16}";
const CANDIDATE_TEXT: &str = "candidate-answer-marker-newt-3086";

fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}

fn amount(value: &str) -> Amount {
    Amount::parse(value).unwrap()
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

/// Route `coding` answers on `local-small`; the candidate route `coding-candidate` (alias
/// `code-candidate`) answers on `remote-large`.
fn catalog() -> Catalog {
    catalog_of(llm_routing::RouteKind::Turn)
}

/// The catalog with its candidate route declared as `kind`.
fn catalog_of(kind: llm_routing::RouteKind) -> Catalog {
    let mut doc = CatalogDocument::parse(EXAMPLE).unwrap();
    doc.routes[0].fallback_enabled = false;
    doc.routes.push(Route {
        id: id("coding-candidate"),
        alias: id("code-candidate"),
        fallback_enabled: false,
        kind,
    });
    doc.targets.push(RouteTarget {
        id: id("candidate-large"),
        route_id: id("coding-candidate"),
        serving_model_id: id("remote-large"),
        position: 0,
    });
    doc.validate().unwrap()
}

/// `local-small` at 1 and 2 per 1000 tokens, `remote-large` at 3 and 5: a call of 10 input and
/// 5 output tokens costs 0.055 on the candidate's target and 0.02 on the other.
fn prices(catalog: &Catalog, currency: &str) -> PriceBook {
    let price = |price_id: &str, serving: &str, input: &str, output: &str| ModelPrice {
        id: id(price_id),
        serving_model_id: id(serving),
        binding_revision: catalog
            .binding(&id(serving))
            .unwrap()
            .provenance()
            .binding_revision
            .clone(),
        upstream_model: id("example/Any-Model"),
        input: Some(Rate {
            amount: amount(input),
            per_units: 1000,
        }),
        cached_input: None,
        cache_creation_input: None,
        output: Some(Rate {
            amount: amount(output),
            per_units: 1000,
        }),
    };
    PriceBook::from_document(PriceDocument::new(
        id("prices-r4"),
        "fixture price list",
        1,
        Currency::new(currency).unwrap(),
        vec![
            price("price-small", "local-small", "1", "2"),
            price("price-large", "remote-large", "3", "5"),
        ],
    ))
    .unwrap()
}

fn usage(input: u64, output: u64) -> Usage {
    Usage {
        input_tokens: Some(input),
        output_tokens: Some(output),
        cached_input_tokens: Some(0),
        cache_creation_input_tokens: Some(0),
        reasoning_output_tokens: None,
    }
}

/// A clock that stands still and counts how often it is read.
struct Ticks {
    reads: AtomicUsize,
    unix_ms: AtomicU64,
}

impl Ticks {
    const fn new() -> Self {
        Self {
            reads: AtomicUsize::new(0),
            unix_ms: AtomicU64::new(1_762_344_000_000),
        }
    }
}

impl Clock for Ticks {
    fn unix_ms(&self) -> u64 {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.unix_ms.load(Ordering::SeqCst)
    }
    fn monotonic(&self) -> Duration {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Duration::ZERO
    }
}

/// What the scripted candidate does with one case.
#[derive(Clone)]
enum Step {
    /// One call that answers `text`, reporting 10 input and 5 output tokens.
    Answer(&'static str),
    /// One call that answers, whose answer the candidate holds to be none the comparison takes.
    Unusable,
    /// One call that answers and reports no usage, so its cost is not known.
    Unpriced(&'static str),
    /// One call the far side accepted and failed, reporting 10 input and 5 output tokens.
    Fail(ErrorCode),
    /// No call: the candidate refuses the case itself.
    Refuse(ErrorCode),
}

/// Answers one call as its step says; without a step, with the case's recorded answer.
struct OneCall {
    provenance: Provenance,
    capabilities: Capabilities,
    recorded: String,
    step: Option<Step>,
}

impl Model for OneCall {
    fn provenance(&self) -> &Provenance {
        &self.provenance
    }
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }
    fn turn<'a>(
        &'a self,
        _: &'a TurnRequest,
        sink: &'a mut dyn StreamSink,
        _: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        Box::pin(async move {
            let mut observation = TurnObservation::new(self.provenance.clone());
            observation.upstream_model = Some(id("example/Any-Model"));
            observation.final_usage = true;
            let text = match &self.step {
                None => {
                    observation.usage = Some(usage(10, 5));
                    self.recorded.clone()
                }
                Some(Step::Answer(text)) => {
                    observation.usage = Some(usage(10, 5));
                    (*text).to_owned()
                }
                Some(Step::Unusable) => {
                    observation.usage = Some(usage(10, 5));
                    "unusable".to_owned()
                }
                Some(Step::Unpriced(text)) => (*text).to_owned(),
                Some(Step::Fail(code)) => {
                    observation.usage = Some(usage(10, 5));
                    return Err(Error::new(*code, "failure-marker-shrew-5521")
                        .with_dispatch(Dispatch::Accepted)
                        .with_retriable(false)
                        .with_observation(observation));
                }
                Some(Step::Refuse(_)) => unreachable!("a refused case makes no call"),
            };
            sink.emit(StreamEvent::TextDelta { text: text.clone() })
                .await?;
            Ok(TurnOutcome {
                stop_reason: StopReason::EndTurn,
                items: vec![Item::assistant(text)],
                observation,
            })
        })
    }
}

impl Models for OneCall {
    fn model(&self, _: &Id) -> Option<&dyn Model> {
        Some(self)
    }
}

struct Discard;

impl StreamSink for Discard {
    fn emit(&mut self, _: StreamEvent) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async { Ok(()) })
    }
}

/// Passes each record on under the case's kind, as `llm-decide` does for its calls.
struct AsKind<'s>(&'s dyn CallSink, Kind);

impl CallSink for AsKind<'_> {
    fn record(&self, mut record: CallRecord) {
        record.kind = self.1;
        self.0.record(record);
    }
}

/// A candidate that sends each case as one routed, recorded turn and answers with the text the
/// scripted model returned: the label of a classification, the JSON of a verdict, the text of a
/// turn. A case without a step answers its own recorded answer.
struct Scripted<'c> {
    catalog: &'c Catalog,
    steps: BTreeMap<&'static str, Step>,
    asked: AtomicUsize,
}

impl<'c> Scripted<'c> {
    fn new(catalog: &'c Catalog, steps: &[(&'static str, Step)]) -> Self {
        Self {
            catalog,
            steps: steps.iter().cloned().collect(),
            asked: AtomicUsize::new(0),
        }
    }
    fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }
}

impl Candidate for Scripted<'_> {
    fn answer<'a>(&'a self, call: CaseCall<'a>) -> BoxFuture<'a, Result<Option<String>, Error>> {
        Box::pin(async move {
            self.asked.fetch_add(1, Ordering::SeqCst);
            let step = self.steps.get(call.case.id.as_str()).cloned();
            if let Some(Step::Refuse(code)) = step {
                return Err(Error::new(code, "the candidate refuses the case"));
            }
            let unusable = matches!(step, Some(Step::Unusable));
            let binding = self.catalog.binding(&id("remote-large")).unwrap();
            let model = OneCall {
                provenance: binding.provenance().clone(),
                capabilities: binding.capabilities().clone(),
                recorded: call.case.recorded.clone(),
                step,
            };
            let request = TurnRequest::new(call.route, vec![Item::user("replayed")]);
            let cancel = Cancel::new();
            let pause = |_: Duration| -> BoxFuture<'static, ()> { Box::pin(async {}) };
            let mut admit = |_: &Selection<'_>| Ok(());
            let kinded = AsKind(call.recording.sink, call.case.kind());
            let run = llm_calls::record_turn(
                self.catalog,
                &request,
                Some(call.case.input_tokens),
                FallbackPolicy {
                    max_attempts: 1,
                    ..FallbackPolicy::default()
                },
                Ports {
                    models: &model,
                    admit: &mut admit,
                    sink: &mut Discard,
                    cancel: &cancel,
                    pause: &pause,
                },
                llm_calls::CallIdentity::primary(call.request_id.clone()),
                Recording {
                    sink: &kinded,
                    ..call.recording
                },
            )
            .await?;
            let outcome = run.result?;
            if unusable {
                return Ok(None);
            }
            Ok(outcome.items.iter().find_map(|item| match item {
                Item::AssistantText { text } => Some(text.clone()),
                _ => None,
            }))
        })
    }
}

fn turn_case(case_id: &str, recorded: &str) -> Value {
    json!({
        "id": case_id, "kind": "turn", "input_tokens": 100,
        "turn": {"format": "llm.turn/3", "request": {
            "model": "recorded-route", "instructions": INSTRUCTIONS,
            "items": [{"kind": "user-text", "text": PROMPT}], "tools": [],
            "max_output_tokens": 128}},
        "recorded": {"text": recorded}
    })
}

fn classification_case(case_id: &str, recorded: &str) -> Value {
    json!({
        "id": case_id, "kind": "classification", "input_tokens": 100,
        "label_set": {"id": "intent", "version": "3", "labels": ["refund", "Refund", "other"]},
        "instructions": INSTRUCTIONS, "input": INPUT,
        "recorded": {"label": recorded}
    })
}

fn verdict_case(case_id: &str, recorded: &Value) -> Value {
    json!({
        "id": case_id, "kind": "verdict", "input_tokens": 100,
        "rubric": {"id": "helpful", "version": "2", "text": RUBRIC_TEXT,
            "schema": {"type": "object"}},
        "subject": SUBJECT,
        "recorded": {"verdict": recorded}
    })
}

fn document(cases: &[Value]) -> Vec<u8> {
    serde_json::to_vec(&json!({"format": "llm.corpus/1", "cases": cases})).unwrap()
}

fn corpus(cases: &[Value]) -> Corpus {
    Corpus::parse(&document(cases)).unwrap()
}

fn run_of(cap: &str, reserve: &str) -> Replay {
    Replay {
        id: id("run-1"),
        candidate_route_id: id("coding-candidate"),
        ledger_id: id("replay-spend"),
        currency: Currency::new("USD").unwrap(),
        money_cap: amount(cap),
        reserve: amount(reserve),
    }
}

/// One run of `cases` under `cap` with 0.1 held per case; the run, every record and how many
/// cases the candidate was asked.
fn played(
    cases: &[Value],
    steps: &[(&'static str, Step)],
    cap: &str,
) -> (ReplayRun, Vec<CallRecord>, usize) {
    let catalog = catalog();
    let prices = prices(&catalog, "USD");
    let clock = Ticks::new();
    let log = CallLog::new();
    let candidate = Scripted::new(&catalog, steps);
    let run = block_on(replay(
        &catalog,
        &corpus(cases),
        &run_of(cap, "0.1"),
        &candidate,
        Recording {
            sink: &log,
            clock: &clock,
            prices: Some(&prices),
        },
    ))
    .unwrap();
    (run, log.records(), candidate.asked())
}

fn results(run: &ReplayRun) -> Vec<(&str, CaseResult, Option<ErrorCode>)> {
    run.results
        .iter()
        .map(|case| (case.case_id.as_str(), case.result, case.error_code))
        .collect()
}

#[test]
fn a_corpus_reads_each_kind_of_case_in_the_order_given() {
    let mut second = classification_case("c-2", "Refund");
    second["primary_record_id"] = json!("call-77");
    let corpus = corpus(&[
        turn_case("c-1", RECORDED_TEXT),
        second,
        verdict_case("c-3", &json!({"score": 4, "grounds": [1.0, 2]})),
    ]);
    let cases = corpus.cases();
    assert_eq!(
        cases
            .iter()
            .map(|case| (case.id.as_str(), case.kind(), case.input_tokens))
            .collect::<Vec<_>>(),
        [
            ("c-1", Kind::Turn, 100),
            ("c-2", Kind::Classification, 100),
            ("c-3", Kind::Verdict, 100)
        ]
    );
    let CaseAsk::Turn { request } = &cases[0].ask else {
        panic!("the first case is a turn");
    };
    assert_eq!(request.instructions, INSTRUCTIONS);
    assert_eq!(request.items, [Item::user(PROMPT)]);
    assert_eq!(cases[0].recorded, RECORDED_TEXT);
    assert_eq!(cases[0].primary_record_id, None);
    assert_eq!(
        cases[1].ask,
        CaseAsk::Classification {
            label_set_id: id("intent"),
            label_set_version: "3".to_owned(),
            labels: vec!["refund".to_owned(), "Refund".to_owned(), "other".to_owned()],
            instructions: INSTRUCTIONS.to_owned(),
            input: INPUT.to_owned(),
        }
    );
    assert_eq!(cases[1].recorded, "Refund");
    assert_eq!(cases[1].primary_record_id, Some(id("call-77")));
    assert_eq!(
        cases[2].ask,
        CaseAsk::Verdict {
            rubric_id: id("helpful"),
            rubric_version: "2".to_owned(),
            rubric_text: RUBRIC_TEXT.to_owned(),
            schema: json!({"type": "object"}),
            subject: SUBJECT.to_owned(),
        }
    );
    // The recorded verdict is held as canonical JSON: keys sorted, no whitespace.
    assert_eq!(cases[2].recorded, r#"{"grounds":[1.0,2],"score":4}"#);
}

#[test]
fn classification_instructions_are_empty_when_the_case_gives_none() {
    let mut case = classification_case("c-1", "other");
    case.as_object_mut().unwrap().remove("instructions");
    let corpus = corpus(&[case]);
    let CaseAsk::Classification { instructions, .. } = &corpus.cases()[0].ask else {
        panic!("the case is a classification");
    };
    assert_eq!(instructions, "");
}

/// `text` with its first occurrence of `from` replaced by `to`.
fn with(text: &str, from: &str, to: &str) -> String {
    assert!(text.contains(from), "{from} is not in the fixture");
    text.replacen(from, to, 1)
}

#[test]
#[expect(clippy::too_many_lines, reason = "one table of refusals")]
fn a_corpus_is_refused_whole_naming_the_member_by_its_path() {
    // Written out, so each row below changes bytes whose place in the document is known.
    let whole = format!(
        concat!(
            r#"{{"format":"llm.corpus/1","cases":["#,
            r#"{{"id":"c-1","kind":"turn","input_tokens":100,"turn":{{"format":"llm.turn/3","#,
            r#""request":{{"model":"recorded-route","instructions":"{instructions}","#,
            r#""items":[{{"kind":"user-text","text":"{prompt}"}}],"tools":[],"#,
            r#""max_output_tokens":128}}}},"recorded":{{"text":"one"}}}},"#,
            r#"{{"id":"c-2","kind":"classification","input_tokens":100,"label_set":"#,
            r#"{{"id":"intent","version":"3","labels":["refund","Refund","other"]}},"#,
            r#""instructions":"{instructions}","input":"{input}","recorded":{{"label":"refund"}}}},"#,
            r#"{{"id":"c-3","kind":"verdict","input_tokens":100,"rubric":{{"id":"helpful","#,
            r#""version":"2","text":"{rubric}","schema":{{"type":"object"}}}},"#,
            r#""subject":"{subject}","recorded":{{"verdict":{{"score":4}}}}}}]}}"#,
        ),
        instructions = INSTRUCTIONS,
        prompt = PROMPT,
        input = INPUT,
        rubric = RUBRIC_TEXT,
        subject = SUBJECT,
    );
    assert_eq!(Corpus::parse(whole.as_bytes()).unwrap().cases().len(), 3);
    let cut = whole.len() - 1;
    let inside = whole.find(r#","kind":"turn""#).unwrap();
    let invalid = ErrorCode::InvalidRequest;
    let rows: Vec<(&str, String, &str)> = vec![
        (
            "a file cut before its last byte",
            whole[..cut].to_owned(),
            "$",
        ),
        (
            "a file cut inside its first case",
            whole[..inside].to_owned(),
            "cases[0]",
        ),
        ("bytes after the document", format!("{whole} x"), "$"),
        ("a second document", format!("{whole}{whole}"), "$"),
        ("an empty file", String::new(), "$"),
        ("an array", "[]".to_owned(), "$"),
        (
            "the format given twice",
            with(&whole, "{", r#"{"format":"llm.corpus/1","#),
            "format",
        ),
        (
            "a member of a turn given twice",
            with(&whole, r#""tools":[]"#, r#""tools":[],"tools":[]"#),
            "cases[0].turn.*.*",
        ),
        (
            "a member of a verdict schema given twice",
            with(
                &whole,
                r#"{"type":"object"}"#,
                r#"{"type":"object","type":"array"}"#,
            ),
            "cases[2].rubric.schema.*",
        ),
        (
            "a later format version",
            with(&whole, "llm.corpus/1", "llm.corpus/2"),
            "format",
        ),
        (
            "a format that is no text",
            with(&whole, r#""llm.corpus/1""#, "1"),
            "format",
        ),
        (
            "no format",
            with(&whole, r#""format":"llm.corpus/1","#, ""),
            "format",
        ),
        (
            "a member the document does not declare",
            with(&whole, "{", r#"{"comment":"x","#),
            "*",
        ),
        (
            "a case kind the format does not know",
            with(
                &whole,
                r#""kind":"classification""#,
                r#""kind":"embed-batch""#,
            ),
            "cases[1].kind",
        ),
        (
            "a member of another kind on a turn case",
            with(&whole, r#""kind":"turn""#, r#""kind":"turn","subject":"x""#),
            "cases[0].subject",
        ),
        (
            "a case without its recorded answer",
            with(&whole, r#","recorded":{"text":"one"}"#, ""),
            "cases[0].recorded",
        ),
        (
            "a recorded answer of another kind",
            with(&whole, r#"{"text":"one"}"#, r#"{"label":"one"}"#),
            "cases[0].recorded.label",
        ),
        (
            "a recorded answer with two members",
            with(
                &whole,
                r#"{"text":"one"}"#,
                r#"{"text":"one","label":"one"}"#,
            ),
            "cases[0].recorded.label",
        ),
        (
            "a recorded label outside the label set",
            with(&whole, r#"{"label":"refund"}"#, r#"{"label":"refunds"}"#),
            "cases[1].recorded.label",
        ),
        (
            "a label that is no text",
            with(&whole, r#""Refund","other""#, r#"7,"other""#),
            "cases[1].label_set.labels[1]",
        ),
        (
            "a case id given twice",
            with(&whole, r#""id":"c-3""#, r#""id":"c-1""#),
            "cases[2].id",
        ),
        (
            "an empty case id",
            with(&whole, r#""id":"c-2""#, r#""id":"""#),
            "cases[1].id",
        ),
        (
            "a case id that holds a slash",
            with(&whole, r#""id":"c-2""#, r#""id":"c/2""#),
            "cases[1].id",
        ),
        (
            "a rubric id one byte past its bound",
            with(
                &whole,
                r#""id":"helpful""#,
                &format!(r#""id":"{}""#, "r".repeat(MAX_RUBRIC_ID_BYTES + 1)),
            ),
            "cases[2].rubric.id",
        ),
        (
            "an input-token estimate with a fraction",
            with(&whole, r#""input_tokens":100"#, r#""input_tokens":100.5"#),
            "cases[0].input_tokens",
        ),
        (
            "a negative input-token estimate",
            with(&whole, r#""input_tokens":100"#, r#""input_tokens":-1"#),
            "cases[0].input_tokens",
        ),
        (
            "a turn document of an unknown version",
            with(&whole, "llm.turn/3", "llm.turn/9"),
            "cases[0].turn",
        ),
        (
            "a turn whose request llm-core refuses",
            with(
                &whole,
                r#""max_output_tokens":128"#,
                r#""max_output_tokens":0"#,
            ),
            "cases[0].turn",
        ),
        (
            "a rubric without its schema",
            with(&whole, r#","schema":{"type":"object"}"#, ""),
            "cases[2].rubric.schema",
        ),
        (
            "no case",
            r#"{"format":"llm.corpus/1","cases":[]}"#.to_owned(),
            "cases",
        ),
        (
            "cases that are no array",
            r#"{"format":"llm.corpus/1","cases":{}}"#.to_owned(),
            "cases",
        ),
    ];
    for (name, bytes, path) in rows {
        let error = Corpus::parse(bytes.as_bytes()).expect_err(name);
        assert_eq!((error.code, error.path.as_str()), (invalid, path), "{name}");
        // The refusal names a place and a fixed reason, never a text of the corpus.
        let shown = error.to_string();
        for marker in [INSTRUCTIONS, INPUT, SUBJECT, RUBRIC_TEXT, PROMPT] {
            assert!(!shown.contains(marker), "{name}: {shown}");
        }
    }
}

/// A member name the caller chose, where the format defines none.
const NAME: &str = "caller-name-marker-wren-3391";

#[test]
#[expect(clippy::too_many_lines, reason = "one table of places")]
fn a_refusal_path_shows_the_names_the_format_defines_and_no_name_the_caller_chose() {
    let text = String::from_utf8(document(&[
        turn_case("c-1", "one"),
        classification_case("c-2", "other"),
        verdict_case("c-3", &json!({"score": 4})),
    ]))
    .unwrap();
    assert!(Corpus::parse(text.as_bytes()).is_ok());
    // `NAME` given twice, so the one-pass reader refuses at it.
    let twice = format!(r#""{NAME}":1,"{NAME}":2"#);
    let after = |member: &str| with(&text, member, &format!("{member},{twice}"));
    let caller: Vec<(&str, String, &str)> = vec![
        (
            "in the document",
            with(&text, "{", &format!("{{{twice},")),
            "*",
        ),
        ("in a case", after(r#""kind":"turn""#), "cases[0].*"),
        (
            "in a turn document",
            after(r#""format":"llm.turn/3""#),
            "cases[0].turn.*",
        ),
        (
            "in the arguments of a tool of a turn",
            with(
                &text,
                r#""tools":[]"#,
                &format!(r#""tools":[{{"{NAME}":{{{twice}}}}}]"#),
            ),
            "cases[0].turn.*.*[0].*.*",
        ),
        (
            "in a label set",
            after(r#""version":"3""#),
            "cases[1].label_set.*",
        ),
        (
            "in a rubric",
            after(r#""version":"2""#),
            "cases[2].rubric.*",
        ),
        (
            "in a verdict schema",
            with(&text, r#"{"type":"object"}"#, &format!("{{{twice}}}")),
            "cases[2].rubric.schema.*",
        ),
        (
            "deeper in a verdict schema",
            with(
                &text,
                r#"{"type":"object"}"#,
                &format!(r#"{{"properties":{{"{NAME}":{{{twice}}}}}}}"#),
            ),
            "cases[2].rubric.schema.*.*.*",
        ),
        (
            "in a recorded answer",
            after(r#""verdict":{"score":4}"#),
            "cases[2].recorded.*",
        ),
        (
            "in a recorded verdict",
            with(&text, r#"{"score":4}"#, &format!("{{{twice}}}")),
            "cases[2].recorded.verdict.*",
        ),
        (
            "in an array of a recorded verdict",
            with(
                &text,
                r#"{"score":4}"#,
                &format!(r#"{{"score":[{{{twice}}}]}}"#),
            ),
            "cases[2].recorded.verdict.*[0].*",
        ),
        (
            "given once in the document",
            with(&text, "{", &format!(r#"{{"{NAME}":1,"#)),
            "*",
        ),
        (
            "given once in a case",
            with(
                &text,
                r#""kind":"classification""#,
                &format!(r#""kind":"classification","{NAME}":1"#),
            ),
            "cases[1].*",
        ),
        (
            "given once in a recorded answer",
            with(
                &text,
                r#"{"text":"one"}"#,
                &format!(r#"{{"{NAME}":"one"}}"#),
            ),
            "cases[0].recorded.*",
        ),
    ];
    for (name, bytes, path) in caller {
        let error = Corpus::parse(bytes.as_bytes()).expect_err(name);
        assert_eq!(error.path, path, "{name}");
        assert!(!error.to_string().contains(NAME), "{name}: {error}");
    }
    // A name the format defines is shown where it defines it, and nowhere else.
    let format: Vec<(&str, String, &str)> = vec![
        (
            "the kind of a case",
            with(&text, r#""kind":"turn""#, r#""kind":"turn","kind":"turn""#),
            "cases[0].kind",
        ),
        (
            "the version of a label set",
            with(&text, r#""version":"3""#, r#""version":"3","version":"3""#),
            "cases[1].label_set.version",
        ),
        (
            "the version of a rubric",
            with(&text, r#""version":"2""#, r#""version":"2","version":"2""#),
            "cases[2].rubric.version",
        ),
        (
            "a recorded verdict",
            with(
                &text,
                r#""verdict":{"score":4}"#,
                r#""verdict":{"score":4},"verdict":1"#,
            ),
            "cases[2].recorded.verdict",
        ),
        (
            "a case's name in the document",
            with(&text, "{", r#"{"rubric":1,"rubric":2,"#),
            "*",
        ),
        (
            "a rubric's name in a label set",
            with(
                &text,
                r#""version":"3""#,
                r#""version":"3","schema":1,"schema":2"#,
            ),
            "cases[1].label_set.*",
        ),
        (
            "a case's name in a verdict schema",
            with(&text, r#"{"type":"object"}"#, r#"{"id":1,"id":2}"#),
            "cases[2].rubric.schema.*",
        ),
        (
            "a case's name in a recorded verdict",
            with(&text, r#"{"score":4}"#, r#"{"kind":1,"kind":2}"#),
            "cases[2].recorded.verdict.*",
        ),
    ];
    for (name, bytes, path) in format {
        let error = Corpus::parse(bytes.as_bytes()).expect_err(name);
        assert_eq!(error.path, path, "{name}");
    }
}

#[test]
fn a_case_id_holds_no_slash_and_a_rubric_id_fits_under_the_digest_request_id() {
    // Other punctuation is taken in a case id.
    let punctuated = "a.b:c_d-e\\g|h";
    let taken = corpus(&[turn_case(punctuated, "one")]);
    assert_eq!(taken.cases()[0].id.as_str(), punctuated);
    for refused in ["a/classify", "/", "a/", "/a"] {
        let error = Corpus::parse(&document(&[turn_case(refused, "one")])).unwrap_err();
        assert_eq!(
            (error.code, error.path.as_str(), error.reason),
            (
                ErrorCode::InvalidRequest,
                "cases[0].id",
                "a case id that holds a slash"
            ),
            "{refused}"
        );
    }
    // `replay-sha256-<hex>/<rubric id>` is an id up to the bound of the rubric id.
    let digest_form = "replay-sha256-".len() + 64;
    assert_eq!(digest_form + 1 + MAX_RUBRIC_ID_BYTES, 256);
    let rubric = |length: usize| {
        let mut case = verdict_case("c-1", &json!(true));
        case["rubric"]["id"] = json!("r".repeat(length));
        document(&[case])
    };
    assert!(Corpus::parse(&rubric(MAX_RUBRIC_ID_BYTES)).is_ok());
    let over = Corpus::parse(&rubric(MAX_RUBRIC_ID_BYTES + 1)).unwrap_err();
    assert_eq!(
        (over.code, over.path.as_str(), over.reason),
        (
            ErrorCode::InvalidRequest,
            "cases[0].rubric.id",
            "a rubric id of more than 177 bytes"
        )
    );
}

#[test]
fn a_recorded_verdict_is_taken_as_given_and_not_held_to_its_schema() {
    let mut case = verdict_case("c-1", &json!("no object"));
    case["rubric"]["schema"] = json!({
        "type": "object", "properties": {"score": {"type": "integer"}},
        "required": ["score"], "additionalProperties": false
    });
    let corpus = corpus(&[case]);
    assert_eq!(corpus.cases()[0].recorded, r#""no object""#);
}

/// A corpus of one classification case whose input is `fill` repeated until the document is
/// `bytes` long.
fn sized(bytes: usize, fill: char) -> Vec<u8> {
    let empty = document(&[classification_case("c-1", "other")]).len() - INPUT.len();
    let room = bytes - empty;
    assert_eq!(
        room % fill.len_utf8(),
        0,
        "the fill does not end on the bound"
    );
    let input: String = std::iter::repeat_n(fill, room / fill.len_utf8()).collect();
    let mut case = classification_case("c-1", "other");
    case["input"] = json!(input);
    let document = document(&[case]);
    assert_eq!(document.len(), bytes);
    document
}

#[test]
fn a_corpus_is_bounded_in_bytes_not_in_characters() {
    // One-byte characters up to the bound, and one byte past it.
    assert!(Corpus::parse(&sized(MAX_CORPUS_BYTES, 'a')).is_ok());
    let over = Corpus::parse(&sized(MAX_CORPUS_BYTES + 1, 'a')).unwrap_err();
    assert_eq!((over.code, over.path.as_str()), (ErrorCode::TooLarge, "$"));
    // Two-byte characters ending on the bound, and one character more: fewer characters than
    // the one-byte corpus the bound admits, and still past it.
    let empty = document(&[classification_case("c-1", "other")]).len() - INPUT.len();
    let even = MAX_CORPUS_BYTES - (MAX_CORPUS_BYTES - empty) % 2;
    assert!(Corpus::parse(&sized(even, '\u{e9}')).is_ok());
    assert!(even + 2 > MAX_CORPUS_BYTES);
    let wide = Corpus::parse(&sized(even + 2, '\u{e9}')).unwrap_err();
    assert_eq!((wide.code, wide.path.as_str()), (ErrorCode::TooLarge, "$"));
}

#[test]
fn a_corpus_holds_at_most_the_bound_of_cases() {
    let cases = |count: usize| -> Vec<Value> {
        (0..count)
            .map(|index| classification_case(&format!("c-{index}"), "other"))
            .collect()
    };
    assert_eq!(corpus(&cases(MAX_CASES)).cases().len(), MAX_CASES);
    let over = Corpus::parse(&document(&cases(MAX_CASES + 1))).unwrap_err();
    assert_eq!(
        (over.code, over.path.as_str()),
        (ErrorCode::TooLarge, "cases")
    );
}

/// A corpus whose one verdict schema nests objects until the document is `levels` deep: the
/// document, its cases, the case, the rubric and the schema are the first five.
fn nested(levels: usize) -> Vec<u8> {
    let mut schema = json!({});
    for _ in 5..levels {
        schema = json!({"a": schema});
    }
    let mut case = verdict_case("c-1", &json!(true));
    case["rubric"]["schema"] = schema;
    document(&[case])
}

#[test]
fn a_corpus_nests_at_most_the_bound_of_levels() {
    assert!(Corpus::parse(&nested(MAX_CORPUS_DEPTH)).is_ok());
    let over = Corpus::parse(&nested(MAX_CORPUS_DEPTH + 1)).unwrap_err();
    assert_eq!(over.code, ErrorCode::TooLarge);
    assert!(
        over.path.starts_with("cases[0].rubric.schema.*.*"),
        "{over}"
    );
}

fn sha256(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

#[test]
fn the_digest_is_of_the_bytes_as_given_so_other_whitespace_or_key_order_is_another_corpus() {
    let compact = r#"{"format":"llm.corpus/1","cases":[{"id":"c-1","kind":"classification","input_tokens":100,"label_set":{"id":"intent","version":"3","labels":["refund","other"]},"input":"hello","recorded":{"label":"other"}}]}"#;
    let spaced = with(compact, r#""cases":["#, "\"cases\": [\n ");
    let reordered = with(
        compact,
        r#""id":"c-1","kind":"classification""#,
        r#""kind":"classification","id":"c-1""#,
    );
    let read = |text: &str| Corpus::parse(text.as_bytes()).unwrap();
    let (first, second, third) = (read(compact), read(&spaced), read(&reordered));
    assert_eq!(first.digest(), sha256(compact.as_bytes()));
    assert_eq!(second.digest(), sha256(spaced.as_bytes()));
    assert_eq!(third.digest(), sha256(reordered.as_bytes()));
    assert_ne!(first.digest(), second.digest());
    assert_ne!(first.digest(), third.digest());
    // The three read as the same cases: only the digest tells them apart.
    assert_eq!(first.cases(), second.cases());
    assert_eq!(first.cases(), third.cases());
    assert_eq!(read(compact).digest(), first.digest());
}

/// Seven cases: each of the three comparisons once equal and once not, and one failed call.
fn mixed() -> Vec<Value> {
    vec![
        turn_case("turn-same", RECORDED_TEXT),
        turn_case("turn-differs", RECORDED_TEXT),
        classification_case("label-same", "Refund"),
        classification_case("label-differs", "Refund"),
        verdict_case("verdict-same", &json!({"score": 4, "grounds": [1.0, 2]})),
        verdict_case("verdict-differs", &json!({"score": 4, "grounds": [1.0, 2]})),
        turn_case("turn-failed", RECORDED_TEXT),
        turn_case("turn-past-the-cap", RECORDED_TEXT),
        classification_case("label-never-reached", "other"),
    ]
}

fn mixed_steps() -> Vec<(&'static str, Step)> {
    vec![
        ("turn-differs", Step::Answer(CANDIDATE_TEXT)),
        ("label-differs", Step::Answer("refund")),
        (
            "verdict-same",
            Step::Answer(r#"{ "grounds": [1.0, 2], "score": 4 }"#),
        ),
        (
            "verdict-differs",
            Step::Answer(r#"{"grounds":[1,2],"score":4}"#),
        ),
        ("turn-failed", Step::Fail(ErrorCode::Unavailable)),
    ]
}

#[test]
fn one_run_reports_same_differs_failed_and_not_sent_in_the_corpus_order() {
    // Seven sent cases cost 0.055 each: the eighth would need 0.385 + 0.1.
    let (run, records, asked) = played(&mixed(), &mixed_steps(), "0.484999999");
    assert_eq!(
        results(&run),
        [
            ("turn-same", CaseResult::Same, None),
            ("turn-differs", CaseResult::Differs, None),
            ("label-same", CaseResult::Same, None),
            ("label-differs", CaseResult::Differs, None),
            ("verdict-same", CaseResult::Same, None),
            ("verdict-differs", CaseResult::Differs, None),
            (
                "turn-failed",
                CaseResult::Failed,
                Some(ErrorCode::Unavailable)
            ),
            ("turn-past-the-cap", CaseResult::NotSent, None),
            ("label-never-reached", CaseResult::NotSent, None),
        ]
    );
    assert_eq!(
        (run.cases, run.same, run.differs, run.failed, run.not_sent),
        (9, 3, 3, 1, 2)
    );
    assert_eq!(run.stopped, Some(BudgetError::LimitExceeded));
    // Priced at the candidate target's 3 and 5 per 1000 tokens, not the other price of the book.
    assert_eq!(run.spent, amount("0.385"));
    assert_eq!(asked, 7);
    assert_eq!(records.len(), 8);
    assert_eq!(run.id, id("run-1"));
    assert_eq!(run.candidate_route_id, id("coding-candidate"));
    assert_eq!(run.ledger_id, id("replay-spend"));
    assert_eq!(run.currency.as_str(), "USD");
    assert_eq!(run.money_cap, amount("0.484999999"));
    assert_eq!(run.reserve, amount("0.1"));
    assert_eq!(run.corpus_digest, sha256(&document(&mixed())));
}

#[test]
fn a_case_is_sent_when_its_admission_equals_the_cap_and_not_one_nano_past_it() {
    let cases = [
        turn_case("c-1", "one"),
        turn_case("c-2", "two"),
        turn_case("c-3", "three"),
        turn_case("c-4", "four"),
    ];
    // Two settled cases are 0.11; the third is admitted at 0.11 + 0.1 = 0.21.
    let (at, _, asked_at) = played(&cases, &[], "0.21");
    assert_eq!(
        (at.same, at.not_sent, at.spent, asked_at),
        (3, 1, amount("0.165"), 3)
    );
    assert_eq!(results(&at)[3], ("c-4", CaseResult::NotSent, None));
    let (under, _, asked_under) = played(&cases, &[], "0.209999999");
    assert_eq!(
        (under.same, under.not_sent, under.spent, asked_under),
        (2, 2, amount("0.11"), 2)
    );
    assert_eq!(results(&under)[2], ("c-3", CaseResult::NotSent, None));
    assert_eq!(under.stopped, Some(BudgetError::LimitExceeded));
    // A cap every case fits under stops nothing.
    let (all, records, _) = played(&cases, &[], "0.265");
    assert_eq!((all.same, all.not_sent, all.stopped), (4, 0, None));
    assert_eq!((all.spent, records.len()), (amount("0.22"), 4));
}

#[test]
fn every_record_of_a_run_has_role_replay_and_the_refused_case_is_recorded_skipped() {
    let mut cases = mixed();
    cases[2]["primary_record_id"] = json!("call-77");
    cases[7]["primary_record_id"] = json!("call-91");
    let (_, records, _) = played(&cases, &mixed_steps(), "0.484999999");
    assert!(records.iter().all(|record| record.role == Role::Replay));
    assert_eq!(
        records
            .iter()
            .map(|record| (record.id.as_str(), record.kind, record.outcome))
            .collect::<Vec<_>>(),
        [
            ("run-1/turn-same", Kind::Turn, Outcome::Succeeded),
            ("run-1/turn-differs", Kind::Turn, Outcome::Succeeded),
            ("run-1/label-same", Kind::Classification, Outcome::Succeeded),
            (
                "run-1/label-differs",
                Kind::Classification,
                Outcome::Succeeded
            ),
            ("run-1/verdict-same", Kind::Verdict, Outcome::Succeeded),
            ("run-1/verdict-differs", Kind::Verdict, Outcome::Succeeded),
            ("run-1/turn-failed", Kind::Turn, Outcome::Failed),
            ("run-1/turn-past-the-cap", Kind::Turn, Outcome::Skipped),
        ]
    );
    assert_eq!(records[2].primary_record_id, Some(id("call-77")));
    assert_eq!(records[0].primary_record_id, None);
    let skipped = &records[7];
    assert_eq!(skipped.route_id, id("coding-candidate"));
    assert_eq!(skipped.primary_record_id, Some(id("call-91")));
    assert_eq!(
        (
            skipped.dispatch,
            skipped.attempts,
            &skipped.cost,
            skipped.route_target_id.as_ref(),
            skipped.latency_ms,
            skipped.error_code
        ),
        (Some(Dispatch::NotSent), 0, &None, None, None, None)
    );
    // A classification past the cap is skipped as a classification.
    let (_, records, _) = played(&cases[2..4], &[], "0.154999999");
    assert_eq!(
        (records[1].kind, records[1].outcome, records[1].role),
        (Kind::Classification, Outcome::Skipped, Role::Replay)
    );
}

#[test]
fn a_failed_call_counts_failed_with_its_code_and_settles_what_it_cost() {
    let cases = [
        turn_case("sent-and-failed", "one"),
        turn_case("refused-unsent", "two"),
        turn_case("answered", "three"),
    ];
    let steps = [
        ("sent-and-failed", Step::Fail(ErrorCode::RateLimited)),
        ("refused-unsent", Step::Refuse(ErrorCode::Unsupported)),
    ];
    let (run, records, asked) = played(&cases, &steps, "1");
    assert_eq!(
        results(&run),
        [
            (
                "sent-and-failed",
                CaseResult::Failed,
                Some(ErrorCode::RateLimited)
            ),
            (
                "refused-unsent",
                CaseResult::Failed,
                Some(ErrorCode::Unsupported)
            ),
            ("answered", CaseResult::Same, None),
        ]
    );
    assert_eq!((run.failed, run.differs, run.same), (2, 0, 1));
    // The failed call reported usage and is paid for; the refused case made no call.
    assert_eq!((run.spent, records.len(), asked), (amount("0.11"), 2, 3));
    assert_eq!(records[0].error_code, Some(ErrorCode::RateLimited));
}

#[test]
fn an_answer_the_candidate_holds_unusable_differs() {
    let cases = [classification_case("c-1", "other")];
    let (run, records, _) = played(&cases, &[("c-1", Step::Unusable)], "1");
    assert_eq!(results(&run), [("c-1", CaseResult::Differs, None)]);
    assert_eq!((run.spent, records.len()), (amount("0.055"), 1));
}

#[test]
fn a_cost_that_is_not_known_stays_held_at_the_reserve_and_stops_the_run() {
    let cases = [
        turn_case("c-1", "one"),
        turn_case("c-2", "two"),
        turn_case("c-3", "three"),
    ];
    let (run, records, asked) = played(&cases, &[("c-2", Step::Unpriced("two"))], "1");
    assert_eq!(
        results(&run),
        [
            ("c-1", CaseResult::Same, None),
            ("c-2", CaseResult::Same, None),
            ("c-3", CaseResult::NotSent, None),
        ]
    );
    assert_eq!(run.stopped, Some(BudgetError::UncertainSpend));
    // 0.055 settled and the second case's whole reserve of 0.1 still held, never zero.
    assert_eq!((run.spent, asked), (amount("0.155"), 2));
    assert_eq!(records[2].outcome, Outcome::Skipped);
}

#[test]
fn a_call_the_price_book_holds_no_price_for_stays_held_and_stops_the_run() {
    let catalog = catalog();
    // The book prices the other serving model only.
    let mut document = prices(&catalog, "USD").document().clone();
    document
        .models
        .retain(|price| price.serving_model_id == id("local-small"));
    let other = PriceBook::from_document(document).unwrap();
    let clock = Ticks::new();
    let log = CallLog::new();
    let candidate = Scripted::new(&catalog, &[]);
    let run = block_on(replay(
        &catalog,
        &corpus(&[turn_case("c-1", "one"), turn_case("c-2", "two")]),
        &run_of("1", "0.1"),
        &candidate,
        Recording {
            sink: &log,
            clock: &clock,
            prices: Some(&other),
        },
    ))
    .unwrap();
    assert_eq!(
        results(&run),
        [
            ("c-1", CaseResult::Same, None),
            ("c-2", CaseResult::NotSent, None),
        ]
    );
    assert_eq!(run.stopped, Some(BudgetError::UncertainSpend));
    assert_eq!((run.spent, candidate.asked()), (amount("0.1"), 1));
}

#[test]
fn texts_and_labels_compare_byte_for_byte() {
    let cases = [
        turn_case("multi-byte-same", "na\u{ef}ve \u{4e16}\u{754c}"),
        turn_case("trailing-space", "answer"),
        turn_case("composed-or-not", "caf\u{e9}"),
        classification_case("other-case", "refund"),
        verdict_case("not-json", &json!({"score": 4})),
        verdict_case("number-form", &json!(2)),
    ];
    let steps = [
        ("trailing-space", Step::Answer("answer ")),
        ("composed-or-not", Step::Answer("cafe\u{301}")),
        ("other-case", Step::Answer("Refund")),
        ("not-json", Step::Answer("{\"score\": 4")),
        ("number-form", Step::Answer("2.0")),
    ];
    let (run, _, _) = played(&cases, &steps, "1");
    assert_eq!(
        results(&run),
        [
            ("multi-byte-same", CaseResult::Same, None),
            ("trailing-space", CaseResult::Differs, None),
            ("composed-or-not", CaseResult::Differs, None),
            ("other-case", CaseResult::Differs, None),
            ("not-json", CaseResult::Differs, None),
            ("number-form", CaseResult::Differs, None),
        ]
    );
}

#[test]
fn a_run_is_refused_before_any_case_is_sent() {
    let catalog = catalog();
    let usd = prices(&catalog, "USD");
    let eur = prices(&catalog, "EUR");
    let corpus = corpus(&[turn_case("c-1", "one")]);
    let route = |route: &str| Replay {
        candidate_route_id: id(route),
        ..run_of("1", "0.1")
    };
    let rows: [(&str, Replay, Option<&PriceBook>, ErrorCode); 6] = [
        (
            "a reserve of zero",
            run_of("1", "0"),
            Some(&usd),
            ErrorCode::InvalidRequest,
        ),
        (
            "a reserve one nano above the cap",
            run_of("0.1", "0.100000001"),
            Some(&usd),
            ErrorCode::InvalidRequest,
        ),
        (
            "a route the catalog does not declare",
            route("coding-absent"),
            Some(&usd),
            ErrorCode::InvalidRequest,
        ),
        (
            "a route alias in place of its id",
            route("code-candidate"),
            Some(&usd),
            ErrorCode::InvalidRequest,
        ),
        (
            "no price book",
            run_of("1", "0.1"),
            None,
            ErrorCode::InvalidRequest,
        ),
        (
            "a price book in another currency",
            run_of("1", "0.1"),
            Some(&eur),
            ErrorCode::InvalidRequest,
        ),
    ];
    for (name, run, prices, code) in rows {
        let clock = Ticks::new();
        let log = CallLog::new();
        let candidate = Scripted::new(&catalog, &[]);
        let refused = block_on(replay(
            &catalog,
            &corpus,
            &run,
            &candidate,
            Recording {
                sink: &log,
                clock: &clock,
                prices,
            },
        ))
        .expect_err(name);
        assert_eq!(refused.code, code, "{name}");
        assert_eq!((candidate.asked(), log.records().len()), (0, 0), "{name}");
    }
    // A reserve equal to the cap admits exactly one case.
    let (run, _, asked) = played(
        &[turn_case("c-1", "one"), turn_case("c-2", "two")],
        &[],
        "0.1",
    );
    assert_eq!((run.same, run.not_sent, asked), (1, 1, 1));
}

#[test]
fn an_embed_route_is_no_candidate() {
    let catalog = catalog_of(llm_routing::RouteKind::Embed);
    let prices = prices(&catalog, "USD");
    let clock = Ticks::new();
    let log = CallLog::new();
    let candidate = Scripted::new(&catalog, &[]);
    let refused = block_on(replay(
        &catalog,
        &corpus(&[turn_case("c-1", "one")]),
        &run_of("1", "0.1"),
        &candidate,
        Recording {
            sink: &log,
            clock: &clock,
            prices: Some(&prices),
        },
    ))
    .unwrap_err();
    assert_eq!(refused.code, ErrorCode::Unsupported);
    assert_eq!((candidate.asked(), log.records().len()), (0, 0));
}

#[test]
fn two_runs_of_one_corpus_report_the_same_digest_and_counts() {
    let (first, _, _) = played(&mixed(), &mixed_steps(), "0.484999999");
    let (second, _, _) = played(&mixed(), &mixed_steps(), "0.484999999");
    assert_eq!(first, second);
    assert_eq!(first.corpus_digest, second.corpus_digest);
    assert_eq!(first.corpus_digest.len(), 64);
}

#[test]
fn the_run_and_its_records_hold_no_text_of_a_case() {
    let (run, records, _) = played(&mixed(), &mixed_steps(), "0.484999999");
    let written = format!(
        "{}{}",
        serde_json::to_string(&run).unwrap(),
        serde_json::to_string(&records).unwrap()
    );
    for marker in [
        INSTRUCTIONS,
        INPUT,
        SUBJECT,
        RUBRIC_TEXT,
        PROMPT,
        "recorded-answer-marker",
        CANDIDATE_TEXT,
        "failure-marker",
        "grounds",
    ] {
        assert!(!written.contains(marker), "{marker} is in {written}");
    }
    let value: Value = serde_json::to_value(&run).unwrap();
    let mut members: Vec<&String> = value.as_object().unwrap().keys().collect();
    members.sort();
    assert_eq!(
        members,
        [
            "candidate_route_id",
            "cases",
            "corpus_digest",
            "currency",
            "differs",
            "failed",
            "id",
            "ledger_id",
            "money_cap",
            "not_sent",
            "reserve",
            "results",
            "same",
            "spent",
            "stopped"
        ]
    );
    assert_eq!(
        value["results"][6],
        json!({"case_id": "turn-failed", "result": "failed", "error_code": "unavailable"})
    );
    assert_eq!(
        value["results"][7],
        json!({"case_id": "turn-past-the-cap", "result": "not-sent", "error_code": null})
    );
    assert_eq!(
        (&value["stopped"], &value["spent"], &value["money_cap"]),
        (
            &json!("limit-exceeded"),
            &json!("0.385"),
            &json!("0.484999999")
        )
    );
}

/// How many times a run of `count` cases asked the candidate, read the clock and wrote a record.
fn work(count: usize) -> (usize, usize, usize) {
    let catalog = catalog();
    let prices = prices(&catalog, "USD");
    let clock = Ticks::new();
    let log = CallLog::new();
    let candidate = Scripted::new(&catalog, &[]);
    let cases: Vec<Value> = (0..count)
        .map(|index| turn_case(&format!("c-{index}"), "same"))
        .collect();
    let run = block_on(replay(
        &catalog,
        &corpus(&cases),
        &run_of("1000", "0.1"),
        &candidate,
        Recording {
            sink: &log,
            clock: &clock,
            prices: Some(&prices),
        },
    ))
    .unwrap();
    assert_eq!(run.same, u64::try_from(count).unwrap());
    (
        candidate.asked(),
        clock.reads.load(Ordering::SeqCst),
        log.records().len(),
    )
}

#[test]
fn a_run_asks_reads_the_clock_and_records_in_proportion_to_its_cases() {
    let (asked, reads, records) = work(100);
    let (asked_twice, reads_twice, records_twice) = work(200);
    let (asked_thrice, reads_thrice, records_thrice) = work(300);
    assert_eq!((asked, records), (100, 100));
    assert_eq!((asked_twice, records_twice), (200, 200));
    assert_eq!((asked_thrice, records_thrice), (300, 300));
    // Each further hundred cases reads the clock as often as the hundred before it.
    assert!(reads_twice > reads);
    assert_eq!(reads_thrice - reads_twice, reads_twice - reads);
}

#[test]
fn a_case_id_that_makes_no_record_id_with_the_run_id_gets_a_digest_id() {
    let long = "c".repeat(256);
    let (run, records, _) = played(&[turn_case(&long, "one")], &[], "1");
    assert_eq!(results(&run), [(long.as_str(), CaseResult::Same, None)]);
    let record = records[0].id.as_str();
    assert!(record.starts_with("replay-sha256-"), "{record}");
    assert_eq!(record.len(), "replay-sha256-".len() + 64);
    // A short case id keeps the readable form.
    let (_, records, _) = played(&[turn_case("c-1", "one")], &[], "1");
    assert_eq!(records[0].id.as_str(), "run-1/c-1");
}

#[test]
fn the_request_id_is_readable_while_the_longest_record_id_of_the_case_is_an_id() {
    type MakeCase = fn(&str) -> Value;
    // What each kind adds to `run-1/<case id>` for the longest id of a record it writes.
    let classify = 1 + REPLAY_CLASSIFIER_ID.len();
    let rubric = 1 + "helpful".len();
    let rows: [(&str, MakeCase, usize); 3] = [
        ("a turn", |case_id| turn_case(case_id, "one"), 0),
        (
            "a classification",
            |case_id| classification_case(case_id, "other"),
            classify,
        ),
        (
            "a verdict",
            |case_id| verdict_case(case_id, &json!(true)),
            rubric,
        ),
    ];
    for (name, case, suffix) in rows {
        let fits = 256 - "run-1/".len() - suffix;
        for (length, readable) in [(fits, true), (fits + 1, false), (256, false)] {
            let case_id = "c".repeat(length);
            let (run, records, asked) = played(&[case(&case_id)], &[], "1");
            assert_eq!(
                (run.same, asked, records.len()),
                (1, 1, 1),
                "{name} {length}"
            );
            let request_id = records[0].id.as_str();
            if readable {
                assert_eq!(request_id, format!("run-1/{case_id}"), "{name} {length}");
            } else {
                assert!(request_id.starts_with("replay-sha256-"), "{name} {length}");
                assert_eq!(request_id.len(), 78, "{name} {length}");
            }
        }
    }
}

#[test]
fn a_run_stops_at_its_last_case_when_that_case_takes_the_spend_past_the_cap() {
    let cases = [turn_case("c-1", "one"), turn_case("c-2", "two")];
    // Each call costs 0.055 and holds one nanounit. The second is admitted at 0.055 + 0.000000001,
    // the cap, and settles the spend at 0.11.
    let catalog = catalog();
    let candidate = Scripted::new(&catalog, &[]);
    let (run, records) = adversary_w29_run(
        &cases,
        "0.055000001",
        "0.000000001",
        &Ticks::new(),
        &candidate,
    );
    assert_eq!((run.same, run.not_sent, candidate.asked()), (2, 0, 2));
    assert_eq!(run.stopped, Some(BudgetError::LimitExceeded));
    // The bound is met exactly: the cap, less the reserve, plus the call that passed it.
    assert_eq!(run.spent, amount("0.11"));
    assert_eq!(
        run.spent.nanos(),
        run.money_cap.nanos() - run.reserve.nanos() + amount("0.055").nanos()
    );
    assert_eq!(records.len(), 2);
    // A cap of what the two calls cost stops nothing.
    let candidate = Scripted::new(&catalog, &[]);
    let (within, _) = adversary_w29_run(&cases, "0.11", "0.000000001", &Ticks::new(), &candidate);
    assert_eq!((within.spent, within.stopped), (amount("0.11"), None));
}

#[test]
fn spent_holds_a_reserve_only_when_the_run_stopped_with_uncertain_spend() {
    let cases = [turn_case("c-1", "one"), turn_case("c-2", "two")];
    // Every cost known: `spent` is what the calls cost.
    let (known, _, _) = played(&cases, &[], "1");
    assert_eq!((known.spent, known.stopped), (amount("0.11"), None));
    // The last case's cost is not known: `spent` is the first case's cost and the reserve held
    // for the second, and `stopped` says so.
    let (held, records, asked) = played(&cases, &[("c-2", Step::Unpriced("two"))], "1");
    assert_eq!(
        (held.spent, held.stopped),
        (amount("0.155"), Some(BudgetError::UncertainSpend))
    );
    assert_eq!(
        (held.same, held.not_sent, asked, records.len()),
        (2, 0, 2, 2)
    );
}

/// One run of `cases` under `cap` with `reserve` held per case, on `clock` and through
/// `candidate`: the run and every record.
fn adversary_w29_run(
    cases: &[Value],
    cap: &str,
    reserve: &str,
    clock: &dyn Clock,
    candidate: &dyn Candidate,
) -> (ReplayRun, Vec<CallRecord>) {
    let catalog = catalog();
    let prices = prices(&catalog, "USD");
    let log = CallLog::new();
    let run = block_on(replay(
        &catalog,
        &corpus(cases),
        &run_of(cap, reserve),
        candidate,
        Recording {
            sink: &log,
            clock,
            prices: Some(&prices),
        },
    ))
    .unwrap();
    (run, log.records())
}

/// Re-pinned in correction round 1 (it held `spent <= money_cap`, which the documents no longer
/// say). The cap bounds admission: a case that costs more than its reserve can take the spend
/// past the cap. Then no later case is sent, and the spend is at most the cap, less the reserve,
/// plus the cost of the one call that passed it.
#[test]
fn adversary_w29_a_run_past_its_cap_stops_within_the_cap_less_a_reserve_plus_one_call() {
    let catalog = catalog();
    let candidate = Scripted::new(&catalog, &[]);
    let cases = [
        turn_case("c-1", "one"),
        turn_case("c-2", "two"),
        turn_case("c-3", "three"),
    ];
    // Each call costs 0.055; one nanounit is held for it. The first is admitted at 0.000000001
    // and the second at 0.055000001, both within the cap; the second settles the spend at 0.11.
    let (run, records) =
        adversary_w29_run(&cases, "0.06", "0.000000001", &Ticks::new(), &candidate);
    assert_eq!(
        results(&run),
        [
            ("c-1", CaseResult::Same, None),
            ("c-2", CaseResult::Same, None),
            ("c-3", CaseResult::NotSent, None),
        ]
    );
    assert_eq!((candidate.asked(), run.spent), (2, amount("0.11")));
    assert_eq!(run.stopped, Some(BudgetError::LimitExceeded));
    assert_eq!(records[2].outcome, Outcome::Skipped);
    let call = amount("0.055").nanos();
    assert!(
        run.spent.nanos() <= run.money_cap.nanos() - run.reserve.nanos() + call,
        "the run spent {} under a cap of {} ({} cases asked)",
        run.spent,
        run.money_cap,
        candidate.asked()
    );
}

/// "A replayed call whose cost is not known keeps its reserve held and stops the run with
/// `uncertain-spend`": the last case of a corpus has no case after it to be refused, and the
/// summary must still say that its `spent` is a hold and not a known spend.
#[test]
fn adversary_w29_a_run_whose_last_case_has_no_known_cost_reports_uncertain_spend() {
    let catalog = catalog();
    let candidate = Scripted::new(&catalog, &[("c-1", Step::Unpriced("one"))]);
    let (run, _) = adversary_w29_run(
        &[turn_case("c-1", "one")],
        "1",
        "0.1",
        &Ticks::new(),
        &candidate,
    );
    assert_eq!(results(&run), [("c-1", CaseResult::Same, None)]);
    // The reserve is reported as the spend: nothing of it is known to have been spent.
    assert_eq!(run.spent, amount("0.1"));
    assert_eq!(run.stopped, Some(BudgetError::UncertainSpend));
}

/// "No text of the corpus is in a refusal": a member name of a recorded verdict is text of the
/// case (the unit's own no-text test counts the verdict member `grounds` as such).
#[test]
fn adversary_w29_a_refusal_holds_no_member_name_of_a_recorded_verdict() {
    let text = String::from_utf8(document(&[verdict_case("c-1", &json!({"score": 4}))])).unwrap();
    let named = with(
        &text,
        r#"{"score":4}"#,
        r#"{"verdict-marker-stoat-8841":true,"verdict-marker-stoat-8841":false}"#,
    );
    let error = Corpus::parse(named.as_bytes()).unwrap_err();
    assert_eq!(error.reason, "a member given twice");
    assert!(
        !error.to_string().contains("verdict-marker-stoat-8841"),
        "{error}"
    );
}

/// A clock that steps back one second each time it is read.
struct Falling(AtomicU64);

impl Clock for Falling {
    fn unix_ms(&self) -> u64 {
        1_762_344_000_000 - 1000 * self.0.fetch_add(1, Ordering::SeqCst)
    }
    fn monotonic(&self) -> Duration {
        Duration::ZERO
    }
}

/// Guard: a wall clock that steps back between two cases (an adjusted host clock) must not make
/// the ledger refuse a case.
#[test]
fn adversary_w29_a_clock_that_steps_back_stops_no_case() {
    let catalog = catalog();
    let candidate = Scripted::new(&catalog, &[]);
    let cases = [
        turn_case("c-1", "one"),
        turn_case("c-2", "two"),
        turn_case("c-3", "three"),
    ];
    let clock = Falling(AtomicU64::new(0));
    let (run, records) = adversary_w29_run(&cases, "1", "0.1", &clock, &candidate);
    assert_eq!((run.same, run.not_sent, run.stopped), (3, 0, None));
    assert_eq!((run.spent, records.len()), (amount("0.165"), 3));
}

/// Guard, re-pinned in correction round 1 (it held the bound of 64 bytes, which is gone): a
/// member name is shown in a path when the format defines it there, and a name the caller chose
/// is not, whatever its length.
#[test]
fn adversary_w29_a_member_name_is_shown_when_the_format_defines_it_and_not_for_its_length() {
    let text = String::from_utf8(document(&[turn_case("c-1", "one")])).unwrap();
    for length in [1, 64, 65] {
        let name = "n".repeat(length);
        let named = with(&text, "{", &format!(r#"{{"{name}":1,"#));
        let error = Corpus::parse(named.as_bytes()).unwrap_err();
        assert_eq!(error.path, "*", "{length}");
    }
    let named = with(&text, r#""kind":"turn""#, r#""kind":"turn","rubric":1"#);
    let error = Corpus::parse(named.as_bytes()).unwrap_err();
    assert_eq!(error.path, "cases[0].rubric");
}

/// Sends each case as one recorded turn under a token that is already cancelled.
struct Cancelled<'c>(&'c Catalog);

impl Candidate for Cancelled<'_> {
    fn answer<'a>(&'a self, call: CaseCall<'a>) -> BoxFuture<'a, Result<Option<String>, Error>> {
        Box::pin(async move {
            let binding = self.0.binding(&id("remote-large")).unwrap();
            let model = OneCall {
                provenance: binding.provenance().clone(),
                capabilities: binding.capabilities().clone(),
                recorded: call.case.recorded.clone(),
                step: None,
            };
            let request = TurnRequest::new(call.route, vec![Item::user("replayed")]);
            let cancel = Cancel::new();
            cancel.cancel();
            let pause = |_: Duration| -> BoxFuture<'static, ()> { Box::pin(async {}) };
            let mut admit = |_: &Selection<'_>| Ok(());
            let run = llm_calls::record_turn(
                self.0,
                &request,
                Some(call.case.input_tokens),
                FallbackPolicy {
                    max_attempts: 1,
                    ..FallbackPolicy::default()
                },
                Ports {
                    models: &model,
                    admit: &mut admit,
                    sink: &mut Discard,
                    cancel: &cancel,
                    pause: &pause,
                },
                llm_calls::CallIdentity::primary(call.request_id.clone()),
                call.recording,
            )
            .await?;
            run.result.map(|_| None)
        })
    }
}

/// Guard: "A call that was not sent and reported no usage cost nothing". A cancelled case
/// writes a not-sent record without a cost; it settles at zero and the next case is admitted.
#[test]
fn adversary_w29_a_record_of_a_call_that_was_not_sent_costs_nothing_and_stops_nothing() {
    let catalog = catalog();
    let cases = [turn_case("c-1", "one"), turn_case("c-2", "two")];
    let (run, records) = adversary_w29_run(&cases, "1", "0.1", &Ticks::new(), &Cancelled(&catalog));
    assert_eq!(
        records
            .iter()
            .map(|record| (record.dispatch, &record.cost, record.role))
            .collect::<Vec<_>>(),
        [
            (Some(Dispatch::NotSent), &None, Role::Replay),
            (Some(Dispatch::NotSent), &None, Role::Replay)
        ]
    );
    assert_eq!(
        results(&run),
        [
            ("c-1", CaseResult::Failed, Some(ErrorCode::Cancelled)),
            ("c-2", CaseResult::Failed, Some(ErrorCode::Cancelled)),
        ]
    );
    assert_eq!((run.spent, run.stopped), (Amount::ZERO, None));
}
