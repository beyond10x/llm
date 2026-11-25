//! Reading a model's answer as the one JSON value it ended on: the judge's verdict and the
//! classifier's label on its two model paths.
//!
//! An answer is taken only when it is whole and has one reading. [`whole`] reads an answer given
//! as text and returns `None` for every answer that is cut off, split, mixed with something
//! else, or that a JSON reader would have to choose a reading of. [`call`] reads an answer given
//! as the arguments of a tool call and returns `None` for a call of a turn that did not stop for
//! it, for more than one call and for a call beside a refusal.

use llm_core::{Item, StopReason, TurnOutcome};
use serde::de::{DeserializeSeed, Deserializer, Error as _, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::fmt;

/// The outcome's answer as JSON, as it arrived and with nothing repaired, or `None` when it is
/// not one whole, unambiguous JSON value:
///
/// - the turn did not end normally (the output limit, an incomplete answer);
/// - the outcome has no assistant text or more than one, or anything else beside it, such as a
///   refusal (state the provider keeps, [`Item::Opaque`], is not part of the answer; routing
///   has already failed a call whose outcome holds a tool call the request did not publish). An
///   empty or whitespace-only text item counts as one, so an answer beside it is not taken;
/// - the text is not one JSON value with nothing but JSON whitespace around it, or nests
///   deeper than [`MAX_DEPTH`];
/// - an object gives a key twice, at any depth;
/// - a number written without a fraction or an exponent is outside what `i64` and `u64` hold,
///   so it would be rounded to a double.
///
/// Every other number is read as JSON readers read it: an integer exactly, except `-0`, which
/// is the double minus zero; a number with a fraction or an exponent as the nearest double; one
/// no double holds is not read.
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
    reader.disable_recursion_limit();
    let value = Strict(MAX_DEPTH).deserialize(&mut reader).ok()?;
    reader.end().ok()?;
    integers_are_exact(text).then_some(value)
}

/// The arguments of the outcome's one call to the tool `name`, as the protocol decoder parsed
/// them, or `None` when the outcome does not carry exactly that:
///
/// - the turn did not stop for its tool call (the output limit, an incomplete answer), so the
///   call may not be all the model meant to say;
/// - the outcome has no tool call, more than one, or one to another tool;
/// - a refusal stands beside the call.
///
/// Assistant text and state the provider keeps ([`Item::Opaque`]) beside the call are not part of
/// the answer. A key the model gave twice in the arguments is not seen here: they arrive parsed.
pub(crate) fn call<'a>(outcome: &'a TurnOutcome, name: &str) -> Option<&'a Value> {
    if outcome.stop_reason != StopReason::ToolCalls {
        return None;
    }
    let mut arguments = None;
    for item in &outcome.items {
        match item {
            Item::ToolCall(call) if arguments.is_none() && call.name.as_str() == name => {
                arguments = Some(&call.arguments);
            }
            Item::ToolCall(_) | Item::AssistantRefusal { .. } => return None,
            _ => {}
        }
    }
    arguments
}

/// The deepest an answer nests: an array or an object inside another is one level, a scalar is
/// none. The bound is this reader's own; the JSON reader's is switched off, so the two cannot
/// differ.
const MAX_DEPTH: usize = 128;

/// Reads any JSON value into a [`Value`], refusing an object that gives a key twice and an array
/// or object opened with no level left. It holds the levels that may still open.
#[derive(Clone, Copy)]
struct Strict(usize);

impl Strict {
    /// The reader for what an array or object holds, or the refusal of one opened past the bound.
    /// Refusing here, before anything inside is read, is what bounds the recursion.
    fn inside<E: serde::de::Error>(self) -> Result<Self, E> {
        self.0
            .checked_sub(1)
            .map(Self)
            .ok_or_else(|| E::custom("nested deeper than the bound"))
    }
}

impl<'de> DeserializeSeed<'de> for Strict {
    type Value = Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Strict {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .write_str("a JSON value that gives no key twice and nests no deeper than the bound")
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
        let inside = self.inside()?;
        let mut read = Vec::new();
        while let Some(item) = items.next_element_seed(inside)? {
            read.push(item);
        }
        Ok(Value::Array(read))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut members: A) -> Result<Value, A::Error> {
        let inside = self.inside()?;
        let mut read = Map::new();
        while let Some(name) = members.next_key::<String>()? {
            let member = members.next_value_seed(inside)?;
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
            // The literal is what the trim took off: its length is a subtraction, so each
            // number costs its own length and the scan the length of the text.
            let (literal, _) = rest.split_at(rest.len() - after.len());
            let is_integer = !literal.contains(['.', 'e', 'E']);
            if is_integer && literal.parse::<i64>().is_err() && literal.parse::<u64>().is_err() {
                return false;
            }
            rest = after;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::call;
    use llm_core::{
        CallId, Id, Item, StopReason, ToolCall, ToolName, TurnObservation, TurnOutcome,
    };
    use llm_routing::Catalog;
    use serde_json::{Value, json};

    fn arguments() -> Value {
        json!({"label": "positive"})
    }

    /// A turn that stopped for its one call, `call-1`, to the tool `name`.
    fn outcome_calling(name: &str) -> TurnOutcome {
        let catalog = Catalog::parse(include_str!("../../../examples/catalog.toml")).unwrap();
        let serving = Id::new("remote-large").unwrap();
        let provenance = catalog.binding(&serving).unwrap().provenance().clone();
        TurnOutcome {
            stop_reason: StopReason::ToolCalls,
            items: vec![Item::ToolCall(ToolCall {
                call_id: CallId::new("call-1").unwrap(),
                name: ToolName::new(name).unwrap(),
                arguments: arguments(),
            })],
            observation: TurnObservation::new(provenance),
        }
    }

    /// The routed call fails a turn that calls a tool other than the one it forced, so no
    /// classifier is handed such an outcome; only a direct reading shows that [`call`] holds the
    /// name itself.
    #[test]
    fn a_call_to_another_tool_is_not_the_answer() {
        assert_eq!(
            call(&outcome_calling("classify"), "classify"),
            Some(&arguments())
        );
        assert_eq!(call(&outcome_calling("label"), "classify"), None);
    }

    /// The routed call fails a turn that holds a call and says it ended normally, so again only
    /// a direct reading shows that [`call`] holds the stop reason itself.
    #[test]
    fn a_call_of_a_turn_that_says_it_ended_normally_is_not_the_answer() {
        let ended = TurnOutcome {
            stop_reason: StopReason::EndTurn,
            ..outcome_calling("classify")
        };
        assert_eq!(call(&ended, "classify"), None);
    }
}
