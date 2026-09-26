use crate::{CallId, Provenance, ToolName};
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
}

impl Item {
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
}
