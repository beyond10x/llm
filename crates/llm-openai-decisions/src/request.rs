//! The decision request: its nouns, the reference's bounds, and its one reading from JSON.
//!
//! Every member name, question type, enum value and bound is read from `openai/openai-openapi`
//! at commit `b4cad10d7cff7fc286f2dbf1992569989522c6f0`, schemas `DecisionRequest`,
//! `QuestionParamPredicate`, `QuestionParamChoice`, `QuestionParamScore`, `ChoiceOptionParam` and
//! `ScoreLevelParam`, each of which closes its members (`additionalProperties: false`).

use crate::strict;
use llm_core::Error;
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::HashSet;

/// The most questions one request asks; the fewest is one.
pub const MAX_QUESTIONS: usize = 400;
/// The fewest choices of a choice question.
pub const MIN_CHOICES: usize = 2;
/// The most choices of a choice question.
pub const MAX_CHOICES: usize = 255;
/// The fewest levels of a score question.
pub const MIN_LEVELS: usize = 2;
/// The most levels of a score question.
pub const MAX_LEVELS: usize = 10;
/// The longest name, instruction, text choice value, description or level label, in characters.
pub const MAX_TEXT_CHARS: usize = 1_048_576;
/// The longest input, in characters.
pub const MAX_INPUT_CHARS: usize = 10_485_760;
/// The longest `safety_identifier`, in characters.
pub const MAX_SAFETY_IDENTIFIER_CHARS: usize = 128;
/// The longest `prompt_cache_key`, in characters.
pub const MAX_PROMPT_CACHE_KEY_CHARS: usize = 64;
/// The longest member name a refusal writes into its path, in bytes.
pub const MAX_NAMED_MEMBER_BYTES: usize = 64;

/// A typed choice value: the text `"true"` and the boolean `true` are distinct.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(untagged)]
pub enum ChoiceValue {
    Text(String),
    Flag(bool),
}

/// One choice of a choice question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChoiceOption {
    pub value: ChoiceValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl ChoiceOption {
    /// A text choice with no description.
    pub fn text(value: impl Into<String>) -> Self {
        Self {
            value: ChoiceValue::Text(value.into()),
            description: None,
        }
    }
}

/// One level of a score question; the levels are ordered as given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScoreLevel {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// The kind of a question, as `llm.decisions.QuestionKind` declares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestionKind {
    Predicate,
    Choice,
    Score,
}

impl QuestionKind {
    /// The wire's `type` of the question and of its answer.
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Predicate => "predicate",
            Self::Choice => "choice",
            Self::Score => "score",
        }
    }
}

/// One question about the request's input. `name` is the caller's correlation name; the answer
/// carries it back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Question {
    /// How likely a statement about the input is true.
    Predicate {
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        instructions: String,
    },
    /// One of the supplied choices.
    Choice {
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        instructions: String,
        choices: Vec<ChoiceOption>,
    },
    /// A rating against the supplied ordered levels.
    Score {
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        instructions: String,
        levels: Vec<ScoreLevel>,
    },
}

impl Question {
    pub const fn kind(&self) -> QuestionKind {
        match self {
            Self::Predicate { .. } => QuestionKind::Predicate,
            Self::Choice { .. } => QuestionKind::Choice,
            Self::Score { .. } => QuestionKind::Score,
        }
    }
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::Predicate { name, .. } | Self::Choice { name, .. } | Self::Score { name, .. } => {
                name.as_deref()
            }
        }
    }
    fn instructions(&self) -> &str {
        match self {
            Self::Predicate { instructions, .. }
            | Self::Choice { instructions, .. }
            | Self::Score { instructions, .. } => instructions,
        }
    }
}

/// The lifetime of a prompt cache entry; the reference names one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PromptCacheTtl {
    #[serde(rename = "30m")]
    ThirtyMinutes,
}

/// Whether the provider places an implicit cache breakpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptCacheMode {
    Implicit,
    Explicit,
}

/// The request's prompt cache options; an absent member is left to the provider's default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct PromptCacheOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<PromptCacheTtl>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<PromptCacheMode>,
}

/// One decision request: an input and the questions about it.
///
/// `model` is the catalog route alias the caller selects a target with; the body sent carries
/// that target's upstream model instead. `input` is a text; the reference's other input form, a
/// list of items, is not sent by this client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionRequest {
    pub model: String,
    pub input: String,
    pub questions: Vec<Question>,
    pub safety_identifier: Option<String>,
    pub prompt_cache_key: Option<String>,
    pub prompt_cache_options: Option<PromptCacheOptions>,
}

/// The body as the wire carries it: the reference's members and no other.
#[derive(Serialize)]
pub(crate) struct Wire<'r> {
    model: &'r str,
    input: &'r str,
    questions: &'r [Question],
    #[serde(skip_serializing_if = "Option::is_none")]
    safety_identifier: Option<&'r str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_cache_key: Option<&'r str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_cache_options: Option<PromptCacheOptions>,
}

fn outside_bounds(path: &str) -> Error {
    Error::invalid(format!(
        "decision request is outside the reference's bounds: {path}"
    ))
}

fn unnamed(path: &str) -> Error {
    Error::invalid(format!(
        "decision request carries a member the reference does not name: {path}"
    ))
}

fn missing(path: &str) -> Error {
    Error::invalid(format!(
        "decision request lacks a member the reference requires: {path}"
    ))
}

fn mistyped(path: &str) -> Error {
    Error::invalid(format!(
        "decision request holds a member of the wrong type: {path}"
    ))
}

fn outside_values(path: &str) -> Error {
    Error::invalid(format!(
        "decision request names a value outside the reference: {path}"
    ))
}

fn unsent(path: &str) -> Error {
    Error::unsupported(format!(
        "decision request uses a form this client does not send: {path}"
    ))
}

/// Whether `text` has at most `max` characters. Its bytes are at least its characters, so only a
/// text longer than `max` bytes is counted.
fn fits(text: &str, max: usize) -> bool {
    text.len() <= max || text.chars().count() <= max
}

fn bounded(text: &str, max: usize, path: impl FnOnce() -> String) -> Result<(), Error> {
    if fits(text, max) {
        Ok(())
    } else {
        Err(outside_bounds(&path()))
    }
}

impl DecisionRequest {
    /// A request with no safety identifier and no prompt cache member.
    pub fn new(
        model: impl Into<String>,
        input: impl Into<String>,
        questions: Vec<Question>,
    ) -> Self {
        Self {
            model: model.into(),
            input: input.into(),
            questions,
            safety_identifier: None,
            prompt_cache_key: None,
            prompt_cache_options: None,
        }
    }

    pub(crate) fn wire<'r>(&'r self, upstream_model: &'r str) -> Wire<'r> {
        Wire {
            model: upstream_model,
            input: &self.input,
            questions: &self.questions,
            safety_identifier: self.safety_identifier.as_deref(),
            prompt_cache_key: self.prompt_cache_key.as_deref(),
            prompt_cache_options: self.prompt_cache_options,
        }
    }

    /// Holds the request to the reference's bounds.
    ///
    /// # Errors
    /// `invalid-request`, naming the path of the first member outside its bound: fewer than one
    /// or more than [`MAX_QUESTIONS`] questions; a choice question with fewer than
    /// [`MIN_CHOICES`] or more than [`MAX_CHOICES`] choices, or with one typed value twice; a
    /// score question with fewer than [`MIN_LEVELS`] or more than [`MAX_LEVELS`] levels; a text
    /// longer than its bound in characters.
    pub fn validate(&self) -> Result<(), Error> {
        bounded(&self.input, MAX_INPUT_CHARS, || "input".to_owned())?;
        if self.questions.is_empty() || self.questions.len() > MAX_QUESTIONS {
            return Err(outside_bounds("questions"));
        }
        for (position, question) in self.questions.iter().enumerate() {
            let at = |member: &str| format!("questions[{position}].{member}");
            if let Some(name) = question.name() {
                bounded(name, MAX_TEXT_CHARS, || at("name"))?;
            }
            bounded(question.instructions(), MAX_TEXT_CHARS, || {
                at("instructions")
            })?;
            match question {
                Question::Predicate { .. } => {}
                Question::Choice { choices, .. } => {
                    if !(MIN_CHOICES..=MAX_CHOICES).contains(&choices.len()) {
                        return Err(outside_bounds(&at("choices")));
                    }
                    let mut seen = HashSet::with_capacity(choices.len());
                    for (index, choice) in choices.iter().enumerate() {
                        let value = || at(&format!("choices[{index}].value"));
                        if let ChoiceValue::Text(text) = &choice.value {
                            bounded(text, MAX_TEXT_CHARS, value)?;
                        }
                        if !seen.insert(&choice.value) {
                            return Err(outside_bounds(&value()));
                        }
                        if let Some(description) = &choice.description {
                            bounded(description, MAX_TEXT_CHARS, || {
                                at(&format!("choices[{index}].description"))
                            })?;
                        }
                    }
                }
                Question::Score { levels, .. } => {
                    if !(MIN_LEVELS..=MAX_LEVELS).contains(&levels.len()) {
                        return Err(outside_bounds(&at("levels")));
                    }
                    for (index, level) in levels.iter().enumerate() {
                        bounded(&level.label, MAX_TEXT_CHARS, || {
                            at(&format!("levels[{index}].label"))
                        })?;
                        if let Some(description) = &level.description {
                            bounded(description, MAX_TEXT_CHARS, || {
                                at(&format!("levels[{index}].description"))
                            })?;
                        }
                    }
                }
            }
        }
        if let Some(identifier) = &self.safety_identifier {
            bounded(identifier, MAX_SAFETY_IDENTIFIER_CHARS, || {
                "safety_identifier".to_owned()
            })?;
        }
        if let Some(key) = &self.prompt_cache_key {
            bounded(key, MAX_PROMPT_CACHE_KEY_CHARS, || {
                "prompt_cache_key".to_owned()
            })?;
        }
        Ok(())
    }

    /// Reads one request in the reference's shape, whose `model` is the route alias, and holds
    /// it to [`DecisionRequest::validate`]. `null` for `safety_identifier`, `prompt_cache_key`,
    /// `prompt_cache_options` and its `mode` is the member left out, as the reference allows.
    ///
    /// # Errors
    /// `invalid-request` when the text is not one whole JSON document (cut short, followed by
    /// anything, a member given twice, nested deeper than [`crate::MAX_DEPTH`]), and, naming the
    /// member's path: a member the reference does not name, a required member missing, a member
    /// of the wrong JSON type, a question type or enum value outside the reference, a bound
    /// passed. `unsupported`, naming the path, for a form the reference names and this client
    /// does not send: an `input` given as items, `instructions` given as content parts. A
    /// message names a path and never a value; a member name longer than
    /// [`MAX_NAMED_MEMBER_BYTES`], or with a character outside ASCII letters, digits, `_` and
    /// `-`, is not written.
    pub fn from_json(text: &str) -> Result<Self, Error> {
        let document = strict::document(text.as_bytes()).ok_or_else(|| {
            Error::invalid(format!(
                "decision request is not one whole JSON document with every member once, nested at most {} deep",
                strict::MAX_DEPTH
            ))
        })?;
        let root = document.as_object().ok_or_else(|| mistyped("(document)"))?;
        closed(
            root,
            &[
                "model",
                "input",
                "questions",
                "safety_identifier",
                "prompt_cache_key",
                "prompt_cache_options",
            ],
            "",
        )?;
        let model = required_text(root, "model", "")?;
        let input = match root.get("input") {
            None => return Err(missing("input")),
            Some(Value::String(input)) => input.clone(),
            Some(Value::Array(_)) => return Err(unsent("input")),
            Some(_) => return Err(mistyped("input")),
        };
        let questions = match root.get("questions") {
            None => return Err(missing("questions")),
            Some(Value::Array(questions)) => questions
                .iter()
                .enumerate()
                .map(|(position, question)| {
                    read_question(question, &format!("questions[{position}]"))
                })
                .collect::<Result<Vec<_>, _>>()?,
            Some(_) => return Err(mistyped("questions")),
        };
        let request = Self {
            model,
            input,
            questions,
            safety_identifier: nullable_text(root, "safety_identifier")?,
            prompt_cache_key: nullable_text(root, "prompt_cache_key")?,
            prompt_cache_options: read_cache_options(root)?,
        };
        request.validate()?;
        Ok(request)
    }
}

fn join(parent: &str, member: &str) -> String {
    if parent.is_empty() {
        member.to_owned()
    } else {
        format!("{parent}.{member}")
    }
}

/// Refuses the first member, in name order, that `known` does not hold.
fn closed(object: &Map<String, Value>, known: &[&str], parent: &str) -> Result<(), Error> {
    match object.keys().find(|name| !known.contains(&name.as_str())) {
        None => Ok(()),
        Some(name) => {
            let writable = !name.is_empty()
                && name.len() <= MAX_NAMED_MEMBER_BYTES
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
            let shown = if writable {
                name
            } else {
                "(a name not written)"
            };
            Err(unnamed(&join(parent, shown)))
        }
    }
}

fn required_text(object: &Map<String, Value>, member: &str, parent: &str) -> Result<String, Error> {
    match object.get(member) {
        None => Err(missing(&join(parent, member))),
        Some(Value::String(text)) => Ok(text.clone()),
        Some(_) => Err(mistyped(&join(parent, member))),
    }
}

fn optional_text(
    object: &Map<String, Value>,
    member: &str,
    parent: &str,
) -> Result<Option<String>, Error> {
    match object.get(member) {
        None => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(mistyped(&join(parent, member))),
    }
}

fn nullable_text(object: &Map<String, Value>, member: &str) -> Result<Option<String>, Error> {
    match object.get(member) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(mistyped(member)),
    }
}

/// The members of an array of objects at `path`, each read by `read` with its own path.
fn objects<T>(
    object: &Map<String, Value>,
    member: &str,
    parent: &str,
    read: impl Fn(&Map<String, Value>, &str) -> Result<T, Error>,
) -> Result<Vec<T>, Error> {
    let path = join(parent, member);
    match object.get(member) {
        None => Err(missing(&path)),
        Some(Value::Array(items)) => items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let at = format!("{path}[{index}]");
                read(item.as_object().ok_or_else(|| mistyped(&at))?, &at)
            })
            .collect(),
        Some(_) => Err(mistyped(&path)),
    }
}

fn read_question(question: &Value, path: &str) -> Result<Question, Error> {
    let object = question.as_object().ok_or_else(|| mistyped(path))?;
    let kind = match object.get("type") {
        None => return Err(missing(&join(path, "type"))),
        Some(Value::String(kind)) => match kind.as_str() {
            "predicate" => QuestionKind::Predicate,
            "choice" => QuestionKind::Choice,
            "score" => QuestionKind::Score,
            _ => return Err(outside_values(&join(path, "type"))),
        },
        Some(_) => return Err(mistyped(&join(path, "type"))),
    };
    let known: &[&str] = match kind {
        QuestionKind::Predicate => &["type", "name", "instructions"],
        QuestionKind::Choice => &["type", "name", "instructions", "choices"],
        QuestionKind::Score => &["type", "name", "instructions", "levels"],
    };
    closed(object, known, path)?;
    let name = optional_text(object, "name", path)?;
    let instructions = match object.get("instructions") {
        None => return Err(missing(&join(path, "instructions"))),
        Some(Value::String(instructions)) => instructions.clone(),
        Some(Value::Array(_)) => return Err(unsent(&join(path, "instructions"))),
        Some(_) => return Err(mistyped(&join(path, "instructions"))),
    };
    Ok(match kind {
        QuestionKind::Predicate => Question::Predicate { name, instructions },
        QuestionKind::Choice => Question::Choice {
            name,
            instructions,
            choices: objects(object, "choices", path, |choice, at| {
                closed(choice, &["value", "description"], at)?;
                let value = match choice.get("value") {
                    None => return Err(missing(&join(at, "value"))),
                    Some(Value::String(text)) => ChoiceValue::Text(text.clone()),
                    Some(Value::Bool(flag)) => ChoiceValue::Flag(*flag),
                    Some(_) => return Err(mistyped(&join(at, "value"))),
                };
                Ok(ChoiceOption {
                    value,
                    description: optional_text(choice, "description", at)?,
                })
            })?,
        },
        QuestionKind::Score => Question::Score {
            name,
            instructions,
            levels: objects(object, "levels", path, |level, at| {
                closed(level, &["label", "description"], at)?;
                Ok(ScoreLevel {
                    label: required_text(level, "label", at)?,
                    description: optional_text(level, "description", at)?,
                })
            })?,
        },
    })
}

fn read_cache_options(root: &Map<String, Value>) -> Result<Option<PromptCacheOptions>, Error> {
    const PATH: &str = "prompt_cache_options";
    let options = match root.get(PATH) {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Object(options)) => options,
        Some(_) => return Err(mistyped(PATH)),
    };
    closed(options, &["ttl", "mode"], PATH)?;
    let ttl = match options.get("ttl") {
        None => None,
        Some(Value::String(ttl)) if ttl == "30m" => Some(PromptCacheTtl::ThirtyMinutes),
        Some(Value::String(_)) => return Err(outside_values(&join(PATH, "ttl"))),
        Some(_) => return Err(mistyped(&join(PATH, "ttl"))),
    };
    let mode = match options.get("mode") {
        None | Some(Value::Null) => None,
        Some(Value::String(mode)) => Some(match mode.as_str() {
            "implicit" => PromptCacheMode::Implicit,
            "explicit" => PromptCacheMode::Explicit,
            _ => return Err(outside_values(&join(PATH, "mode"))),
        }),
        Some(_) => return Err(mistyped(&join(PATH, "mode"))),
    };
    Ok(Some(PromptCacheOptions { ttl, mode }))
}
