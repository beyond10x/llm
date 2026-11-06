//! The label set, the two results a classifier returns, and the classifiers: two model paths
//! through the routed, recorded call of `llm-calls`, and a lexical one the caller supplies.

use llm_calls::{CallIdentity, CallRecord, CallSink, Clock, Kind, Recording, record_turn};
use llm_core::{
    BoxFuture, Cancel, Error, ErrorCode, Id, Item, OutputFormat, StreamEvent, StreamSink,
    ToolChoice, ToolName, ToolSpec, TurnOutcome, TurnRequest,
};
use llm_routing::{Catalog, FallbackPolicy, Models, Pause, Selection};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    future::{Future, poll_fn},
    pin::pin,
    task::Poll,
    time::{Duration, Instant},
};

/// The highest confidence, in basis points: certainty.
pub const MAX_BPS: u16 = 10_000;
/// The most labels one set holds.
pub const MAX_LABELS: usize = 256;
/// The longest label, in bytes.
pub const MAX_LABEL_BYTES: usize = 256;
/// The one tool a forced-tool classifier publishes and forces.
pub const TOOL_NAME: &str = "classify";
/// The name of the output format a JSON-schema classifier asks for.
pub const FORMAT_NAME: &str = "classification";

const TOOL_DESCRIPTION: &str =
    "Answer with one label of the set and a confidence per label in basis points (0 to 10000).";

/// A label set, chain or classifier that cannot be built. Each is refused before any call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupError {
    /// Fewer than two distinct labels.
    TooFewLabels,
    TooManyLabels,
    EmptyLabel,
    LabelTooLong,
    DuplicateLabel(String),
    EmptyVersion,
    /// A chain with no classifier.
    NoClassifier,
    DuplicateClassifier(Id),
    ZeroDeadline,
    /// A confidence floor above [`MAX_BPS`].
    FloorAboveMax(u16),
}

impl SetupError {
    /// The refusal's kebab-case code, as a conformance observation reports it.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::TooFewLabels => "too-few-labels",
            Self::TooManyLabels => "too-many-labels",
            Self::EmptyLabel => "empty-label",
            Self::LabelTooLong => "label-too-long",
            Self::DuplicateLabel(_) => "duplicate-label",
            Self::EmptyVersion => "empty-version",
            Self::NoClassifier => "no-classifier",
            Self::DuplicateClassifier(_) => "duplicate-classifier",
            Self::ZeroDeadline => "zero-deadline",
            Self::FloorAboveMax(_) => "floor-above-max",
        }
    }
}

impl fmt::Display for SetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooFewLabels => f.write_str("a label set needs at least two distinct labels"),
            Self::TooManyLabels => write!(f, "a label set holds at most {MAX_LABELS} labels"),
            Self::EmptyLabel => f.write_str("a label cannot be empty"),
            Self::LabelTooLong => write!(f, "a label is at most {MAX_LABEL_BYTES} bytes"),
            Self::DuplicateLabel(label) => write!(f, "the label {label:?} is given twice"),
            Self::EmptyVersion => f.write_str("a label set version cannot be empty"),
            Self::NoClassifier => f.write_str("a chain needs at least one classifier"),
            Self::DuplicateClassifier(id) => write!(f, "the classifier {id} is given twice"),
            Self::ZeroDeadline => f.write_str("a chain deadline must be longer than zero"),
            Self::FloorAboveMax(bps) => {
                write!(
                    f,
                    "a confidence floor of {bps} is above {MAX_BPS} basis points"
                )
            }
        }
    }
}

impl std::error::Error for SetupError {}

/// A versioned set of at least two distinct, non-empty labels, in the caller's order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "LabelSetDocument", into = "LabelSetDocument")]
pub struct LabelSet {
    id: Id,
    version: String,
    labels: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LabelSetDocument {
    id: Id,
    version: String,
    labels: Vec<String>,
}

impl TryFrom<LabelSetDocument> for LabelSet {
    type Error = SetupError;
    fn try_from(document: LabelSetDocument) -> Result<Self, SetupError> {
        Self::new(document.id, document.version, document.labels)
    }
}

impl From<LabelSet> for LabelSetDocument {
    fn from(set: LabelSet) -> Self {
        Self {
            id: set.id,
            version: set.version,
            labels: set.labels,
        }
    }
}

impl LabelSet {
    /// # Errors
    /// Refuses an empty version, an empty, oversized or repeated label, fewer than two labels
    /// and more than [`MAX_LABELS`].
    pub fn new(
        id: Id,
        version: impl Into<String>,
        labels: Vec<String>,
    ) -> Result<Self, SetupError> {
        let version = version.into();
        if version.is_empty() {
            return Err(SetupError::EmptyVersion);
        }
        if labels.len() > MAX_LABELS {
            return Err(SetupError::TooManyLabels);
        }
        let mut seen = BTreeSet::new();
        for label in &labels {
            if label.is_empty() {
                return Err(SetupError::EmptyLabel);
            }
            if label.len() > MAX_LABEL_BYTES {
                return Err(SetupError::LabelTooLong);
            }
            if !seen.insert(label) {
                return Err(SetupError::DuplicateLabel(label.clone()));
            }
        }
        if labels.len() < 2 {
            return Err(SetupError::TooFewLabels);
        }
        Ok(Self {
            id,
            version,
            labels,
        })
    }

    pub const fn id(&self) -> &Id {
        &self.id
    }
    pub fn version(&self) -> &str {
        &self.version
    }
    pub fn labels(&self) -> &[String] {
        &self.labels
    }
    pub fn contains(&self, label: &str) -> bool {
        self.labels.iter().any(|known| known == label)
    }

    /// The strict JSON Schema a model answer is held to: `label`, one of the labels, and
    /// `confidences`, an integer from 0 to [`MAX_BPS`] for every label; no other key.
    pub fn schema(&self) -> Value {
        let bps = json!({"type": "integer", "minimum": 0, "maximum": MAX_BPS});
        let properties: Map<String, Value> = self
            .labels
            .iter()
            .map(|label| (label.clone(), bps.clone()))
            .collect();
        json!({
            "type": "object",
            "properties": {
                "label": {"type": "string", "enum": self.labels},
                "confidences": {
                    "type": "object",
                    "properties": properties,
                    "required": self.labels,
                    "additionalProperties": false
                }
            },
            "required": ["label", "confidences"],
            "additionalProperties": false
        })
    }

    /// A model answer held to [`LabelSet::schema`]; `None` when it does not decode as the schema
    /// or names no label of the set.
    pub fn decode(&self, answer: &Value) -> Option<Answer> {
        let object = answer.as_object()?;
        if object.len() != 2 {
            return None;
        }
        let label = object.get("label")?.as_str()?;
        let given = object.get("confidences")?.as_object()?;
        if !self.contains(label) || given.len() != self.labels.len() {
            return None;
        }
        let mut confidences_bps = BTreeMap::new();
        for known in &self.labels {
            let bps = u16::try_from(given.get(known)?.as_u64()?).ok()?;
            if bps > MAX_BPS {
                return None;
            }
            confidences_bps.insert(known.clone(), bps);
        }
        Some(Answer {
            label: label.to_owned(),
            confidences_bps,
        })
    }
}

/// One answer before it is held to a label set: the label named and the confidences known. A
/// label missing from `confidences_bps` has an unknown confidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub label: String,
    #[serde(default)]
    pub confidences_bps: BTreeMap<String, u16>,
}

/// One label's confidence in basis points; unknown stays `None`, never 0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelConfidence {
    pub label: String,
    pub confidence_bps: Option<u16>,
}

/// One label of the set with a confidence per label, from one classifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub classifier_id: Id,
    pub label_set_id: Id,
    pub label_set_version: String,
    pub label: String,
    /// Every label of the set, in its order.
    pub confidences: Vec<LabelConfidence>,
    pub latency_ms: u64,
    /// The call record of the model call; absent exactly for a lexical classifier.
    pub call_record_id: Option<Id>,
}

/// Why a classifier gave no decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AbstentionCause {
    /// The chain's deadline passed before an answer: the call was cancelled, its answer arrived
    /// at or after the deadline, or no time was left to start it.
    Timeout,
    /// The call failed with this llm error code.
    ProviderError(ErrorCode),
    /// The answer named no label of the set, did not decode as the schema, or its label's
    /// confidence was under the classifier's floor.
    OffLabel,
}

impl AbstentionCause {
    /// The cause's name, as `llm.decisions.AbstentionCause` declares it.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Timeout => "Timeout",
            Self::ProviderError(_) => "ProviderError",
            Self::OffLabel => "OffLabel",
        }
    }
    pub const fn error_code(self) -> Option<ErrorCode> {
        match self {
            Self::ProviderError(code) => Some(code),
            Self::Timeout | Self::OffLabel => None,
        }
    }
}

/// No decision from one classifier, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Abstention {
    pub classifier_id: Id,
    pub cause: AbstentionCause,
    /// True when no later classifier of the chain decided: this is the chain's answer.
    #[serde(rename = "final")]
    pub is_final: bool,
    pub latency_ms: u64,
    /// The call record of the model call; absent for a lexical classifier and for a model
    /// classifier that made no call.
    pub call_record_id: Option<Id>,
}

/// What one classifier returns: a decision or an abstention, nothing else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Classification {
    Decided(Decision),
    Abstained(Abstention),
}

impl Classification {
    pub const fn classifier_id(&self) -> &Id {
        match self {
            Self::Decided(decision) => &decision.classifier_id,
            Self::Abstained(abstention) => &abstention.classifier_id,
        }
    }
    pub const fn call_record_id(&self) -> Option<&Id> {
        match self {
            Self::Decided(decision) => decision.call_record_id.as_ref(),
            Self::Abstained(abstention) => abstention.call_record_id.as_ref(),
        }
    }
}

/// How a classifier reaches its answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Path {
    /// A turn whose `controls.output_format` is the label set's strict schema.
    JsonSchema,
    /// A turn forcing one tool whose parameters are the label set's schema.
    ForcedTool,
    /// A classifier the caller supplies; no model call.
    Lexical,
}

/// The caller's side of a classification: the catalog and models a model path routes through,
/// the limit decision consulted before every attempt, cancellation, the timer, and where each
/// call's record goes.
#[derive(Clone, Copy)]
pub struct Ports<'p> {
    pub catalog: &'p Catalog,
    pub models: &'p dyn Models,
    pub admit: &'p (dyn Fn(&Selection<'_>) -> Result<(), Error> + Sync),
    /// Cancelling it cancels the call in progress; that call abstains with its code.
    pub cancel: &'p Cancel,
    /// Waits a duration: the chain's deadline timer and routing's retry waits. A Tokio caller
    /// passes `&|wait| Box::pin(tokio::time::sleep(wait))`.
    pub pause: Pause<'p>,
    /// Its clock measures latency and the deadline; its sink receives one record per model call.
    pub recording: Recording<'p>,
}

/// One question a chain puts to one classifier.
#[derive(Clone, Copy)]
pub struct Ask<'a> {
    pub label_set: &'a LabelSet,
    pub input: &'a str,
    /// Time left before the chain's deadline; zero once it has passed.
    pub remaining: Duration,
    /// The chain request's id; a model call's record id is `<request id>/<classifier id>`.
    pub request_id: &'a Id,
    /// The caller's estimate of a model turn's input tokens, which routing admits each target
    /// against; llm counts none, and without one routing finds no compatible target.
    pub input_tokens: Option<u64>,
    pub ports: Ports<'a>,
}

/// A classifier: one decision or one abstention per question.
pub trait Classifier: Send + Sync {
    fn id(&self) -> &Id;
    fn path(&self) -> Path;
    fn min_confidence_bps(&self) -> Option<u16>;
    fn classify<'a>(&'a self, ask: Ask<'a>) -> BoxFuture<'a, Classification>;
}

/// A classifier the caller writes, with no model call: `None` abstains `OffLabel`.
pub trait Lexical: Send + Sync {
    fn classify(&self, label_set: &LabelSet, input: &str) -> Option<Answer>;
}

fn floor(bps: u16) -> Result<u16, SetupError> {
    if bps > MAX_BPS {
        return Err(SetupError::FloorAboveMax(bps));
    }
    Ok(bps)
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Holds an answer to the set and the floor: a decision, or an abstention `OffLabel`.
fn settle(
    classifier_id: &Id,
    ask: &Ask<'_>,
    answer: Option<Answer>,
    min_confidence_bps: Option<u16>,
    latency_ms: u64,
    call_record_id: Option<Id>,
) -> Classification {
    let set = ask.label_set;
    let decided = answer.filter(|answer| {
        set.contains(&answer.label)
            && answer
                .confidences_bps
                .iter()
                .all(|(label, bps)| set.contains(label) && *bps <= MAX_BPS)
            && min_confidence_bps.is_none_or(|floor| {
                answer
                    .confidences_bps
                    .get(&answer.label)
                    .is_some_and(|bps| *bps >= floor)
            })
    });
    let Some(answer) = decided else {
        return abstain(
            classifier_id,
            AbstentionCause::OffLabel,
            latency_ms,
            call_record_id,
        );
    };
    Classification::Decided(Decision {
        classifier_id: classifier_id.clone(),
        label_set_id: set.id.clone(),
        label_set_version: set.version.clone(),
        confidences: set
            .labels
            .iter()
            .map(|label| LabelConfidence {
                label: label.clone(),
                confidence_bps: answer.confidences_bps.get(label).copied(),
            })
            .collect(),
        label: answer.label,
        latency_ms,
        call_record_id,
    })
}

fn abstain(
    classifier_id: &Id,
    cause: AbstentionCause,
    latency_ms: u64,
    call_record_id: Option<Id>,
) -> Classification {
    Classification::Abstained(Abstention {
        classifier_id: classifier_id.clone(),
        cause,
        is_final: false,
        latency_ms,
        call_record_id,
    })
}

/// A caller's [`Lexical`] classifier with an id and an optional confidence floor.
pub struct LexicalClassifier<L> {
    id: Id,
    lexical: L,
    min_confidence_bps: Option<u16>,
}

impl<L: Lexical> LexicalClassifier<L> {
    pub const fn new(id: Id, lexical: L) -> Self {
        Self {
            id,
            lexical,
            min_confidence_bps: None,
        }
    }

    /// An answer whose label's confidence is under `bps`, or unknown, abstains `OffLabel`.
    ///
    /// # Errors
    /// Refuses a floor above [`MAX_BPS`].
    pub fn with_min_confidence_bps(mut self, bps: u16) -> Result<Self, SetupError> {
        self.min_confidence_bps = Some(floor(bps)?);
        Ok(self)
    }
}

impl<L: Lexical> Classifier for LexicalClassifier<L> {
    fn id(&self) -> &Id {
        &self.id
    }
    fn path(&self) -> Path {
        Path::Lexical
    }
    fn min_confidence_bps(&self) -> Option<u16> {
        self.min_confidence_bps
    }
    fn classify<'a>(&'a self, ask: Ask<'a>) -> BoxFuture<'a, Classification> {
        Box::pin(async move {
            let clock = ask.ports.recording.clock;
            let start = clock.monotonic();
            let answer = self.lexical.classify(ask.label_set, ask.input);
            let latency_ms = millis(clock.monotonic().saturating_sub(start));
            settle(
                &self.id,
                &ask,
                answer,
                self.min_confidence_bps,
                latency_ms,
                None,
            )
        })
    }
}

/// A classifier that asks a routed model, on the JSON-schema or the forced-tool path, through
/// [`llm_calls::record_turn`]: every call it makes writes one record of kind `Classification`.
#[derive(Debug, Clone)]
pub struct ModelClassifier {
    id: Id,
    path: Path,
    route: String,
    instructions: String,
    min_confidence_bps: Option<u16>,
    policy: FallbackPolicy,
}

impl ModelClassifier {
    /// Asks the route alias `route` for an answer held to the label set's strict schema.
    pub fn json_schema(id: Id, route: impl Into<String>) -> Self {
        Self::on(id, Path::JsonSchema, route.into())
    }

    /// Asks the route alias `route` for one call to the forced tool [`TOOL_NAME`].
    pub fn forced_tool(id: Id, route: impl Into<String>) -> Self {
        Self::on(id, Path::ForcedTool, route.into())
    }

    fn on(id: Id, path: Path, route: String) -> Self {
        Self {
            id,
            path,
            route,
            instructions: String::new(),
            min_confidence_bps: None,
            policy: FallbackPolicy::default(),
        }
    }

    /// The standing instruction of every turn: the caller's prompt.
    #[must_use]
    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = instructions.into();
        self
    }

    /// An answer whose label's confidence is under `bps` abstains `OffLabel`.
    ///
    /// # Errors
    /// Refuses a floor above [`MAX_BPS`].
    pub fn with_min_confidence_bps(mut self, bps: u16) -> Result<Self, SetupError> {
        self.min_confidence_bps = Some(floor(bps)?);
        Ok(self)
    }

    /// The routing bounds of each call; the chain's deadline still applies.
    #[must_use]
    pub const fn with_policy(mut self, policy: FallbackPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// The turn this classifier sends for `input`.
    ///
    /// # Errors
    /// Refuses only a tool name llm does not accept, which [`TOOL_NAME`] is not.
    pub fn request(&self, label_set: &LabelSet, input: &str) -> Result<TurnRequest, Error> {
        let mut request = TurnRequest::new(self.route.clone(), vec![Item::user(input)]);
        request.instructions.clone_from(&self.instructions);
        if self.path == Path::ForcedTool {
            let name = ToolName::new(TOOL_NAME).map_err(|_| Error::invalid("invalid tool name"))?;
            request.tools = vec![ToolSpec {
                name: name.clone(),
                description: TOOL_DESCRIPTION.to_owned(),
                input_schema: label_set.schema(),
            }];
            request.tool_choice = ToolChoice::Named(name);
        } else {
            request.controls.output_format = Some(
                OutputFormat::json_schema(label_set.schema())
                    .with_name(FORMAT_NAME)
                    .with_strict(true),
            );
        }
        Ok(request)
    }

    /// The answer the outcome carries, held to the set's schema.
    fn answer(&self, label_set: &LabelSet, outcome: &TurnOutcome) -> Option<Answer> {
        let value = if self.path == Path::ForcedTool {
            let mut calls = outcome.tool_calls();
            let call = calls.next()?;
            if calls.next().is_some() || call.name.as_str() != TOOL_NAME {
                return None;
            }
            call.arguments.clone()
        } else {
            let text: String = outcome
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::AssistantText { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            serde_json::from_str(&text).ok()?
        };
        label_set.decode(&value)
    }

    async fn ask(&self, ask: Ask<'_>) -> Classification {
        let ports = ask.ports;
        let clock: &dyn Clock = ports.recording.clock;
        let start = clock.monotonic();
        let latency = || millis(clock.monotonic().saturating_sub(start));
        if ask.remaining.is_zero() {
            return abstain(&self.id, AbstentionCause::Timeout, 0, None);
        }
        let record_id = Id::new(format!("{}/{}", ask.request_id, self.id));
        let request = self.request(ask.label_set, ask.input);
        let (Ok(record_id), Ok(request)) = (record_id, request) else {
            let cause = AbstentionCause::ProviderError(ErrorCode::InvalidRequest);
            return abstain(&self.id, cause, latency(), None);
        };
        let mut policy = self.policy;
        // No attempt starts, and no retry wait begins that would end, past the chain's deadline.
        if let Some(by) = Instant::now().checked_add(ask.remaining) {
            policy.deadline = Some(policy.deadline.map_or(by, |own| own.min(by)));
        }
        let cancel = ports.cancel.child_token();
        let classifying = Classifying(ports.recording.sink);
        let mut admit = |selection: &Selection<'_>| (ports.admit)(selection);
        let mut discard = Discard;
        let call = record_turn(
            ports.catalog,
            &request,
            ask.input_tokens,
            policy,
            llm_routing::Ports {
                models: ports.models,
                admit: &mut admit,
                sink: &mut discard,
                cancel: &cancel,
                pause: ports.pause,
            },
            CallIdentity::primary(record_id.clone()),
            Recording {
                sink: &classifying,
                ..ports.recording
            },
        );
        let (ran, cancelled) = race(call, (ports.pause)(ask.remaining), &cancel).await;
        let latency_ms = latency();
        // An answer at or after the deadline is not used, whoever noticed the deadline first.
        let timed_out = cancelled || latency_ms >= millis(ask.remaining);
        let run = match ran {
            // Refused before any attempt: no call was made and no record written.
            Err(error) => {
                let cause = if timed_out || error.code == ErrorCode::Deadline {
                    AbstentionCause::Timeout
                } else {
                    AbstentionCause::ProviderError(error.code)
                };
                return abstain(&self.id, cause, latency_ms, None);
            }
            Ok(run) => run,
        };
        let record = Some(record_id);
        if timed_out {
            return abstain(&self.id, AbstentionCause::Timeout, latency_ms, record);
        }
        match run.result {
            Err(error) if error.code == ErrorCode::Deadline => {
                abstain(&self.id, AbstentionCause::Timeout, latency_ms, record)
            }
            Err(error) => abstain(
                &self.id,
                AbstentionCause::ProviderError(error.code),
                latency_ms,
                record,
            ),
            Ok(outcome) => settle(
                &self.id,
                &ask,
                self.answer(ask.label_set, &outcome),
                self.min_confidence_bps,
                latency_ms,
                record,
            ),
        }
    }
}

impl Classifier for ModelClassifier {
    fn id(&self) -> &Id {
        &self.id
    }
    fn path(&self) -> Path {
        self.path
    }
    fn min_confidence_bps(&self) -> Option<u16> {
        self.min_confidence_bps
    }
    fn classify<'a>(&'a self, ask: Ask<'a>) -> BoxFuture<'a, Classification> {
        Box::pin(self.ask(ask))
    }
}

/// Runs `call` until it returns or `timer` fires; when the timer fires first, cancels the call
/// and still awaits it, so its record is written. Returns whether the timer fired.
async fn race<T>(
    call: impl Future<Output = T>,
    mut timer: BoxFuture<'static, ()>,
    cancel: &Cancel,
) -> (T, bool) {
    let mut call = pin!(call);
    let first = poll_fn(|context| {
        if let Poll::Ready(value) = call.as_mut().poll(context) {
            return Poll::Ready(Some(value));
        }
        if timer.as_mut().poll(context).is_ready() {
            return Poll::Ready(None);
        }
        Poll::Pending
    })
    .await;
    if let Some(value) = first {
        return (value, false);
    }
    cancel.cancel();
    (call.await, true)
}

/// Passes each record on as a classification call.
struct Classifying<'s>(&'s dyn CallSink);

impl CallSink for Classifying<'_> {
    fn record(&self, mut record: CallRecord) {
        record.kind = Kind::Classification;
        self.0.record(record);
    }
}

/// A classifier reads only the finished outcome.
struct Discard;

impl StreamSink for Discard {
    fn emit(&mut self, _event: StreamEvent) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async { Ok(()) })
    }
}
