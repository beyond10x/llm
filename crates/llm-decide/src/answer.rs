//! Reading a model's answer as the one JSON value it ended on.
//!
//! An answer is taken only when it is whole and has one reading. [`whole`] returns `None` for
//! every answer that is cut off, split, mixed with something else, or that a JSON reader would
//! have to choose a reading of.

use llm_core::{Item, StopReason, TurnOutcome};
use serde::de::{DeserializeSeed, Deserializer, Error as _, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::fmt;

/// The outcome's answer as JSON, exactly as it arrived, or `None` when it is not one whole,
/// unambiguous JSON value:
///
/// - the turn did not end normally (the output limit, an incomplete answer);
/// - the outcome has no assistant text or more than one, or anything else beside it, such as a
///   refusal (state the provider keeps, [`Item::Opaque`], is not part of the answer; routing
///   has already failed a call whose outcome holds a tool call the request did not publish);
/// - the text is not one JSON value with nothing but JSON whitespace around it;
/// - an object gives a key twice, at any depth;
/// - a number written without a fraction or an exponent is outside what `i64` and `u64` hold,
///   so it would be rounded to a double.
///
/// Every other number is read as JSON readers read it: an integer exactly, a number with a
/// fraction or an exponent as the nearest double; one no double holds is not read.
pub(crate) fn whole(outcome: &TurnOutcome) -> Option<Value> {
    if outcome.stop_reason != StopReason::EndTurn {
        return None;
    }
    let mut answer = None;
    for item in &outcome.items {
        match item {
            Item::AssistantText { text } if answer.is_none() => answer = Some(text.as_str()),
            Item::Opaque { .. } => {}
            _ => return None,
        }
    }
    let text = answer?;
    let mut reader = serde_json::Deserializer::from_str(text);
    let value = Strict.deserialize(&mut reader).ok()?;
    reader.end().ok()?;
    integers_are_exact(text).then_some(value)
}

/// Reads any JSON value into a [`Value`], refusing an object that gives a key twice.
struct Strict;

impl<'de> DeserializeSeed<'de> for Strict {
    type Value = Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Strict {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value that gives no key twice")
    }
    fn visit_bool<E>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }
    fn visit_i64<E>(self, value: i64) -> Result<Value, E> {
        Ok(Value::from(value))
    }
    fn visit_u64<E>(self, value: u64) -> Result<Value, E> {
        Ok(Value::from(value))
    }
    fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Value, E> {
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("a number no double holds"))
    }
    fn visit_str<E>(self, value: &str) -> Result<Value, E> {
        Ok(Value::from(value))
    }
    fn visit_string<E>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }
    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<Value, A::Error> {
        let mut read = Vec::new();
        while let Some(item) = items.next_element_seed(Self)? {
            read.push(item);
        }
        Ok(Value::Array(read))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut members: A) -> Result<Value, A::Error> {
        let mut read = Map::new();
        while let Some(name) = members.next_key::<String>()? {
            let member = members.next_value_seed(Self)?;
            if read.insert(name, member).is_some() {
                return Err(A::Error::custom("an object gives a key twice"));
            }
        }
        Ok(Value::Object(read))
    }
}

/// Whether every number of `text` written without a fraction or an exponent is one `i64` or
/// `u64` holds. `text` is JSON that was read, so outside a string a digit or a minus sign starts
/// a number.
fn integers_are_exact(text: &str) -> bool {
    let starts_a_token = |each: char| each == '"' || each == '-' || each.is_ascii_digit();
    let mut rest = text;
    loop {
        rest = rest.trim_start_matches(|each: char| !starts_a_token(each));
        if rest.is_empty() {
            return true;
        }
        if let Some(inside) = rest.strip_prefix('"') {
            let mut string = inside.chars();
            loop {
                match string.next() {
                    Some('\\') => {
                        string.next();
                    }
                    Some('"') | None => break,
                    Some(_) => {}
                }
            }
            rest = string.as_str();
        } else {
            let after = rest.trim_start_matches(|each: char| {
                matches!(each, '-' | '+' | '.' | 'e' | 'E') || each.is_ascii_digit()
            });
            let literal = rest.strip_suffix(after).unwrap_or(rest);
            let is_integer = !literal.contains(['.', 'e', 'E']);
            if is_integer && literal.parse::<i64>().is_err() && literal.parse::<u64>().is_err() {
                return false;
            }
            rest = after;
        }
    }
}
