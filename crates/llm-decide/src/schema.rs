//! The JSON Schema keywords a verdict schema may use, and the check of an answer against them.
//!
//! A schema is accepted only when every keyword in it, at every depth, is one [`holds`] checks
//! and has a value of the shape [`holds`] reads. So no answer is held to less than its schema
//! says: a keyword this module does not check refuses the rubric instead of being skipped.

use serde_json::{Number, Value};
use std::cmp::Ordering;

/// The deepest a schema, or a value inside its `const` or `enum`, nests.
const MAX_DEPTH: usize = 64;

const TYPES: [&str; 7] = [
    "object", "array", "string", "integer", "number", "boolean", "null",
];

/// Whether `schema` is a JSON object using only the checked keywords: `type`, `properties`,
/// `required`, `additionalProperties`, `items`, `enum`, `const`, `minimum`, `maximum`, `anyOf`,
/// and the annotations `title` and `description`.
pub(crate) fn supported(schema: &Value) -> bool {
    nested(schema, 0)
}

fn nested(schema: &Value, depth: usize) -> bool {
    let Some(schema) = schema.as_object() else {
        return false;
    };
    if depth > MAX_DEPTH {
        return false;
    }
    let sub = |schema: &Value| nested(schema, depth + 1);
    schema.iter().all(|(keyword, rule)| match keyword.as_str() {
        "type" => match rule {
            Value::String(name) => TYPES.contains(&name.as_str()),
            Value::Array(names) => {
                !names.is_empty()
                    && names
                        .iter()
                        .all(|name| name.as_str().is_some_and(|name| TYPES.contains(&name)))
            }
            _ => false,
        },
        "properties" => rule
            .as_object()
            .is_some_and(|properties| properties.values().all(sub)),
        "required" => rule
            .as_array()
            .is_some_and(|names| names.iter().all(Value::is_string)),
        "additionalProperties" => rule.is_boolean() || sub(rule),
        "items" => sub(rule),
        "anyOf" => rule
            .as_array()
            .is_some_and(|options| !options.is_empty() && options.iter().all(sub)),
        "enum" => rule.is_array() && shallow(rule, depth),
        "const" => shallow(rule, depth),
        "minimum" | "maximum" => rule.is_number(),
        "title" | "description" => rule.is_string(),
        _ => false,
    })
}

/// Whether a literal value nests no deeper than the bound.
fn shallow(value: &Value, depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    match value {
        Value::Array(items) => items.iter().all(|item| shallow(item, depth + 1)),
        Value::Object(members) => members.values().all(|member| shallow(member, depth + 1)),
        _ => true,
    }
}

/// Whether `answer` is valid against `schema`. Every keyword is read as JSON Schema reads it: a
/// keyword for another type of value says nothing about this one. A schema [`supported`] did
/// not accept holds no answer.
pub(crate) fn holds(schema: &Value, answer: &Value) -> bool {
    let Some(schema) = schema.as_object() else {
        return false;
    };
    schema.iter().all(|(keyword, rule)| match keyword.as_str() {
        "type" => match rule {
            Value::String(name) => is(name, answer),
            Value::Array(names) => names
                .iter()
                .filter_map(Value::as_str)
                .any(|name| is(name, answer)),
            _ => false,
        },
        "properties" => match (rule, answer) {
            (Value::Object(properties), Value::Object(given)) => properties
                .iter()
                .all(|(name, property)| given.get(name).is_none_or(|value| holds(property, value))),
            _ => true,
        },
        "required" => match (rule, answer) {
            (Value::Array(names), Value::Object(given)) => names
                .iter()
                .filter_map(Value::as_str)
                .all(|name| given.contains_key(name)),
            _ => true,
        },
        "additionalProperties" => answer.as_object().is_none_or(|given| {
            let declared = schema.get("properties").and_then(Value::as_object);
            given
                .iter()
                .filter(|(name, _)| declared.is_none_or(|known| !known.contains_key(*name)))
                .all(|(_, value)| match rule {
                    Value::Bool(allowed) => *allowed,
                    rule => holds(rule, value),
                })
        }),
        "items" => answer
            .as_array()
            .is_none_or(|items| items.iter().all(|item| holds(rule, item))),
        "anyOf" => rule
            .as_array()
            .is_some_and(|options| options.iter().any(|option| holds(option, answer))),
        "enum" => rule
            .as_array()
            .is_some_and(|allowed| allowed.iter().any(|literal| same(literal, answer))),
        "const" => same(rule, answer),
        "minimum" => bounded(rule, answer, Ordering::is_ge),
        "maximum" => bounded(rule, answer, Ordering::is_le),
        "title" | "description" => true,
        _ => false,
    })
}

/// Whether `answer` has the JSON Schema type `name`. An integer is a number without a
/// fractional part, however it is written.
fn is(name: &str, answer: &Value) -> bool {
    match name {
        "object" => answer.is_object(),
        "array" => answer.is_array(),
        "string" => answer.is_string(),
        "boolean" => answer.is_boolean(),
        "null" => answer.is_null(),
        "number" => answer.is_number(),
        "integer" => {
            answer.is_i64()
                || answer.is_u64()
                || answer
                    .as_f64()
                    .is_some_and(|value| value.trunc().to_bits() == value.to_bits())
        }
        _ => false,
    }
}

/// A numeric bound: whether the answer, compared with the limit by exact value, is `within` it.
/// A bound says nothing about an answer that is not a number.
fn bounded(limit: &Value, answer: &Value, within: fn(Ordering) -> bool) -> bool {
    match (limit, answer) {
        (Value::Number(limit), Value::Number(given)) => within(compare(given, limit)),
        _ => true,
    }
}

/// Whether two values are equal as JSON Schema compares the literals of `enum` and `const`: two
/// numbers by their value however they are written (`1` and `1.0` are one number), at every
/// depth; arrays item by item in order; objects by their keys whatever the order.
fn same(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => compare(left, right) == Ordering::Equal,
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len() && left.iter().zip(right).all(|(one, other)| same(one, other))
        }
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .all(|(name, member)| right.get(name).is_some_and(|other| same(member, other)))
        }
        _ => left == right,
    }
}

/// A JSON number as its exact value: `mantissa * 2^exponent`, negated when `negative`. Every
/// `u64`, `i64` and finite `f64` has one, so no comparison goes through a rounding conversion.
struct Exact {
    negative: bool,
    mantissa: u64,
    exponent: i32,
}

impl Exact {
    fn of(number: &Number) -> Self {
        if let Some(value) = number.as_u64() {
            return Self {
                negative: false,
                mantissa: value,
                exponent: 0,
            };
        }
        if let Some(value) = number.as_i64() {
            return Self {
                negative: value < 0,
                mantissa: value.unsigned_abs(),
                exponent: 0,
            };
        }
        // An IEEE 754 double: one sign bit, eleven exponent bits, 52 fraction bits. JSON holds
        // no infinity and no NaN.
        let bits = number.as_f64().unwrap_or_default().to_bits();
        let fraction = bits & ((1 << 52) - 1);
        let biased = i32::try_from((bits >> 52) & 0x7ff).unwrap_or_default();
        let (mantissa, exponent) = if biased == 0 {
            (fraction, -1074)
        } else {
            (fraction | (1 << 52), biased - 1075)
        };
        Self {
            negative: bits >> 63 == 1,
            mantissa,
            exponent,
        }
    }

    /// -1, 0 or 1. Zero has no sign: `-0.0` is `0`.
    fn sign(&self) -> i8 {
        match (self.mantissa, self.negative) {
            (0, _) => 0,
            (_, true) => -1,
            (_, false) => 1,
        }
    }

    /// Compares two magnitudes, neither of them zero: first by the place of the highest set
    /// bit, then by the mantissas shifted to one place. Both steps are exact: the shift is to
    /// the left inside 128 bits and drops nothing.
    fn magnitude(&self, other: &Self) -> Ordering {
        let top =
            |number: &Self| i64::from(number.exponent) - i64::from(number.mantissa.leading_zeros());
        let aligned =
            |number: &Self| u128::from(number.mantissa) << (number.mantissa.leading_zeros() + 64);
        top(self)
            .cmp(&top(other))
            .then_with(|| aligned(self).cmp(&aligned(other)))
    }
}

/// Orders two JSON numbers by their exact values, whichever of `u64`, `i64` and `f64` holds
/// each: 2^53 + 1 is over 2^53, and the greatest `u64` is under 2^64.
fn compare(left: &Number, right: &Number) -> Ordering {
    let (left, right) = (Exact::of(left), Exact::of(right));
    match (left.sign(), right.sign()) {
        (0, 0) => Ordering::Equal,
        (1, 1) => left.magnitude(&right),
        (-1, -1) => right.magnitude(&left),
        (one, other) => one.cmp(&other),
    }
}
