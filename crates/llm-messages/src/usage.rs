use crate::{fields, object, path};
use llm_core::{Error, Usage};
use serde_json::Value;

/// A consistent cumulative wire snapshot. Updates are transactional: a bad delta cannot replace it.
#[derive(Clone, Default)]
pub(crate) struct Snapshot {
    input: Option<u64>,
    read: Option<u64>,
    created: Option<u64>,
    output: Option<u64>,
    reasoning: Option<u64>,
}

impl Snapshot {
    pub fn update(&self, value: &Value, at: &str) -> Result<Self, Error> {
        fields(
            value,
            at,
            &[
                "input_tokens",
                "output_tokens",
                "cache_read_input_tokens",
                "cache_creation_input_tokens",
                "cache_creation",
                "output_tokens_details",
                "server_tool_use",
                "service_tier",
                "inference_geo",
            ],
        )?;
        let mut next = self.clone();
        update(&mut next.input, value, "input_tokens")?;
        update(&mut next.output, value, "output_tokens")?;
        update(&mut next.read, value, "cache_read_input_tokens")?;
        update(&mut next.created, value, "cache_creation_input_tokens")?;
        if let Some(details) = value.get("output_tokens_details").filter(|v| !v.is_null()) {
            fields(
                details,
                &path(at, "output_tokens_details"),
                &["thinking_tokens"],
            )?;
            update(&mut next.reasoning, details, "thinking_tokens")?;
        }
        if let Some(cache) = value.get("cache_creation").filter(|v| !v.is_null()) {
            fields(
                cache,
                &path(at, "cache_creation"),
                &["ephemeral_1h_input_tokens", "ephemeral_5m_input_tokens"],
            )?;
            let hour = count(cache, "ephemeral_1h_input_tokens")?;
            let short = count(cache, "ephemeral_5m_input_tokens")?;
            if hour.is_some_and(|n| n != 0) {
                return Err(Error::unsupported(
                    "Messages one-hour cache pricing is outside the declared subset",
                ));
            }
            if let (Some(0), Some(short), Some(total)) = (hour, short, next.created)
                && short != total
            {
                return Err(Error::protocol(
                    "Messages cache breakdown contradicts its total",
                ));
            }
        }
        if let Some(tools) = value.get("server_tool_use").filter(|v| !v.is_null()) {
            fields(
                tools,
                &path(at, "server_tool_use"),
                &["web_search_requests", "web_fetch_requests"],
            )?;
            for key in object(tools)?.keys() {
                if count(tools, key)?.is_none_or(|n| n != 0) {
                    return Err(Error::unsupported(
                        "Messages server-tool charges are outside the declared subset",
                    ));
                }
            }
        }
        // `service_tier` and `inference_geo` are accepted here and read nowhere; the shape they
        // have to arrive in is enforced by `fields` above, with the rest of that class.
        next.normalized()?.validate()?;
        Ok(next)
    }

    /// Whether the route reported any counter at all. Nothing reported stays unknown, not zero.
    pub const fn is_known(&self) -> bool {
        self.input.is_some()
            || self.read.is_some()
            || self.created.is_some()
            || self.output.is_some()
            || self.reasoning.is_some()
    }

    pub fn normalized(&self) -> Result<Usage, Error> {
        let total = match (self.input, self.read, self.created) {
            (Some(input), Some(read), Some(created)) => Some(
                input
                    .checked_add(read)
                    .and_then(|n| n.checked_add(created))
                    .ok_or_else(|| Error::protocol("Messages input usage overflows"))?,
            ),
            _ => None,
        };
        Ok(Usage {
            input_tokens: total,
            output_tokens: self.output,
            cached_input_tokens: self.read,
            cache_creation_input_tokens: self.created,
            reasoning_output_tokens: self.reasoning,
        })
    }
}
fn count(value: &Value, name: &str) -> Result<Option<u64>, Error> {
    match value.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(n) => n
            .as_u64()
            .map(Some)
            .ok_or_else(|| Error::protocol("Messages usage counter is not an unsigned integer")),
    }
}
fn update(target: &mut Option<u64>, value: &Value, name: &str) -> Result<(), Error> {
    if let Some(next) = count(value, name)? {
        if target.is_some_and(|previous| next < previous) {
            return Err(Error::protocol("Messages cumulative usage regressed"));
        }
        *target = Some(next);
    }
    Ok(())
}
