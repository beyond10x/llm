//! Reading bytes as one whole JSON document.
//!
//! A request or an answer is taken only when it has one reading: [`document`] returns `None` for
//! bytes that are cut short, that hold anything but JSON whitespace after the document (a second
//! document included), that give a member twice at any depth, or that nest deeper than
//! [`MAX_DEPTH`]. Nothing is repaired and no reading is chosen.

use serde::de::{DeserializeSeed, Deserializer, Error as _, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::fmt;

/// The deepest a document nests: the document's own object is level 1, an array or an object
/// inside another one level more, a scalar none.
pub const MAX_DEPTH: usize = 16;

/// The one JSON document `bytes` hold, or `None`.
///
/// A number written without a fraction or an exponent is read exactly when `i64` or `u64` holds
/// it; every other number is the nearest double, and one no double holds is not read.
pub(crate) fn document(bytes: &[u8]) -> Option<Value> {
    let mut reader = serde_json::Deserializer::from_slice(bytes);
    let value = Strict(MAX_DEPTH).deserialize(&mut reader).ok()?;
    reader.end().ok()?;
    Some(value)
}

/// Reads any JSON value into a [`Value`], refusing an object that gives a member twice and an
/// array or object opened with no level left. It holds the levels that may still open. The bound
/// is below the JSON reader's own, so this one is the bound that applies.
#[derive(Clone, Copy)]
struct Strict(usize);

impl Strict {
    /// The reader for what an array or object holds, or the refusal of one opened past the bound.
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
        formatter.write_str("a JSON value that gives no member twice and nests within the bound")
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
                return Err(A::Error::custom("an object gives a member twice"));
            }
        }
        Ok(Value::Object(read))
    }
}
