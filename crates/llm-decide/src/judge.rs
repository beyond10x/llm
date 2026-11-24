//! The rubric, the verdict, and the one routed, recorded call that judges a subject against a
//! rubric.

use crate::{
    answer,
    classify::{Discard, Ports},
    schema,
};
use llm_calls::{CallIdentity, CallRecord, CallSink, Kind, Recording, record_turn};
use llm_core::{Error, ErrorCode, Id, Item, OutputFormat, TurnRequest};
use llm_routing::{FallbackPolicy, Selection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt;

/// The name of the output format a judge call asks for.
pub const VERDICT_FORMAT_NAME: &str = "verdict";

/// A rubric that cannot be built, or cannot be judged with over the caller's catalog. Each is
/// refused before any call.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RubricError {
    EmptyVersion,
    EmptyText,
    /// The verdict schema is not a JSON object, uses a keyword an answer is not checked against,
    /// gives a checked keyword a value it cannot have, or nests too deep.
    UnsupportedSchema,
    /// The rubric's route does not pin one model: it has `targets` targets, not one, or its
    /// fallback is on.
    RouteNotPinned {
        targets: usize,
        fallback_enabled: bool,
    },
}

impl RubricError {
    /// The refusal's kebab-case code, as a conformance observation reports it.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::EmptyVersion => "empty-version",
            Self::EmptyText => "empty-text",
            Self::UnsupportedSchema => "unsupported-schema",
            Self::RouteNotPinned { .. } => "route-not-pinned",
        }
    }
}

impl fmt::Display for RubricError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyVersion => f.write_str("a rubric version cannot be empty"),
            Self::EmptyText => f.write_str("a rubric text cannot be empty"),
            Self::UnsupportedSchema => f.write_str(
                "a verdict schema is a JSON object using only type, properties, required, \
                 additionalProperties, items, enum, const, minimum, maximum, anyOf, title and \
                 description",
            ),
            Self::RouteNotPinned {
                targets,
                fallback_enabled,
            } => write!(
                f,
                "a rubric's route needs exactly one target and fallback off; this one has \
                 {targets} and fallback {}",
                if *fallback_enabled { "on" } else { "off" }
            ),
        }
    }
}

impl std::error::Error for RubricError {}

/// Why [`judge`] returned no verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum JudgeError {
    /// The rubric cannot be judged with over this catalog; no call was made.
    Rubric(RubricError),
    /// Routing refused the call, the call failed, or its answer named no upstream model. A call
    /// that was attempted has its record in the caller's sink.
    Call(Error),
}

impl fmt::Display for JudgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rubric(error) => error.fmt(f),
            Self::Call(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for JudgeError {}

impl From<RubricError> for JudgeError {
    fn from(error: RubricError) -> Self {
        Self::Rubric(error)
    }
}

impl From<Error> for JudgeError {
    fn from(error: Error) -> Self {
        Self::Call(error)
    }
}

/// What a subject is judged against: a versioned text, the JSON Schema of the verdict, and the
/// route alias of the one model that judges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rubric {
    id: Id,
    version: String,
    text: String,
    schema: Value,
    route: String,
    digest: String,
}

impl Rubric {
    /// # Errors
    /// Refuses an empty version, an empty text, and a schema an answer cannot be held to
    /// ([`RubricError::UnsupportedSchema`]).
    pub fn new(
        id: Id,
        version: impl Into<String>,
        text: impl Into<String>,
        schema: &Value,
        route: impl Into<String>,
    ) -> Result<Self, RubricError> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let (version, text) = (version.into(), text.into());
        if version.is_empty() {
            return Err(RubricError::EmptyVersion);
        }
        if text.is_empty() {
            return Err(RubricError::EmptyText);
        }
        if !schema::supported(schema) {
            return Err(RubricError::UnsupportedSchema);
        }
        // Kept with its keys in canonical order and every scalar as it was given, so the
        // stored schema and the request carry the bytes the digest covers, however the
        // caller built the schema. Nothing is written and read back.
        let schema = ordered(schema);
        let canonical_schema = canonical(&schema);
        let mut hasher = Sha256::new();
        hasher.update(u64::try_from(text.len()).unwrap_or(u64::MAX).to_be_bytes());
        hasher.update(text.as_bytes());
        hasher.update(canonical_schema.as_bytes());
        let mut digest = String::with_capacity(64);
        for byte in hasher.finalize() {
            digest.push(char::from(HEX[usize::from(byte >> 4)]));
            digest.push(char::from(HEX[usize::from(byte & 15)]));
        }
        Ok(Self {
            id,
            version,
            text,
            schema,
            route: route.into(),
            digest,
        })
    }

    pub const fn id(&self) -> &Id {
        &self.id
    }
    pub fn version(&self) -> &str {
        &self.version
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    /// The verdict schema, its keys in canonical order.
    pub const fn schema(&self) -> &Value {
        &self.schema
    }
    /// The route alias the judge call names.
    pub fn route(&self) -> &str {
        &self.route
    }
    /// Lowercase hex sha256 of: the text's length in bytes as eight big-endian bytes, the text,
    /// and the schema as canonical JSON. The id, the version and the route do not enter it.
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// The turn a judge call sends for `subject`, before its sampling is fixed: the text as the
    /// instructions, the subject as the one user item, the schema as the strict output format.
    fn request(&self, subject: &str) -> TurnRequest {
        let mut request = TurnRequest::new(self.route.clone(), vec![Item::user(subject)]);
        request.instructions.clone_from(&self.text);
        request.controls.output_format = Some(
            OutputFormat::json_schema(self.schema.clone())
                .with_name(VERDICT_FORMAT_NAME)
                .with_strict(true),
        );
        request
    }
}

/// One rubric's judgement of one subject, with what produced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verdict {
    pub rubric_id: Id,
    pub rubric_version: String,
    /// [`Rubric::digest`].
    pub rubric_digest: String,
    /// The route's one target, which answered.
    pub route_target_id: Id,
    /// The model the far side reported, not the name the catalog declares.
    pub upstream_model: Id,
    /// The answer as canonical JSON, valid against the rubric's schema: object keys sorted, no
    /// whitespace, an integer with every digit, any other number as the shortest decimal that
    /// reads back as the same double. Absent exactly when `error_code` is set.
    pub verdict_json: Option<String>,
    /// `protocol` when the answer was not one whole, unambiguous JSON value or not valid against
    /// the schema. Such an answer is refused as it is, never repaired.
    pub error_code: Option<ErrorCode>,
    /// The call's record in the caller's sink: `<request id>/<rubric id>`, kind `Verdict`.
    pub call_record_id: Id,
}

/// Judges `subject` against `rubric` with one call to the rubric's route.
///
/// The request is the rubric's text as the instructions, `subject` as the one user item, the
/// rubric's schema as the strict output format [`VERDICT_FORMAT_NAME`], and temperature 0 where
/// the route's target declares temperature: its bytes are a function of rubric and subject. The
/// route must have exactly one target and fallback off, so the model cannot change between runs.
/// The call goes through [`llm_calls::record_turn`] with no fallback and the same-target retry
/// of every routed call, and writes one record of kind `Verdict` with the id
/// `<request id>/<rubric id>`. As for every routed turn, `input_tokens` is the caller's estimate
/// routing admits the target against; llm counts no tokens.
///
/// An answer comes back as a [`Verdict`] with `error_code` `protocol` and no JSON when it is not
/// the one whole JSON value the model ended on (the turn stopped at the output limit or was
/// reported incomplete, the outcome holds no assistant text or more than one, or a refusal
/// beside it, an object gives a key twice, an integer is outside what `i64` and `u64` hold), or
/// when that value is not valid against the schema.
///
/// # Errors
/// [`JudgeError::Rubric`] for a route that does not pin one model, before any call.
/// [`JudgeError::Call`] for what routing refuses (an unknown alias, no compatible target), for
/// a failed call, and with code `protocol` for an answer whose far side reported no upstream
/// model: a verdict cannot pin what it was not told.
pub async fn judge(
    rubric: &Rubric,
    subject: &str,
    request_id: &Id,
    input_tokens: Option<u64>,
    ports: Ports<'_>,
) -> Result<Verdict, JudgeError> {
    let mut request = rubric.request(subject);
    let explanation = ports.catalog.explain(&request, input_tokens)?;
    let fallback_enabled = ports
        .catalog
        .routes()
        .any(|route| route.id == explanation.route_id && route.fallback_enabled);
    let target = match explanation.targets.as_slice() {
        [target] if !fallback_enabled => target,
        targets => {
            return Err(JudgeError::Rubric(RubricError::RouteNotPinned {
                targets: targets.len(),
                fallback_enabled,
            }));
        }
    };
    if target.capabilities.temperature {
        request.sampling.temperature = Some(0.0);
    }
    let record_id = Id::new(format!("{request_id}/{}", rubric.id))
        .map_err(|_| Error::invalid("the request id and rubric id make no call record id"))?;
    let judging = Judging(ports.recording.sink);
    let mut admit = |selection: &Selection<'_>| (ports.admit)(selection);
    let mut discard = Discard;
    let run = record_turn(
        ports.catalog,
        &request,
        input_tokens,
        FallbackPolicy {
            max_attempts: 1,
            ..FallbackPolicy::default()
        },
        llm_routing::Ports {
            models: ports.models,
            admit: &mut admit,
            sink: &mut discard,
            cancel: ports.cancel,
            pause: ports.pause,
        },
        CallIdentity::primary(record_id.clone()),
        Recording {
            sink: &judging,
            ..ports.recording
        },
    )
    .await?;
    let outcome = run.result?;
    let (Some(attempt), Some(upstream_model)) = (
        run.attempts.last(),
        outcome.observation.upstream_model.clone(),
    ) else {
        return Err(JudgeError::Call(Error::new(
            ErrorCode::Protocol,
            "the answer names no upstream model, so its verdict cannot pin one",
        )));
    };
    let verdict_json = answer::whole(&outcome)
        .filter(|answer| schema::holds(&rubric.schema, answer))
        .map(|answer| canonical(&answer));
    Ok(Verdict {
        rubric_id: rubric.id.clone(),
        rubric_version: rubric.version.clone(),
        rubric_digest: rubric.digest.clone(),
        route_target_id: attempt.target_id.clone(),
        upstream_model,
        error_code: verdict_json.is_none().then_some(ErrorCode::Protocol),
        verdict_json,
        call_record_id: record_id,
    })
}

/// `value` with the keys of every object in canonical order and every scalar as it is: no number
/// is written and read back, so none changes.
fn ordered(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(ordered).collect()),
        Value::Object(members) => {
            let mut members: Vec<(&String, &Value)> = members.iter().collect();
            members.sort_by_key(|(name, _)| *name);
            Value::Object(
                members
                    .into_iter()
                    .map(|(name, member)| (name.clone(), ordered(member)))
                    .collect(),
            )
        }
        scalar => scalar.clone(),
    }
}

/// `value` as canonical JSON: the keys of every object sorted, no whitespace, every scalar as
/// `serde_json` writes it (an integer with every digit, any other number as the shortest decimal
/// that reads back as the same double). It does not depend on the order a map was built in.
fn canonical(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
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
                write_canonical(member, out);
            }
            out.push('}');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

/// Passes each record on as a verdict call.
struct Judging<'s>(&'s dyn CallSink);

impl CallSink for Judging<'_> {
    fn record(&self, mut record: CallRecord) {
        record.kind = Kind::Verdict;
        self.0.record(record);
    }
}
