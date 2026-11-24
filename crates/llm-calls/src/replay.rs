//! Offline replay: a recorded corpus sent case by case to a candidate route and compared with
//! the answers it recorded, under a money cap held on an `llm-cost` budget ledger.
//!
//! The corpus is the caller's document, `llm.corpus/1`: llm writes none and records no text of
//! one. [`Corpus::parse`] takes it only whole. It refuses, naming the member by its path and
//! sending nothing: a document that is not one JSON value (cut, followed by more bytes or by a
//! second value), a member given twice at any depth, a format version or case kind it does not
//! know, a member the format does not declare, and a document past its bounds of
//! [`MAX_CORPUS_BYTES`] bytes, [`MAX_CASES`] cases or [`MAX_CORPUS_DEPTH`] levels of nesting.
//! A refusal holds a path and a fixed reason, no text of the corpus.
//!
//! [`replay`] sends the cases in the order given, one at a time, through a [`Candidate`]: the
//! port that turns a case into the candidate route's calls, because the classifier and the judge
//! live in `llm-decide`, which depends on this crate. Each case is admitted on a ledger of the
//! run's own, an `llm-cost` [`BudgetEngine`] whose limit is the run's cap: the case's reserve is
//! reserved and started before the candidate is asked, and settled afterwards with the priced
//! cost of every call record the case wrote. Money is exact: a cap, a reserve and a spend are
//! [`Amount`]s, decimals with nine fractional digits in the run's currency, and a price becomes
//! spend only through the caller's price book, as for every call record. A case is admitted
//! while the run's spend plus the reserve is at most the cap. The first case the ledger refuses
//! is not sent and writes one record with outcome `skipped`; the run stops there, and that case
//! and each one after it count [`CaseResult::NotSent`]. A cost that is not known (a call that
//! reported no usage, a price the book does not hold) is not settled as zero: the case's reserve
//! stays held and the ledger admits no further case.
//!
//! The run's own loops are linear in the corpus. The budget engine reads every reservation of
//! its ledger at each command, so the ledger's work grows with the square of the cases sent,
//! bounded by [`MAX_CASES`].

use crate::{CallIdentity, CallRecord, CallSink, Kind, Recording, Role};
use llm_core::{BoxFuture, Dispatch, Error, ErrorCode, Id, Item, TurnDocument, TurnRequest};
use llm_cost::{
    Amount, Currency,
    budget::{
        BudgetCommand, BudgetEngine, BudgetError, BudgetPolicy, Operation, ReservationRequest,
        Settlement,
    },
};
use llm_routing::Catalog;
use serde::{
    Deserializer, Serialize,
    de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    fmt::{self, Write as _},
    sync::{Mutex, PoisonError},
};

/// The one corpus format this crate reads.
pub const CORPUS_FORMAT: &str = "llm.corpus/1";
/// The most bytes a corpus document may have: 16 MiB.
pub const MAX_CORPUS_BYTES: usize = 16 * 1024 * 1024;
/// The most cases a corpus may hold.
pub const MAX_CASES: usize = 4096;
/// The most levels of nesting a corpus may have; the document's own object is the first.
pub const MAX_CORPUS_DEPTH: usize = 64;

const NOT_WHOLE: &str = "not one whole JSON document";
const TWICE: &str = "a member given twice";
const TOO_DEEP: &str = "nested deeper than 64 levels";

/// Why a corpus was refused: `invalid-request`, or `too-large` past a bound, the path of the
/// member (`cases[1].recorded.label`; `$` is the document) and a fixed reason. A member name
/// that is not at most 64 letters, digits, `-` and `_` is shown as `*`, so no text of the corpus
/// is in a refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusError {
    pub code: ErrorCode,
    pub path: String,
    pub reason: &'static str,
}

impl fmt::Display for CorpusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path, self.reason)
    }
}

impl std::error::Error for CorpusError {}

impl From<CorpusError> for Error {
    fn from(error: CorpusError) -> Self {
        Self::new(error.code, error.to_string())
    }
}

/// One step of a path into the corpus.
#[derive(Debug, Clone, Copy)]
enum Seg<'a> {
    Member(&'a str),
    Index(usize),
}

fn render<'a>(segments: impl IntoIterator<Item = Seg<'a>>) -> String {
    let mut path = String::new();
    for segment in segments {
        match segment {
            Seg::Member(name) => {
                if !path.is_empty() {
                    path.push('.');
                }
                let shown = !name.is_empty()
                    && name.len() <= 64
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
                path.push_str(if shown { name } else { "*" });
            }
            Seg::Index(index) => {
                let _ = write!(path, "[{index}]");
            }
        }
    }
    if path.is_empty() {
        path.push('$');
    }
    path
}

/// A step the one-pass reader has taken and not yet left.
enum Step {
    Member(String),
    Index(usize),
}

/// Where the one-pass reader is, and the refusal of its own it raised, if any.
#[derive(Default)]
struct Reading {
    path: RefCell<Vec<Step>>,
    refusal: Cell<Option<(ErrorCode, &'static str)>>,
}

impl Reading {
    fn refuse<E: de::Error>(&self, code: ErrorCode, reason: &'static str) -> E {
        self.refusal.set(Some((code, reason)));
        E::custom(reason)
    }
}

/// Reads one JSON value in one pass, refusing a member given twice and nesting past the bound.
/// A step is left only when its value was read, so after a failure the path is where it failed.
struct Whole<'r> {
    reading: &'r Reading,
    step: Option<Step>,
    depth: usize,
}

impl Whole<'_> {
    fn enter<E: de::Error>(&self) -> Result<usize, E> {
        if self.depth >= MAX_CORPUS_DEPTH {
            return Err(self.reading.refuse(ErrorCode::TooLarge, TOO_DEEP));
        }
        Ok(self.depth + 1)
    }
}

impl<'de> DeserializeSeed<'de> for Whole<'_> {
    type Value = Value;
    fn deserialize<D: Deserializer<'de>>(mut self, deserializer: D) -> Result<Value, D::Error> {
        let reading = self.reading;
        let step = self.step.take();
        let stepped = step.is_some();
        reading.path.borrow_mut().extend(step);
        let value = deserializer.deserialize_any(self)?;
        if stepped {
            reading.path.borrow_mut().pop();
        }
        Ok(value)
    }
}

impl<'de> Visitor<'de> for Whole<'_> {
    type Value = Value;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }
    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }
    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }
    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }
    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Value, E> {
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| self.reading.refuse(ErrorCode::InvalidRequest, NOT_WHOLE))
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<Value, E> {
        Ok(Value::String(value.to_owned()))
    }
    fn visit_string<E: de::Error>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }
    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<Value, A::Error> {
        let depth = self.enter()?;
        let mut read = Vec::new();
        while let Some(item) = items.next_element_seed(Whole {
            reading: self.reading,
            step: Some(Step::Index(read.len())),
            depth,
        })? {
            read.push(item);
        }
        Ok(Value::Array(read))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut members: A) -> Result<Value, A::Error> {
        let depth = self.enter()?;
        let mut read = Map::new();
        while let Some(name) = members.next_key::<String>()? {
            if read.contains_key(&name) {
                self.reading.path.borrow_mut().push(Step::Member(name));
                return Err(self.reading.refuse(ErrorCode::InvalidRequest, TWICE));
            }
            let value = members.next_value_seed(Whole {
                reading: self.reading,
                step: Some(Step::Member(name.clone())),
                depth,
            })?;
            read.insert(name, value);
        }
        Ok(Value::Object(read))
    }
}

/// `bytes` as the one JSON value they are, or the refusal and where the reader was.
fn whole(bytes: &[u8]) -> Result<Value, CorpusError> {
    let reading = Reading::default();
    let mut reader = serde_json::Deserializer::from_slice(bytes);
    let seed = Whole {
        reading: &reading,
        step: None,
        depth: 0,
    };
    seed.deserialize(&mut reader)
        .and_then(|value| reader.end().map(|()| value))
        .map_err(|_| {
            let (code, reason) = reading
                .refusal
                .take()
                .unwrap_or((ErrorCode::InvalidRequest, NOT_WHOLE));
            let path = reading.path.borrow();
            CorpusError {
                code,
                path: render(path.iter().map(|step| match step {
                    Step::Member(name) => Seg::Member(name),
                    Step::Index(index) => Seg::Index(*index),
                })),
                reason,
            }
        })
}

/// `value` as canonical JSON, the form `llm-decide` gives a verdict: the keys of every object
/// sorted, no whitespace, every scalar as `serde_json` writes it.
fn canonical(value: &Value, out: &mut String) {
    match value {
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(members) => {
            let mut members: Vec<(&String, &Value)> = members.iter().collect();
            members.sort_by_key(|(name, _)| *name);
            out.push('{');
            for (index, (name, member)) in members.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&Value::from(name.as_str()).to_string());
                out.push(':');
                canonical(member, out);
            }
            out.push('}');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

/// A place in the corpus the typed reading is at: its parent and the step from it.
#[derive(Clone, Copy)]
struct Place<'a> {
    parent: Option<&'a Place<'a>>,
    segment: Option<Seg<'a>>,
}

impl<'a> Place<'a> {
    const DOCUMENT: Place<'static> = Place {
        parent: None,
        segment: None,
    };

    fn member(&'a self, name: &'a str) -> Self {
        Self {
            parent: Some(self),
            segment: Some(Seg::Member(name)),
        }
    }

    fn index(&'a self, index: usize) -> Self {
        Self {
            parent: Some(self),
            segment: Some(Seg::Index(index)),
        }
    }

    fn error(&self, code: ErrorCode, reason: &'static str) -> CorpusError {
        let mut segments = Vec::new();
        let mut place = Some(self);
        while let Some(at) = place {
            segments.extend(at.segment);
            place = at.parent;
        }
        segments.reverse();
        CorpusError {
            code,
            path: render(segments),
            reason,
        }
    }

    fn refuse(&self, reason: &'static str) -> CorpusError {
        self.error(ErrorCode::InvalidRequest, reason)
    }

    /// The object here, which holds no member outside `declared`.
    fn object<'v>(
        &self,
        value: &'v Value,
        declared: &[&[&str]],
    ) -> Result<&'v Map<String, Value>, CorpusError> {
        let members = value
            .as_object()
            .ok_or_else(|| self.refuse("not an object"))?;
        for name in members.keys() {
            if !declared.iter().any(|names| names.contains(&name.as_str())) {
                return Err(self
                    .member(name)
                    .refuse("not a member llm.corpus/1 declares here"));
            }
        }
        Ok(members)
    }

    fn get<'v>(
        &self,
        members: &'v Map<String, Value>,
        name: &str,
    ) -> Result<&'v Value, CorpusError> {
        members
            .get(name)
            .ok_or_else(|| self.member(name).refuse("a member that must be given"))
    }

    fn text<'v>(
        &self,
        members: &'v Map<String, Value>,
        name: &str,
    ) -> Result<&'v str, CorpusError> {
        self.get(members, name)?
            .as_str()
            .ok_or_else(|| self.member(name).refuse("not a text"))
    }

    fn id(&self, members: &Map<String, Value>, name: &str) -> Result<Id, CorpusError> {
        Id::new(self.text(members, name)?).map_err(|_| {
            self.member(name)
                .refuse("not an identifier of 1 to 256 printable ASCII bytes")
        })
    }
}

/// What a case asks the candidate route.
#[derive(Debug, Clone, PartialEq)]
pub enum CaseAsk {
    /// One turn: the request of the case's `llm.turn/3` or `llm.turn/4` document. The route it
    /// names is the one it was recorded on; a candidate sends it to the candidate route.
    Turn { request: Box<TurnRequest> },
    /// One classification of `input` into one label of the set.
    Classification {
        label_set_id: Id,
        label_set_version: String,
        labels: Vec<String>,
        instructions: String,
        input: String,
    },
    /// One judgement of `subject` against the rubric.
    Verdict {
        rubric_id: Id,
        rubric_version: String,
        rubric_text: String,
        schema: Value,
        subject: String,
    },
}

/// One case of a corpus: a request and the answer recorded for it.
#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    pub id: Id,
    /// The caller's input-token estimate, which routing admits the candidate's target against.
    pub input_tokens: u64,
    /// The primary call this case was recorded from; each of the case's records names it.
    pub primary_record_id: Option<Id>,
    pub ask: CaseAsk,
    /// The recorded answer: a turn's text, a classification's label, or a verdict as canonical
    /// JSON (the keys of every object sorted, no whitespace).
    pub recorded: String,
}

impl Case {
    /// The kind of the case's calls.
    pub const fn kind(&self) -> Kind {
        match self.ask {
            CaseAsk::Turn { .. } => Kind::Turn,
            CaseAsk::Classification { .. } => Kind::Classification,
            CaseAsk::Verdict { .. } => Kind::Verdict,
        }
    }
}

const COMMON: &[&str] = &[
    "id",
    "kind",
    "input_tokens",
    "primary_record_id",
    "recorded",
];

/// The one member a recorded answer of this kind holds.
fn recorded<'v>(
    at: &Place<'_>,
    members: &'v Map<String, Value>,
    name: &'static str,
) -> Result<&'v Value, CorpusError> {
    let place = at.member("recorded");
    let answer = place.object(at.get(members, "recorded")?, &[&[name]])?;
    place.get(answer, name)
}

fn turn(at: &Place<'_>, members: &Map<String, Value>) -> Result<(CaseAsk, String), CorpusError> {
    let place = at.member("turn");
    let document: TurnDocument = serde_json::from_value(at.get(members, "turn")?.clone())
        .map_err(|_| place.refuse("not an llm.turn/3 or llm.turn/4 document"))?;
    document
        .request
        .validate()
        .map_err(|_| place.refuse("a turn request llm-core refuses"))?;
    let text = recorded(at, members, "text")?
        .as_str()
        .ok_or_else(|| at.member("recorded").member("text").refuse("not a text"))?;
    let request = Box::new(document.request);
    Ok((CaseAsk::Turn { request }, text.to_owned()))
}

fn classification(
    at: &Place<'_>,
    members: &Map<String, Value>,
) -> Result<(CaseAsk, String), CorpusError> {
    let place = at.member("label_set");
    let set = place.object(
        at.get(members, "label_set")?,
        &[&["id", "version", "labels"]],
    )?;
    let listed = place.member("labels");
    let given = place
        .get(set, "labels")?
        .as_array()
        .filter(|labels| !labels.is_empty())
        .ok_or_else(|| listed.refuse("not an array of at least one label"))?;
    let mut labels = Vec::with_capacity(given.len());
    for (index, label) in given.iter().enumerate() {
        let label = label
            .as_str()
            .ok_or_else(|| listed.index(index).refuse("not a text"))?;
        labels.push(label.to_owned());
    }
    let instructions = if members.contains_key("instructions") {
        at.text(members, "instructions")?
    } else {
        ""
    };
    let label = recorded(at, members, "label")?;
    let label = label
        .as_str()
        .filter(|label| labels.iter().any(|known| known == label))
        .ok_or_else(|| {
            at.member("recorded")
                .member("label")
                .refuse("not a label of the case's label set")
        })?;
    let ask = CaseAsk::Classification {
        label_set_id: place.id(set, "id")?,
        label_set_version: place.text(set, "version")?.to_owned(),
        instructions: instructions.to_owned(),
        input: at.text(members, "input")?.to_owned(),
        labels,
    };
    Ok((ask, label.to_owned()))
}

fn verdict(at: &Place<'_>, members: &Map<String, Value>) -> Result<(CaseAsk, String), CorpusError> {
    let place = at.member("rubric");
    let rubric = place.object(
        at.get(members, "rubric")?,
        &[&["id", "version", "text", "schema"]],
    )?;
    let ask = CaseAsk::Verdict {
        rubric_id: place.id(rubric, "id")?,
        rubric_version: place.text(rubric, "version")?.to_owned(),
        rubric_text: place.text(rubric, "text")?.to_owned(),
        schema: place.get(rubric, "schema")?.clone(),
        subject: at.text(members, "subject")?.to_owned(),
    };
    let mut answer = String::new();
    canonical(recorded(at, members, "verdict")?, &mut answer);
    Ok((ask, answer))
}

fn case(at: &Place<'_>, value: &Value) -> Result<Case, CorpusError> {
    let members = value
        .as_object()
        .ok_or_else(|| at.refuse("not an object"))?;
    let kind = at.text(members, "kind")?;
    let own: &[&str] = match kind {
        "turn" => &["turn"],
        "classification" => &["label_set", "instructions", "input"],
        "verdict" => &["rubric", "subject"],
        _ => {
            return Err(at.member("kind").refuse("not a case kind of llm.corpus/1"));
        }
    };
    at.object(value, &[COMMON, own])?;
    let id = at.id(members, "id")?;
    let input_tokens = at.get(members, "input_tokens")?.as_u64().ok_or_else(|| {
        at.member("input_tokens")
            .refuse("not a whole number of tokens")
    })?;
    let primary_record_id = members
        .get("primary_record_id")
        .map(|_| at.id(members, "primary_record_id"))
        .transpose()?;
    let (ask, recorded) = match kind {
        "turn" => turn(at, members)?,
        "classification" => classification(at, members)?,
        _ => verdict(at, members)?,
    };
    Ok(Case {
        id,
        input_tokens,
        primary_record_id,
        ask,
        recorded,
    })
}

/// A corpus read whole: its cases in the order given, and the digest of the bytes it was read
/// from.
#[derive(Debug, Clone, PartialEq)]
pub struct Corpus {
    digest: String,
    cases: Vec<Case>,
}

impl Corpus {
    /// Reads an `llm.corpus/1` document: a JSON object with `format` and `cases`. Each case has
    /// `id`, `kind` (`turn`, `classification` or `verdict`), `input_tokens`, an optional
    /// `primary_record_id` and `recorded`, and by kind: `turn` (an `llm.turn/3` or `llm.turn/4`
    /// document) with `recorded.text`; `label_set` (`id`, `version`, `labels`), `input` and an
    /// optional `instructions` with `recorded.label`, one of the labels; `rubric` (`id`,
    /// `version`, `text`, `schema`) and `subject` with `recorded.verdict`, any JSON value.
    ///
    /// # Errors
    /// `too-large` at `$` for more than [`MAX_CORPUS_BYTES`] bytes, at `cases` for more than
    /// [`MAX_CASES`] cases, and at the member reached for more than [`MAX_CORPUS_DEPTH`] levels.
    /// `invalid-request` for everything else the module documentation lists, an empty `cases`,
    /// a case id given twice at the second case's `id`, and a member that is missing or of
    /// another type, at that member.
    pub fn parse(bytes: &[u8]) -> Result<Self, CorpusError> {
        let document = Place::DOCUMENT;
        if bytes.len() > MAX_CORPUS_BYTES {
            return Err(document.error(ErrorCode::TooLarge, "longer than 16 MiB"));
        }
        let value = whole(bytes)?;
        let members = document.object(&value, &[&["format", "cases"]])?;
        if document.text(members, "format")? != CORPUS_FORMAT {
            return Err(document.member("format").refuse("not llm.corpus/1"));
        }
        let listed = document.member("cases");
        let given = document
            .get(members, "cases")?
            .as_array()
            .filter(|cases| !cases.is_empty())
            .ok_or_else(|| listed.refuse("not an array of at least one case"))?;
        if given.len() > MAX_CASES {
            return Err(listed.error(ErrorCode::TooLarge, "more than 4096 cases"));
        }
        let mut seen = HashSet::with_capacity(given.len());
        let mut cases = Vec::with_capacity(given.len());
        for (index, value) in given.iter().enumerate() {
            let at = listed.index(index);
            let case = case(&at, value)?;
            if !seen.insert(case.id.clone()) {
                return Err(at.member("id").refuse("a case id given twice"));
            }
            cases.push(case);
        }
        let mut digest = String::with_capacity(64);
        for byte in Sha256::digest(bytes) {
            let _ = write!(digest, "{byte:02x}");
        }
        Ok(Self { digest, cases })
    }

    /// Lowercase hex SHA-256 of the bytes the corpus was read from, as given: two documents
    /// that differ only in whitespace or key order read as the same cases and have two digests.
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// The cases, in the order the document gives them.
    pub fn cases(&self) -> &[Case] {
        &self.cases
    }
}

/// One replay run: which route answers, and the ledger its spend is held on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replay {
    pub id: Id,
    /// The id of the catalog route every case is sent to, not its alias.
    pub candidate_route_id: Id,
    /// The id of the run's budget ledger.
    pub ledger_id: Id,
    pub currency: Currency,
    /// The most the run may spend, in `currency`.
    pub money_cap: Amount,
    /// What each case holds on the ledger until its priced cost settles it. A case that costs
    /// more is settled at what it cost, and the cap then admits correspondingly less.
    pub reserve: Amount,
}

/// One case handed to the candidate.
#[derive(Clone, Copy)]
pub struct CaseCall<'a> {
    pub case: &'a Case,
    /// The alias of the candidate route, which a request names to reach it.
    pub route: &'a str,
    /// `<run id>/<case id>`, or `replay-sha256-<hex>` of both when that is no id: the case's
    /// reservation, and the id or id prefix of its call records.
    pub request_id: &'a Id,
    /// Where the case's call records go. The run marks each one a replay record and settles the
    /// case with their cost, so every call a candidate makes is recorded here.
    pub recording: Recording<'a>,
}

/// Sends one case to the candidate route and gives back its answer in the form the case's
/// recorded answer has: a turn's text, a classification's label, a verdict's JSON.
pub trait Candidate: Send + Sync {
    /// `Ok(None)` for a call that answered with nothing the comparison takes: no label of the
    /// set, no verdict the schema holds. The case then differs.
    ///
    /// # Errors
    /// A call that failed, or a case the candidate could not send. The case then counts failed
    /// with the error's code.
    fn answer<'a>(&'a self, call: CaseCall<'a>) -> BoxFuture<'a, Result<Option<String>, Error>>;
}

/// How one case ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaseResult {
    /// The candidate's answer equals the recorded one: the label, the canonical verdict JSON, or
    /// the turn's text byte for byte.
    Same,
    /// The candidate answered otherwise, or with nothing the comparison takes.
    Differs,
    /// The candidate's call failed; the report carries the error code.
    Failed,
    /// Not sent: the ledger refused this case or one before it.
    NotSent,
}

/// One case's result. It holds no text of the case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaseReport {
    pub case_id: Id,
    pub result: CaseResult,
    /// Set exactly when the result is `Failed`.
    pub error_code: Option<ErrorCode>,
}

/// What one run did, as `llm.calls.ReplayRun` declares it. It holds no text of a case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReplayRun {
    pub id: Id,
    pub candidate_route_id: Id,
    /// [`Corpus::digest`].
    pub corpus_digest: String,
    pub currency: Currency,
    pub money_cap: Amount,
    pub reserve: Amount,
    pub ledger_id: Id,
    /// The ledger's exposure when the run ended: what its cases settled, plus the reserve of
    /// each case whose cost is not known.
    pub spent: Amount,
    /// Why the ledger refused the first case that was not sent: `limit-exceeded` past the cap,
    /// `uncertain-spend` after a case whose cost is not known. Absent when every case was sent.
    pub stopped: Option<BudgetError>,
    pub cases: u64,
    pub same: u64,
    pub differs: u64,
    pub failed: u64,
    pub not_sent: u64,
    /// One report per case, in the corpus order.
    pub results: Vec<CaseReport>,
}

/// Passes each record of one case on as a replay record and adds up what the case cost.
struct Spend<'s> {
    sink: &'s dyn CallSink,
    primary_record_id: &'s Option<Id>,
    currency: &'s Currency,
    /// The known nanounits, and whether they are the whole cost.
    tally: Mutex<(u64, bool)>,
}

impl CallSink for Spend<'_> {
    fn record(&self, mut record: CallRecord) {
        record.role = Role::Replay;
        record.primary_record_id.clone_from(self.primary_record_id);
        {
            let mut tally = self.tally.lock().unwrap_or_else(PoisonError::into_inner);
            match &record.cost {
                Some(cost) if &cost.currency == self.currency => {
                    let sum = tally.0.checked_add(cost.known_subtotal.nanos());
                    tally.1 &= cost.complete && sum.is_some();
                    tally.0 = sum.unwrap_or(tally.0);
                }
                // A call that was not sent spent nothing; any other unpriced call is unknown.
                None if record.dispatch == Some(Dispatch::NotSent) => {}
                _ => tally.1 = false,
            }
        }
        self.sink.record(record);
    }
}

/// The request id of one case, which is its reservation id and the id of its skipped record.
/// The readable form always holds `/` and the digest form never does, so the two never meet.
fn request_id(run: &Id, case: &Id) -> Option<Id> {
    crate::shadow::pair_id(
        format!("{run}/{case}"),
        "replay-sha256-",
        [run.as_str(), case.as_str()],
    )
}

fn compare(case: &Case, answer: &str) -> CaseResult {
    let same = if matches!(case.ask, CaseAsk::Verdict { .. }) {
        whole(answer.as_bytes()).is_ok_and(|value| {
            let mut given = String::new();
            canonical(&value, &mut given);
            given == case.recorded
        })
    } else {
        answer == case.recorded
    };
    if same {
        CaseResult::Same
    } else {
        CaseResult::Differs
    }
}

/// Checks the cap and holds the reservation in one step, then marks it started.
fn admit(
    ledger: &mut BudgetEngine,
    now: u64,
    request: ReservationRequest,
) -> Result<(), BudgetError> {
    let id = request.id.clone();
    ledger.apply(now, BudgetCommand::Reserve { request })?;
    if let Err(error) = ledger.apply(now, BudgetCommand::Begin { id: id.clone() }) {
        let _ = ledger.apply(now, BudgetCommand::Cancel { id });
        return Err(error);
    }
    Ok(())
}

/// Sends each case of `corpus` to the candidate route, in order and one at a time, and compares
/// its answer with the recorded one.
///
/// Every record a case writes reaches `recording.sink` with role `Replay` and the case's
/// `primary_record_id`. The first case the run's ledger refuses writes one record with outcome
/// `skipped` under the case's request id and is not sent; it and every later case count
/// `NotSent`, and no later case writes a record. See the module documentation for the ledger.
///
/// # Errors
/// `invalid-request`, before any case is sent and any record written, for a reserve of zero or
/// above the cap, a candidate route id the catalog does not declare, no price book in
/// `recording`, and a price book in another currency than the run's. `unsupported` for an embed
/// route.
pub async fn replay(
    catalog: &Catalog,
    corpus: &Corpus,
    run: &Replay,
    candidate: &dyn Candidate,
    recording: Recording<'_>,
) -> Result<ReplayRun, Error> {
    startable(run, recording)?;
    let route = catalog
        .routes()
        .find(|route| route.id == run.candidate_route_id)
        .ok_or_else(|| Error::invalid("the candidate route is not a route id of the catalog"))?;
    // The reservation names the target the route tries first; the probe's text goes nowhere.
    let probe = TurnRequest::new(route.alias.as_str(), vec![Item::user("replay")]);
    let explanation = catalog.explain(&probe, None)?;
    let target = explanation
        .targets
        .first()
        .ok_or_else(|| Error::invalid("the candidate route has no target"))?;
    let mut ledger = BudgetEngine::new(BudgetPolicy {
        id: run.ledger_id.clone(),
        currency: run.currency.clone(),
        limit: run.money_cap,
        max_active: 1,
    })
    .map_err(|_| Error::invalid("the budget ledger refuses the run"))?;
    let assumption = Id::new("replay-reserve").map_err(|_| Error::invalid("invalid id"))?;
    let mut results = Vec::with_capacity(corpus.cases.len());
    let mut stopped = None;
    for case in &corpus.cases {
        let (result, error_code) = if stopped.is_some() {
            (CaseResult::NotSent, None)
        } else {
            let request_id = request_id(&run.id, &case.id)
                .ok_or_else(|| Error::invalid("the run id and case id make no request id"))?;
            let started = recording.clock.unix_ms();
            let reservation = ReservationRequest {
                id: request_id.clone(),
                operation: Operation::Inference {
                    binding: target.provenance.clone(),
                    billing: target.billing_kind,
                    request_id: request_id.clone(),
                },
                reserved: run.reserve,
                assumption: assumption.clone(),
                expires_at_ms: u64::MAX,
            };
            let now = started.max(ledger.now_ms());
            match admit(&mut ledger, now, reservation) {
                Ok(()) => {
                    let call = CaseCall {
                        case,
                        route: route.alias.as_str(),
                        request_id: &request_id,
                        recording,
                    };
                    sent(call, &run.currency, candidate, &mut ledger).await
                }
                Err(cause) => {
                    let identity = CallIdentity {
                        id: request_id,
                        role: Role::Replay,
                        primary_record_id: case.primary_record_id.clone(),
                    };
                    recording.sink.record(crate::shadow::skipped(
                        identity,
                        case.kind(),
                        &route.id,
                        started,
                    ));
                    stopped = Some(cause);
                    (CaseResult::NotSent, None)
                }
            }
        };
        results.push(CaseReport {
            case_id: case.id.clone(),
            result,
            error_code,
        });
    }
    summary(run, corpus, &ledger, stopped, results)
}

/// Asks the candidate one admitted case, settles the case's reservation with what its records
/// cost, and compares the answer.
async fn sent(
    call: CaseCall<'_>,
    currency: &Currency,
    candidate: &dyn Candidate,
    ledger: &mut BudgetEngine,
) -> (CaseResult, Option<ErrorCode>) {
    let spend = Spend {
        sink: call.recording.sink,
        primary_record_id: &call.case.primary_record_id,
        currency,
        tally: Mutex::new((0, true)),
    };
    let answer = candidate
        .answer(CaseCall {
            recording: Recording {
                sink: &spend,
                ..call.recording
            },
            ..call
        })
        .await;
    let (known, complete) = spend
        .tally
        .into_inner()
        .unwrap_or_else(PoisonError::into_inner);
    let settlement = Settlement {
        currency: currency.clone(),
        known: Amount::from_nanos(known),
        complete,
        evidence: call.request_id.clone(),
    };
    // A settlement the ledger refuses stays held at its reservation.
    let settled = call.recording.clock.unix_ms().max(ledger.now_ms());
    let _ = ledger.apply(
        settled,
        BudgetCommand::Settle {
            id: call.request_id.clone(),
            settlement,
        },
    );
    match answer {
        Ok(Some(answer)) => (compare(call.case, &answer), None),
        Ok(None) => (CaseResult::Differs, None),
        Err(error) => (CaseResult::Failed, Some(error.code)),
    }
}

/// The run's report: its counts, and the ledger's exposure as its spend.
fn summary(
    run: &Replay,
    corpus: &Corpus,
    ledger: &BudgetEngine,
    stopped: Option<BudgetError>,
    results: Vec<CaseReport>,
) -> Result<ReplayRun, Error> {
    let spent = ledger
        .view()
        .ok()
        .and_then(|view| view.totals.exposure_nanos.parse::<u64>().ok())
        .map(Amount::from_nanos)
        .ok_or_else(|| Error::new(ErrorCode::Protocol, "the ledger reports no exposure"))?;
    let count = |result: CaseResult| {
        let cases = results.iter().filter(|case| case.result == result).count();
        u64::try_from(cases).unwrap_or(u64::MAX)
    };
    Ok(ReplayRun {
        id: run.id.clone(),
        candidate_route_id: run.candidate_route_id.clone(),
        corpus_digest: corpus.digest.clone(),
        currency: run.currency.clone(),
        money_cap: run.money_cap,
        reserve: run.reserve,
        ledger_id: run.ledger_id.clone(),
        spent,
        stopped,
        cases: u64::try_from(results.len()).unwrap_or(u64::MAX),
        same: count(CaseResult::Same),
        differs: count(CaseResult::Differs),
        failed: count(CaseResult::Failed),
        not_sent: count(CaseResult::NotSent),
        results,
    })
}

/// Refuses a run whose reserve or price book cannot hold its spend under the cap.
fn startable(run: &Replay, recording: Recording<'_>) -> Result<(), Error> {
    if run.reserve == Amount::ZERO {
        return Err(Error::invalid("a case must reserve more than zero"));
    }
    if run.reserve > run.money_cap {
        return Err(Error::invalid(
            "a case must not reserve more than the money cap",
        ));
    }
    let priced = recording
        .prices
        .is_some_and(|prices| prices.document().currency == run.currency);
    if !priced {
        return Err(Error::invalid(
            "a replay needs a price book in the run's currency",
        ));
    }
    Ok(())
}
