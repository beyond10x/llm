#![forbid(unsafe_code)]

//! Bounded Messages text/tool projection. Authentication and billing come from the binding.

mod client;
mod codec;
mod decode;
mod usage;

pub use client::{ANTHROPIC_VERSION, BETA_HEADER, MessagesClient, OAUTH_BETA, VERSION_HEADER};
pub use codec::{IngressRequest, SUBSCRIPTION_CLIENT_PREAMBLE, decode_request, encode_request};
pub use decode::{StreamDecoder, decode_message, decode_stream};

use llm_core::Error;
use serde_json::{Map, Value};

fn object(value: &Value) -> Result<&Map<String, Value>, Error> {
    value
        .as_object()
        .ok_or_else(|| Error::protocol("Messages value must be an object"))
}
/// Every field this subset accepts on an arriving object and never reads.
///
/// Accepting a name is not the same as ignoring its value. Each of these is documented as a
/// string, and a value arriving in another shape is not a field to pass over: it is evidence that
/// the route is not the one this projection was written against, so it is refused. The check
/// travels with [`fields`] rather than sitting beside one accept list, because a new accept list
/// that picks one of these up would otherwise silently pick it up unchecked.
///
/// `citations` and `caller` are accepted names too, and are held to something stricter still:
/// any value at all is outside the declared subset, so `decode_block` refuses them outright.
const UNCONSUMED: [&str; 3] = ["inference_geo", "service_tier", "stop_sequence"];

/// Refuses a field `value` carries that `allowed` does not name, and an accepted-but-unread one
/// in a shape the route does not report.
///
/// `at` is where `value` sits, as a dotted path from the event or body root
/// (`message_start.message`, `messages[].content[]`; empty for the root itself). A refusal names
/// the field by that path and its own name, so a run against a live route says which field to
/// read the producer's documentation for; it never carries the field's value.
fn fields(value: &Value, at: &str, allowed: &[&str]) -> Result<(), Error> {
    let object = object(value)?;
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(Error::unsupported(format!(
            "Messages field is outside the declared subset: {}",
            path(at, key)
        )));
    }
    if object.iter().any(|(key, found)| {
        UNCONSUMED.contains(&key.as_str()) && !found.is_null() && !found.is_string()
    }) {
        return Err(Error::protocol(
            "Messages accepted field has a type this route does not report",
        ));
    }
    Ok(())
}
/// The longest field name a diagnostic copies.
const MAX_NAME_BYTES: usize = 64;

/// `key` under `at`. A field name is producer text, so only a bounded one of the route's own name
/// shape is copied; any other is written `?`.
fn path(at: &str, key: &str) -> String {
    let key = if (1..=MAX_NAME_BYTES).contains(&key.len())
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        key
    } else {
        "?"
    };
    if at.is_empty() {
        key.to_owned()
    } else {
        format!("{at}.{key}")
    }
}
/// An optional field's value, or `None` when it is missing **or** spelled as an explicit null.
///
/// Both halves of this codec read absence the same way: a producer that writes
/// `"stop_reason": null` and a caller that writes `"tool_choice": null` are both saying the
/// field does not apply, and one codec cannot mean two things by one spelling.
fn optional<'a>(value: &'a Value, field: &str) -> Option<&'a Value> {
    value.get(field).filter(|found| !found.is_null())
}
fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str, Error> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::protocol("Messages string field is missing or invalid"))
}
fn absent(value: &Value, field: &str) -> Result<(), Error> {
    if value.get(field).is_some_and(|value| !value.is_null()) {
        return Err(Error::unsupported(
            "Messages optional feature is outside the declared subset",
        ));
    }
    Ok(())
}
