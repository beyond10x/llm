//! The decision answer: its nouns and its one reading from the body.
//!
//! Every member name and answer type is read from `openai/openai-openapi` at commit
//! `b4cad10d7cff7fc286f2dbf1992569989522c6f0`, schemas `DecisionResponse`,
//! `AnswerResourcePredicate`, `AnswerResourceChoice`, `AnswerResourceScore`,
//! `AnswerResourceRefusal` and `ResponseUsageResource`.

use crate::{
    request::{ChoiceValue, Question, QuestionKind},
    strict,
};
use llm_core::{Dispatch, Error, Usage};
use serde::Serialize;
use serde_json::{Map, Value};

/// The probability a choice answer gives one typed value, as the body wrote it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChoiceProbability {
    pub value: ChoiceValue,
    pub probability: f64,
}

/// The probability a score answer gives one level, as the body wrote it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScoreProbability {
    pub value: i64,
    pub label: String,
    pub probability: f64,
}

/// The kind of an answer, as `llm.decisions.AnswerKind` declares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerKind {
    Predicate,
    Choice,
    Score,
    Refusal,
}

/// One answer, of its question's kind or a refusal. `name` is the question's name. A number is
/// the double nearest to the one the body wrote; none is held to a range here.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DecisionAnswer {
    Predicate {
        name: Option<String>,
        probability: f64,
    },
    Choice {
        name: Option<String>,
        choice: ChoiceValue,
        probabilities: Vec<ChoiceProbability>,
        confidence: f64,
    },
    Score {
        name: Option<String>,
        score: f64,
        probabilities: Vec<ScoreProbability>,
        confidence: f64,
    },
    /// The model declined to answer this question; other questions keep their answers.
    Refusal { name: Option<String> },
}

impl DecisionAnswer {
    pub const fn kind(&self) -> AnswerKind {
        match self {
            Self::Predicate { .. } => AnswerKind::Predicate,
            Self::Choice { .. } => AnswerKind::Choice,
            Self::Score { .. } => AnswerKind::Score,
            Self::Refusal { .. } => AnswerKind::Refusal,
        }
    }
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::Predicate { name, .. }
            | Self::Choice { name, .. }
            | Self::Score { name, .. }
            | Self::Refusal { name } => name.as_deref(),
        }
    }
}

/// What the endpoint answered one request: one answer per question, in question order.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionResponse {
    /// The model the body named.
    pub model: String,
    pub answers: Vec<DecisionAnswer>,
    /// Each counter as the body reported it; one it did not report is `None`, never 0.
    pub usage: Usage,
}

/// What one body gave: the answers or the refusal, and whatever model and usage it reported
/// either way. `whole` is whether the body was one whole JSON document.
pub(crate) struct Read {
    pub result: Result<Vec<DecisionAnswer>, Error>,
    pub model: Option<String>,
    pub usage: Usage,
    pub whole: bool,
}

/// An answer the client cannot use: `protocol`, dispatch `accepted`, final.
fn refused(message: impl Into<String>) -> Error {
    Error::protocol(message).with_dispatch(Dispatch::Accepted)
}

fn mistyped(path: &str) -> Error {
    refused(format!(
        "decision answer lacks a member or holds one of the wrong type: {path}"
    ))
}

/// Reads one body and holds it to the questions asked: exactly one answer per question, in
/// order, each of its question's type or a refusal, each carrying its question's name.
pub(crate) fn read(body: &[u8], questions: &[Question]) -> Read {
    let Some(document) = strict::document(body) else {
        return Read {
            result: Err(refused(format!(
                "decision answer is not one whole JSON document with every member once, nested at most {} deep",
                strict::MAX_DEPTH
            ))),
            model: None,
            usage: Usage::default(),
            whole: false,
        };
    };
    let Some(root) = document.as_object() else {
        return Read {
            result: Err(mistyped("(document)")),
            model: None,
            usage: Usage::default(),
            whole: true,
        };
    };
    // The usage and the model are read apart from the answers, so a body refused for an answer
    // still reports the tokens it named.
    let model = root.get("model").and_then(Value::as_str).map(str::to_owned);
    let result = if model.is_some() {
        answers(root, questions)
    } else {
        Err(mistyped("model"))
    };
    Read {
        result,
        model,
        usage: usage(root),
        whole: true,
    }
}

/// The counters the body reports. One is taken only when the body writes it as a non-negative
/// integer, without a fraction or an exponent, that 64 bits hold; absent, `null`, negative,
/// fractional or of another type it is unknown. Counters that contradict each other are all
/// unknown: none of them can be trusted over the others.
fn usage(root: &Map<String, Value>) -> Usage {
    let reported = root.get("usage");
    let counter = |path: &[&str]| {
        path.iter()
            .try_fold(reported?, |value, member| value.get(member))?
            .as_u64()
    };
    let usage = Usage {
        input_tokens: counter(&["input_tokens"]),
        output_tokens: counter(&["output_tokens"]),
        cached_input_tokens: counter(&["input_tokens_details", "cached_tokens"]),
        cache_creation_input_tokens: counter(&["input_tokens_details", "cache_write_tokens"]),
        reasoning_output_tokens: counter(&["output_tokens_details", "reasoning_tokens"]),
    };
    if usage.validate().is_ok() {
        usage
    } else {
        Usage::default()
    }
}

fn answers(
    root: &Map<String, Value>,
    questions: &[Question],
) -> Result<Vec<DecisionAnswer>, Error> {
    let given = root
        .get("answers")
        .and_then(Value::as_array)
        .ok_or_else(|| mistyped("answers"))?;
    if given.len() != questions.len() {
        return Err(refused(
            "decision answer does not hold one answer per question: answers",
        ));
    }
    given
        .iter()
        .zip(questions)
        .enumerate()
        .map(|(position, (answer, question))| read_answer(answer, question, position))
        .collect()
}

fn number(object: &Map<String, Value>, member: &str, path: &str) -> Result<f64, Error> {
    object
        .get(member)
        .and_then(Value::as_f64)
        .ok_or_else(|| mistyped(&format!("{path}.{member}")))
}

fn choice_value(
    object: &Map<String, Value>,
    member: &str,
    path: &str,
) -> Result<ChoiceValue, Error> {
    match object.get(member) {
        Some(Value::String(text)) => Ok(ChoiceValue::Text(text.clone())),
        Some(Value::Bool(flag)) => Ok(ChoiceValue::Flag(*flag)),
        _ => Err(mistyped(&format!("{path}.{member}"))),
    }
}

/// The members of the array `probabilities` of the answer at `path`, each an object.
fn probabilities<T>(
    object: &Map<String, Value>,
    path: &str,
    read: impl Fn(&Map<String, Value>, &str) -> Result<T, Error>,
) -> Result<Vec<T>, Error> {
    let path = format!("{path}.probabilities");
    object
        .get("probabilities")
        .and_then(Value::as_array)
        .ok_or_else(|| mistyped(&path))?
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let at = format!("{path}[{index}]");
            read(item.as_object().ok_or_else(|| mistyped(&at))?, &at)
        })
        .collect()
}

fn read_answer(
    answer: &Value,
    question: &Question,
    position: usize,
) -> Result<DecisionAnswer, Error> {
    let path = format!("answers[{position}]");
    let object = answer.as_object().ok_or_else(|| mistyped(&path))?;
    let kind = match object.get("type").and_then(Value::as_str) {
        None => return Err(mistyped(&format!("{path}.type"))),
        Some("predicate") => AnswerKind::Predicate,
        Some("choice") => AnswerKind::Choice,
        Some("score") => AnswerKind::Score,
        Some("refusal") => AnswerKind::Refusal,
        Some(_) => {
            return Err(refused(format!(
                "decision answer has a type outside predicate, choice, score and refusal: {path}.type"
            )));
        }
    };
    let asked = match question.kind() {
        QuestionKind::Predicate => AnswerKind::Predicate,
        QuestionKind::Choice => AnswerKind::Choice,
        QuestionKind::Score => AnswerKind::Score,
    };
    if kind != AnswerKind::Refusal && kind != asked {
        return Err(refused(format!(
            "decision answer is of another type than its question: {path}.type"
        )));
    }
    // The reference requires the member and writes `null` for an unnamed question.
    let name = match object.get("name") {
        Some(Value::Null) => None,
        Some(Value::String(name)) => Some(name.clone()),
        _ => return Err(mistyped(&format!("{path}.name"))),
    };
    if name.as_deref() != question.name() {
        return Err(refused(format!(
            "decision answer does not carry its question's name: {path}.name"
        )));
    }
    Ok(match kind {
        AnswerKind::Refusal => DecisionAnswer::Refusal { name },
        AnswerKind::Predicate => DecisionAnswer::Predicate {
            name,
            probability: number(object, "probability", &path)?,
        },
        AnswerKind::Choice => DecisionAnswer::Choice {
            name,
            choice: choice_value(object, "choice", &path)?,
            probabilities: probabilities(object, &path, |item, at| {
                Ok(ChoiceProbability {
                    value: choice_value(item, "value", at)?,
                    probability: number(item, "probability", at)?,
                })
            })?,
            confidence: number(object, "confidence", &path)?,
        },
        AnswerKind::Score => DecisionAnswer::Score {
            name,
            score: number(object, "score", &path)?,
            probabilities: probabilities(object, &path, |item, at| {
                Ok(ScoreProbability {
                    value: item
                        .get("value")
                        .and_then(Value::as_i64)
                        .ok_or_else(|| mistyped(&format!("{at}.value")))?,
                    label: item
                        .get("label")
                        .and_then(Value::as_str)
                        .ok_or_else(|| mistyped(&format!("{at}.label")))?
                        .to_owned(),
                    probability: number(item, "probability", at)?,
                })
            })?,
            confidence: number(object, "confidence", &path)?,
        },
    })
}
