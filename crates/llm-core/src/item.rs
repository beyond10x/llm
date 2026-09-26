use crate::{CallId, Error, Protocol, Provenance, ToolName};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A model's request to its caller, carrying no permission to execute.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCall {
    pub call_id: CallId,
    pub name: ToolName,
    pub arguments: Value,
}

/// Stateless conversation content. Opaque payloads remain bound to their original serving model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Item {
    UserText {
        text: String,
    },
    AssistantText {
        text: String,
    },
    ToolCall(ToolCall),
    ToolResult {
        call_id: CallId,
        output: Value,
        failed: bool,
    },
    Opaque {
        provenance: Provenance,
        payload: Value,
    },
    /// Opaque continuation state carried across an ingress surface that could not observe which
    /// binding produced it: the payload as a JSON value (JSON-equal to what arrived, not byte-equal:
    /// keys re-serialize sorted), and the protocol it was read from.
    ///
    /// **Never sendable.** Every egress path refuses it with [`Item::UNATTRIBUTED_REFUSAL`]
    /// until a caller binds it with [`Item::bind_unattributed`] or
    /// [`crate::TurnRequest::bind_unattributed`]. No adapter makes that decision.
    UnattributedOpaque {
        protocol: Protocol,
        payload: Value,
    },
}

impl Item {
    /// The one diagnostic every egress path refuses unbound opaque state with.
    ///
    /// Fixed, so a caller tells this refusal from every other `unsupported` one by name, and so
    /// no payload byte ever reaches a diagnostic.
    pub const UNATTRIBUTED_REFUSAL: &str =
        "unattributed opaque state is not sendable until a caller binds it to a target";

    pub fn user(text: impl Into<String>) -> Self {
        Self::UserText { text: text.into() }
    }
    pub fn assistant(text: impl Into<String>) -> Self {
        Self::AssistantText { text: text.into() }
    }
    pub const fn as_tool_call(&self) -> Option<&ToolCall> {
        if let Self::ToolCall(call) = self {
            Some(call)
        } else {
            None
        }
    }

    /// The explicit caller decision that unattributed state belongs to `target`.
    ///
    /// Only the caller can make it: it asserts an origin nobody observed, so it is the caller's
    /// assertion and never an adapter's. Every other item is returned unchanged, bound opaque
    /// state included — rebinding state that already names its binding is not this decision.
    ///
    /// # Errors
    /// Refuses a target of a protocol other than the one the payload was read from.
    pub fn bind_unattributed(self, target: &Provenance) -> Result<Self, Error> {
        match self {
            Self::UnattributedOpaque { protocol, payload } => {
                if protocol != target.protocol {
                    return Err(Error::unsupported(
                        "unattributed opaque state was read from another protocol than the target's",
                    ));
                }
                Ok(Self::Opaque {
                    provenance: target.clone(),
                    payload,
                })
            }
            other => Ok(other),
        }
    }
}
