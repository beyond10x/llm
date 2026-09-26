// Adapted from beyond10x/harness 709a2eb: harness-wire/src/id.rs.
use serde::{Deserialize, Serialize};

const MAX_ID_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{kind} must be 1..={MAX_ID_BYTES} printable ASCII bytes")]
pub struct InvalidId {
    kind: &'static str,
}

impl InvalidId {
    pub fn kind(&self) -> &'static str {
        self.kind
    }
}

fn validate(kind: &'static str, value: &str) -> Result<(), InvalidId> {
    if value.is_empty()
        || value.len() > MAX_ID_BYTES
        || !value.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(InvalidId { kind });
    }
    Ok(())
}

// Protocol-specific restrictions belong to the corresponding adapter.

macro_rules! id_type {
    ($name:ident, $kind:literal, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Creates a validated identifier.
            ///
            /// # Errors
            ///
            /// Returns [`InvalidId`] when the value is empty, exceeds the size limit, or holds
            /// anything other than printable ASCII bytes.
            pub fn new(value: impl Into<String>) -> Result<Self, InvalidId> {
                let value = value.into();
                validate($kind, &value)?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

id_type!(
    CallId,
    "call id",
    "Correlates one tool call with the result that answers it."
);
id_type!(
    ToolName,
    "tool name",
    "Names one published tool. Protocol-specific naming constraints belong to the adapter."
);
id_type!(
    Id,
    "binding id",
    "An operator-defined provider, account, endpoint or model identifier."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_refuse_whitespace_and_non_ascii() {
        assert!(CallId::new("call-1").is_ok());
        assert!(CallId::new("").is_err());
        assert!(CallId::new("call 1").is_err());
        assert!(CallId::new("call\n1").is_err());
        assert!(ToolName::new("workspace_read").is_ok());
        assert!(ToolName::new("workspace🧶").is_err());
    }

    #[test]
    fn a_tool_name_carries_a_dot_because_narrowing_it_is_a_wires_business() {
        // Pinned so that a later reader does not "tidy" the OpenAI pattern back into this crate.
        // the Responses adapter refuses a dotted name before it sends one; a wire that accepts one
        // must be free to.
        assert!(ToolName::new("workspace.read").is_ok());
    }

    #[test]
    fn ids_refuse_oversize() {
        assert!(CallId::new("c".repeat(MAX_ID_BYTES)).is_ok());
        assert!(CallId::new("c".repeat(MAX_ID_BYTES + 1)).is_err());
    }

    #[test]
    fn deserialization_applies_the_same_validation() {
        assert!(serde_json::from_str::<CallId>("\"call-1\"").is_ok());
        assert!(serde_json::from_str::<CallId>("\"\"").is_err());
        assert!(serde_json::from_str::<ToolName>("\"has space\"").is_err());
    }
}
