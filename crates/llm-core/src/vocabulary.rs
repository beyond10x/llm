use crate::{Error, Id};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Protocol {
    Responses,
    Messages,
    ChatCompletions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthKind {
    Anonymous,
    Bearer,
    ApiKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BillingKind {
    Metered,
    Subscription,
    SelfHosted,
}

/// The exact selected binding. Opaque state may only return to this binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub protocol: Protocol,
    pub provider: Id,
    pub account: Id,
    pub endpoint: Id,
    pub model: Id,
    /// Identity of the concrete binding definition, including endpoint URL and upstream model.
    /// A caller must change this when a definition is repointed, even if its display IDs persist.
    pub binding_revision: Id,
}

/// Declared model/settings support. Context tokens require separate tokenizer/accounting evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
// Independent capability dimensions can be combined; they are not lifecycle states.
#[allow(clippy::struct_excessive_bools)]
pub struct Capabilities {
    pub tools: bool,
    pub tool_choice: bool,
    pub temperature: bool,
    pub top_p: bool,
    pub reasoning_efforts: Vec<String>,
    pub context_window: u64,
    pub max_output_tokens: u64,
}

impl Capabilities {
    /// A text-only declaration, with all optional settings refused until explicitly enabled.
    pub const fn text(context_window: u64, max_output_tokens: u64) -> Self {
        Self {
            tools: false,
            tool_choice: false,
            temperature: false,
            top_p: false,
            reasoning_efforts: Vec::new(),
            context_window,
            max_output_tokens,
        }
    }

    /// # Errors
    /// Refuses impossible token limits or empty reasoning effort names.
    pub fn validate(&self) -> Result<(), Error> {
        if self.context_window == 0
            || self.max_output_tokens == 0
            || self.max_output_tokens > self.context_window
        {
            return Err(Error::invalid(
                "capability token limits must be positive and output cannot exceed context",
            ));
        }
        if self.reasoning_efforts.iter().any(String::is_empty) {
            return Err(Error::invalid("reasoning effort names cannot be empty"));
        }
        Ok(())
    }
}
