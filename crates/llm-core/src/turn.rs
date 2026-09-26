//! Turn and tool projection values adapted from Harness, without execution authority.
use crate::{
    Capabilities, Error, Id, Item, MAX_INSTRUCTION_BYTES, MAX_ITEMS, MAX_REQUEST_BYTES,
    MAX_TOOL_ARGUMENT_BYTES, MAX_TOOL_DESCRIPTION_BYTES, MAX_TOOL_RESULT_BYTES, MAX_TOOLS,
    Provenance, ToolCall, ToolName, exceeds,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

/// A schema published to a model. Executing a proposed call belongs entirely to the caller.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSpec {
    pub name: ToolName,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sampling {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

impl Sampling {
    pub const fn is_empty(&self) -> bool {
        self.temperature.is_none() && self.top_p.is_none() && self.reasoning_effort.is_none()
    }
    /// # Errors
    /// Rejects non-finite/out-of-range sampling and empty reasoning effort.
    pub fn validate(&self) -> Result<(), Error> {
        if self
            .temperature
            .is_some_and(|v| !v.is_finite() || !(0.0..=2.0).contains(&v))
        {
            return Err(Error::invalid(
                "temperature must be finite and within 0..=2",
            ));
        }
        if self
            .top_p
            .is_some_and(|v| !v.is_finite() || v <= 0.0 || v > 1.0)
        {
            return Err(Error::invalid(
                "top_p must be finite and within 0 (exclusive)..=1",
            ));
        }
        if self.reasoning_effort.as_ref().is_some_and(String::is_empty) {
            return Err(Error::invalid("reasoning effort cannot be empty"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolChoice {
    #[default]
    Auto,
    Required,
    Named(ToolName),
}
impl ToolChoice {
    pub const fn is_auto(&self) -> bool {
        matches!(self, Self::Auto)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnRequest {
    pub model: String,
    pub instructions: String,
    pub items: Vec<Item>,
    pub tools: Vec<ToolSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Sampling::is_empty")]
    pub sampling: Sampling,
    #[serde(default, skip_serializing_if = "ToolChoice::is_auto")]
    pub tool_choice: ToolChoice,
}

impl TurnRequest {
    pub fn new(model: impl Into<String>, items: Vec<Item>) -> Self {
        Self {
            model: model.into(),
            instructions: String::new(),
            items,
            tools: Vec::new(),
            max_output_tokens: None,
            sampling: Sampling::default(),
            tool_choice: ToolChoice::Auto,
        }
    }

    /// # Errors
    /// Rejects malformed fields, duplicate tool/call identifiers, and oversized input before I/O.
    pub fn validate(&self) -> Result<(), Error> {
        crate::Id::new(&self.model).map_err(|_| Error::invalid("invalid model identifier"))?;
        self.sampling.validate()?;
        if self.max_output_tokens == Some(0) {
            return Err(Error::invalid("output token limit cannot be zero"));
        }
        if self.instructions.len() > MAX_INSTRUCTION_BYTES
            || self.tools.len() > MAX_TOOLS
            || self.items.len() > MAX_ITEMS
        {
            return Err(Error::too_large(
                "request instructions, tools or items exceed their bound",
            ));
        }
        self.validate_tools()?;
        let mut calls = BTreeSet::new();
        let mut results = BTreeSet::new();
        for item in &self.items {
            match item {
                Item::ToolCall(call) => {
                    if !calls.insert(&call.call_id) {
                        return Err(Error::invalid("duplicate tool call id"));
                    }
                    if exceeds(&call.arguments, MAX_TOOL_ARGUMENT_BYTES) {
                        return Err(Error::too_large("tool arguments exceed their bound"));
                    }
                }
                Item::ToolResult {
                    call_id, output, ..
                } => {
                    if !calls.contains(call_id) || !results.insert(call_id) {
                        return Err(Error::invalid(
                            "tool result must answer one preceding, unanswered call",
                        ));
                    }
                    if exceeds(output, MAX_TOOL_RESULT_BYTES) {
                        return Err(Error::too_large("tool result exceeds its bound"));
                    }
                }
                _ => {}
            }
        }
        if calls != results {
            return Err(Error::invalid(
                "every prior tool call needs a result before the next turn",
            ));
        }
        if exceeds(self, MAX_REQUEST_BYTES) {
            return Err(Error::too_large("encoded request exceeds its bound"));
        }
        Ok(())
    }

    fn validate_tools(&self) -> Result<(), Error> {
        if !self.tool_choice.is_auto() && self.tools.is_empty() {
            return Err(Error::invalid("required tool choice needs published tools"));
        }
        if let ToolChoice::Named(name) = &self.tool_choice
            && !self.tools.iter().any(|tool| &tool.name == name)
        {
            return Err(Error::invalid("named tool choice is not published"));
        }
        let mut names = BTreeSet::new();
        for tool in &self.tools {
            if !names.insert(&tool.name) {
                return Err(Error::invalid("duplicate tool name"));
            }
            if tool.description.len() > MAX_TOOL_DESCRIPTION_BYTES
                || exceeds(&tool.input_schema, MAX_TOOL_ARGUMENT_BYTES)
            {
                return Err(Error::too_large(
                    "tool description or schema exceeds its bound",
                ));
            }
            if !tool.input_schema.is_object() && !tool.input_schema.is_boolean() {
                return Err(Error::invalid(
                    "tool schema must be a JSON Schema object or boolean",
                ));
            }
        }
        Ok(())
    }

    /// # Errors
    /// Refuses incompatible opaque state, target model or unsupported requested settings.
    pub fn validate_for(
        &self,
        target: &Provenance,
        capabilities: &Capabilities,
    ) -> Result<(), Error> {
        self.validate()?;
        capabilities.validate()?;
        if self.model != target.model.as_str() {
            return Err(Error::invalid(
                "request model differs from selected binding",
            ));
        }
        for item in &self.items {
            if let Item::Opaque { provenance, .. } = item
                && provenance != target
            {
                return Err(Error::unsupported(
                    "opaque state belongs to a different serving binding",
                ));
            }
        }
        if (!self.tools.is_empty() && !capabilities.tools)
            || (!self.tool_choice.is_auto() && !capabilities.tool_choice)
            || (self.sampling.temperature.is_some() && !capabilities.temperature)
            || (self.sampling.top_p.is_some() && !capabilities.top_p)
            || self
                .sampling
                .reasoning_effort
                .as_ref()
                .is_some_and(|v| !capabilities.reasoning_efforts.contains(v))
        {
            return Err(Error::unsupported(
                "binding does not support a requested tool or sampling setting",
            ));
        }
        if self
            .max_output_tokens
            .is_some_and(|v| v > capabilities.max_output_tokens)
        {
            return Err(Error::unsupported(
                "requested output exceeds binding capability",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum StopReason {
    EndTurn,
    ToolCalls,
    MaxOutputTokens,
    Incomplete { reason: String },
}

/// Provider-reported counts. Cache counts partition input; reasoning is a subset of output.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Usage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_output_tokens: Option<u64>,
}

impl Usage {
    /// # Errors
    /// Refuses overflow and contradictory known counts, preserving unknown counts as unknown.
    pub fn validate(&self) -> Result<(), Error> {
        let known_cache = self
            .cached_input_tokens
            .unwrap_or(0)
            .checked_add(self.cache_creation_input_tokens.unwrap_or(0))
            .ok_or_else(|| Error::protocol("reported cache tokens overflow"))?;
        if self.input_tokens.is_some_and(|total| known_cache > total)
            || self
                .output_tokens
                .zip(self.reasoning_output_tokens)
                .is_some_and(|(total, reasoning)| reasoning > total)
        {
            return Err(Error::protocol("reported usage subsets exceed totals"));
        }
        Ok(())
    }
}

/// Evidence reported by one bound upstream attempt, retained even when that attempt fails.
/// Missing identifiers and counters stay unknown; a requested alias is not an observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnObservation {
    pub binding: Provenance,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_model: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// The reported counters are terminal, not necessarily complete or invoiced.
    /// False makes known counters a partial snapshot, never a final cost.
    pub final_usage: bool,
}

impl TurnObservation {
    pub const fn new(binding: Provenance) -> Self {
        Self {
            binding,
            upstream_model: None,
            response_id: None,
            usage: None,
            final_usage: false,
        }
    }

    /// # Errors
    /// Refuses evidence from a different binding and contradictory known counters.
    pub fn validate_for(&self, target: &Provenance) -> Result<(), Error> {
        if &self.binding != target {
            return Err(Error::protocol("observation carries a different binding"));
        }
        if let Some(usage) = &self.usage {
            usage.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnOutcome {
    pub stop_reason: StopReason,
    pub items: Vec<Item>,
    pub observation: TurnObservation,
}
impl TurnOutcome {
    pub fn tool_calls(&self) -> impl Iterator<Item = &ToolCall> {
        self.items.iter().filter_map(Item::as_tool_call)
    }

    /// # Errors
    /// Refuses invalid model output, foreign opaque state, unknown tools and contradictory usage.
    pub fn validate_for(&self, request: &TurnRequest, target: &Provenance) -> Result<(), Error> {
        self.observation.validate_for(target)?;
        if !self.observation.final_usage {
            return Err(Error::protocol(
                "successful outcome lacks terminal evidence",
            ));
        }
        if self.items.len() > MAX_ITEMS || exceeds(self, MAX_REQUEST_BYTES) {
            return Err(Error::too_large("model output exceeds its bound"));
        }
        let mut calls = BTreeSet::new();
        for item in &self.items {
            match item {
                Item::UserText { .. } | Item::ToolResult { .. } => {
                    return Err(Error::protocol(
                        "model output contains caller-owned content",
                    ));
                }
                Item::ToolCall(call) => {
                    if !calls.insert(&call.call_id)
                        || !request.tools.iter().any(|tool| tool.name == call.name)
                    {
                        return Err(Error::protocol(
                            "model output contains a duplicate call or unpublished tool",
                        ));
                    }
                    if let ToolChoice::Named(name) = &request.tool_choice
                        && *name != call.name
                    {
                        return Err(Error::protocol(
                            "model called a tool other than the required named tool",
                        ));
                    }
                    if exceeds(&call.arguments, MAX_TOOL_ARGUMENT_BYTES) {
                        return Err(Error::too_large("model tool arguments exceed their bound"));
                    }
                }
                Item::Opaque { provenance, .. } if provenance != target => {
                    return Err(Error::protocol("model output carries foreign opaque state"));
                }
                _ => {}
            }
        }
        if (self.stop_reason == StopReason::ToolCalls && calls.is_empty())
            || (self.stop_reason == StopReason::EndTurn
                && (!calls.is_empty() || !request.tool_choice.is_auto()))
        {
            return Err(Error::protocol(
                "model terminal reason contradicts its tool obligations",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum TurnFormat {
    #[serde(rename = "llm.turn/2")]
    V2,
}

/// A versioned persisted request. Unknown envelope versions and fields refuse deserialization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnDocument {
    format: TurnFormat,
    pub request: TurnRequest,
}
impl TurnDocument {
    pub const fn new(request: TurnRequest) -> Self {
        Self {
            format: TurnFormat::V2,
            request,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum OutcomeFormat {
    #[serde(rename = "llm.outcome/3")]
    V3,
}

/// Versioned persisted output, distinct from the in-process result value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutcomeDocument {
    format: OutcomeFormat,
    pub outcome: TurnOutcome,
}
impl OutcomeDocument {
    pub const fn new(outcome: TurnOutcome) -> Self {
        Self {
            format: OutcomeFormat::V3,
            outcome,
        }
    }
}
