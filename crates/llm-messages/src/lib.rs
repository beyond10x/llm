#![forbid(unsafe_code)]

//! Bounded Messages text/tool projection. Authentication and billing come from the binding.

mod codec;
mod usage;

pub use codec::{IngressRequest, decode_request, encode_request};

use llm_core::Error;
use serde_json::{Map, Value};

fn object(value: &Value) -> Result<&Map<String, Value>, Error> {
    value
        .as_object()
        .ok_or_else(|| Error::protocol("Messages value must be an object"))
}
fn fields(value: &Value, allowed: &[&str]) -> Result<(), Error> {
    if object(value)?
        .keys()
        .any(|key| !allowed.contains(&key.as_str()))
    {
        return Err(Error::unsupported(
            "Messages field is outside the declared subset",
        ));
    }
    Ok(())
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
