//! The JSON Schema keywords a verdict schema may use, and the check of an answer against them.
//!
//! A schema is accepted only when every keyword in it, at every depth, is one [`holds`] checks
//! and has a value of the shape [`holds`] reads. So no answer is held to less than its schema
//! says: a keyword this module does not check refuses the rubric instead of being skipped.

use serde_json::Value;

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
            .is_some_and(|allowed| allowed.contains(answer)),
        "const" => rule == answer,
        "minimum" => bounded(rule, answer, |limit, given| given >= limit),
        "maximum" => bounded(rule, answer, |limit, given| given <= limit),
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

/// A numeric bound, compared as `f64`; it says nothing about an answer that is not a number.
fn bounded(limit: &Value, answer: &Value, within: fn(f64, f64) -> bool) -> bool {
    match (limit.as_f64(), answer.as_f64()) {
        (Some(limit), Some(given)) => within(limit, given),
        _ => true,
    }
}
