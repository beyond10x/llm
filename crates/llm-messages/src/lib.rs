#![forbid(unsafe_code)]

//! Bounded Messages text/tool projection. Authentication and billing come from the binding.
//!
//! # A Tokio runtime is required
//!
//! [`MessagesClient`] runs its turn on Tokio and does not start a runtime of its own. A turn
//! polled on a thread that is in no Tokio runtime is refused, `unsupported` and not retriable,
//! and does not panic: on its first poll before it encodes, resolves a credential or sends
//! anything, dispatch `not-sent` (`llm_http::require_tokio_runtime`); moved out of its runtime
//! part-way, at its next wait (the credential, the answer, a read of the stream, an event handed
//! to the sink under the turn deadline) with the dispatch it had reached
//! (`llm_http::within_tokio_runtime`). A caller without a runtime runs the turn inside one it
//! starts, or through `llm-blocking`, whose `BlockingModel` owns one. [`decode_stream`],
//! which has no deadline, needs no runtime.

mod client;
mod codec;
mod decode;
mod ingress;
mod usage;

pub use client::{
    ANTHROPIC_VERSION, BETA_HEADER, MessagesClient, OAUTH_BETA, OPENCODE_SESSION_HEADER,
    VERSION_HEADER, is_opencode_go,
};
pub use codec::{
    IngressRequest, SUBSCRIPTION_CLIENT_PREAMBLE, decode_request, decode_request_with_headers,
    encode_body, encode_request, omitted_advisories,
};
pub use decode::{StreamDecoder, USAGE_EXTENSION, decode_message, decode_stream};
pub use ingress::{IngressStream, encode_ingress_message};

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

/// Whether `found` is a value this subset may pass over for the accepted, never-read field `key`.
///
/// Each name here is one the route documents as empty unless the request asked for a feature this
/// projection never asks for: a container tool, prompt-cache diagnostics, fast inference, a
/// toolset. Empty is `null`, and for `speed` its `standard` mode. Any other value is evidence of a
/// request this projection did not send, and is refused like a wrong type of an [`UNCONSUMED`]
/// field. `container` alone is accepted when filled too: the route names in it the container a
/// container tool ran in, which no later request of this projection has a use for
/// (`docs/messages.md`, "Usage"). `stop_details` is the one name the route fills without being
/// asked, on a `refusal` stop; it is accepted only as `null`, so a refusal's details are refused
/// rather than dropped. Like [`UNCONSUMED`], the check travels with [`fields`], so every accept
/// list that names one of these holds it to the same shape.
fn unrequested(key: &str, found: &Value) -> bool {
    match key {
        "container" => found.is_null() || container(found),
        "diagnostics" | "stop_details" | "toolset_name" => found.is_null(),
        "speed" => found.is_null() || found.as_str() == Some("standard"),
        _ => true,
    }
}

/// The documented container object: `id` and `expires_at`, and `skills` only when empty.
fn container(found: &Value) -> bool {
    found.as_object().is_some_and(|container| {
        container.get("id").is_some_and(Value::is_string)
            && container.get("expires_at").is_some_and(Value::is_string)
            && container.iter().all(|(key, value)| match key.as_str() {
                "id" | "expires_at" => true,
                "skills" => value.is_null(),
                _ => false,
            })
    })
}

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
    if object.iter().any(|(key, found)| !unrequested(key, found)) {
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
