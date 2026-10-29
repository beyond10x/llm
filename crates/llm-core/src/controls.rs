//! What a turn asks beyond its sampling: thinking, parallel tool calls and advisory hints.
//!
//! The rules are `docs/design.md` "Client request fields". [`wire_effort`] is the one place the
//! reasoning table is written; every protocol crate and [`crate::TurnRequest::validate_for`] read
//! it from here.
use crate::{Error, Protocol, StreamEvent, ToolSpec, TurnRequest};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

/// The longest prompt-cache key a turn carries, in bytes.
pub const MAX_PROMPT_CACHE_KEY_BYTES: usize = 512;

/// The most beta names a turn carries.
pub const MAX_ANTHROPIC_BETAS: usize = 32;

/// The longest beta name a turn carries, in bytes.
pub const MAX_ANTHROPIC_BETA_BYTES: usize = 128;

/// The code of the warning a client emits for the hints its wire did not send.
pub const ADVISORY_OMITTED: &str = "advisory-omitted";

/// How the model is asked to think, beyond the effort name in [`crate::Sampling`].
///
/// Read strictly: `budget_tokens` is present exactly when the mode is `enabled`, and no other
/// key is accepted beside `mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case", try_from = "RawThinking")]
pub enum Thinking {
    /// The model decides whether and how long to think: the behaviour of Chat Completions and
    /// Responses when no thinking control is sent.
    Adaptive,
    /// Think, within a positive token budget.
    Enabled { budget_tokens: u64 },
    /// Do not think.
    Disabled,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ThinkingMode {
    Adaptive,
    Enabled,
    Disabled,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawThinking {
    mode: ThinkingMode,
    #[serde(default)]
    budget_tokens: Option<u64>,
}

impl TryFrom<RawThinking> for Thinking {
    type Error = &'static str;
    fn try_from(raw: RawThinking) -> Result<Self, Self::Error> {
        match (raw.mode, raw.budget_tokens) {
            (ThinkingMode::Adaptive, None) => Ok(Self::Adaptive),
            (ThinkingMode::Disabled, None) => Ok(Self::Disabled),
            (ThinkingMode::Enabled, Some(budget_tokens)) => Ok(Self::Enabled { budget_tokens }),
            (ThinkingMode::Enabled, None) => Err("enabled thinking needs budget_tokens"),
            (ThinkingMode::Adaptive | ThinkingMode::Disabled, Some(_)) => {
                Err("only enabled thinking carries budget_tokens")
            }
        }
    }
}

/// How long a prompt-cache entry should live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CacheTtl {
    #[serde(rename = "5m")]
    FiveMinutes,
    #[serde(rename = "1h")]
    OneHour,
}

/// A caller's request to cache what this turn shares with the next.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptCache {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl: Option<CacheTtl>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verbosity {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReasoningSummary {
    Auto,
    Concise,
    Detailed,
}

/// What an ingress decoder read and no egress wire sends. Its value is never kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Unforwarded {
    /// Client-assigned identifiers of input items, meaningless to a provider that stores nothing.
    InputItemIds,
    /// Client telemetry with no model semantics.
    ClientMetadata,
}

/// The name an advisory hint is reported under, in the order it is reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Advisory {
    PromptCache,
    PromptCacheKey,
    Verbosity,
    ReasoningSummary,
    AnthropicBeta,
    InputItemIds,
    ClientMetadata,
}

impl Advisory {
    pub const fn name(self) -> &'static str {
        match self {
            Self::PromptCache => "prompt-cache",
            Self::PromptCacheKey => "prompt-cache-key",
            Self::Verbosity => "verbosity",
            Self::ReasoningSummary => "reasoning-summary",
            Self::AnthropicBeta => "anthropic-beta",
            Self::InputItemIds => "input-item-ids",
            Self::ClientMetadata => "client-metadata",
        }
    }
}

impl fmt::Display for Advisory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

impl From<Unforwarded> for Advisory {
    fn from(unforwarded: Unforwarded) -> Self {
        match unforwarded {
            Unforwarded::InputItemIds => Self::InputItemIds,
            Unforwarded::ClientMetadata => Self::ClientMetadata,
        }
    }
}

/// Advisory hints: a wire without the concept omits one, and the omission is reported.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_cache: Option<PromptCache>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verbosity: Option<Verbosity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_summary: Option<ReasoningSummary>,
    /// The Anthropic beta names the client asked for (its `anthropic-beta` header values), in
    /// the order it sent them: the Messages wire sends them as that header, Bedrock `InvokeModel`
    /// as the body's `anthropic_beta`. At most [`MAX_ANTHROPIC_BETAS`], each one distinct, at
    /// most [`MAX_ANTHROPIC_BETA_BYTES`] bytes of ASCII letters, digits, `-`, `_` and `.`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anthropic_beta: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub unforwarded: BTreeSet<Unforwarded>,
}

/// Whether `name` is a beta name a turn may carry: non-empty, at most
/// [`MAX_ANTHROPIC_BETA_BYTES`] bytes of ASCII letters, digits, `-`, `_` and `.`.
pub fn anthropic_beta_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_ANTHROPIC_BETA_BYTES
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

impl Hints {
    pub fn is_empty(&self) -> bool {
        self.prompt_cache.is_none()
            && self.prompt_cache_key.is_none()
            && self.verbosity.is_none()
            && self.reasoning_summary.is_none()
            && self.anthropic_beta.is_empty()
            && self.unforwarded.is_empty()
    }

    /// Every hint this turn carries, in [`Advisory`] order.
    pub fn advisories(&self) -> Vec<Advisory> {
        let mut present = Vec::new();
        if self.prompt_cache.is_some() {
            present.push(Advisory::PromptCache);
        }
        if self.prompt_cache_key.is_some() {
            present.push(Advisory::PromptCacheKey);
        }
        if self.verbosity.is_some() {
            present.push(Advisory::Verbosity);
        }
        if self.reasoning_summary.is_some() {
            present.push(Advisory::ReasoningSummary);
        }
        if !self.anthropic_beta.is_empty() {
            present.push(Advisory::AnthropicBeta);
        }
        present.extend(self.unforwarded.iter().copied().map(Advisory::from));
        present
    }

    /// The hints this turn carries that a wire sending only `carried` omits.
    pub fn omitted_by(&self, carried: &[Advisory]) -> Vec<Advisory> {
        self.advisories()
            .into_iter()
            .filter(|advisory| !carried.contains(advisory))
            .collect()
    }

    fn validate(&self) -> Result<(), Error> {
        if self
            .prompt_cache_key
            .as_ref()
            .is_some_and(|key| key.is_empty() || key.len() > MAX_PROMPT_CACHE_KEY_BYTES)
        {
            return Err(Error::invalid(
                "prompt cache key must be non-empty and at most 512 bytes",
            ));
        }
        if self.anthropic_beta.len() > MAX_ANTHROPIC_BETAS {
            return Err(Error::invalid(
                "a turn carries at most 32 anthropic-beta names",
            ));
        }
        if !self
            .anthropic_beta
            .iter()
            .all(|name| anthropic_beta_name(name))
        {
            return Err(Error::invalid(
                "an anthropic-beta name is 1 to 128 ASCII letters, digits, '-', '_' or '.'",
            ));
        }
        let distinct: BTreeSet<&String> = self.anthropic_beta.iter().collect();
        if distinct.len() != self.anthropic_beta.len() {
            return Err(Error::invalid("an anthropic-beta name is given twice"));
        }
        Ok(())
    }
}

/// What a turn asks beyond sampling. Empty controls are the 0.6.0 turn exactly.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Controls {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<Thinking>,
    /// Whether the model may propose several tool calls in one turn. Needs published tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    #[serde(default, skip_serializing_if = "Hints::is_empty")]
    pub hints: Hints,
    /// Return a well-formed call to a tool this turn did not publish instead of refusing the
    /// turn, so the caller can answer it as a refusal the model learns from. Checked only by
    /// `TurnOutcome::validate_for`; no wire sends it. Every other outcome check still refuses.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub return_unpublished_calls: bool,
}

impl Controls {
    pub fn is_empty(&self) -> bool {
        self.thinking.is_none()
            && self.parallel_tool_calls.is_none()
            && self.hints.is_empty()
            && !self.return_unpublished_calls
    }

    pub(crate) fn validate(&self, tools: &[ToolSpec]) -> Result<(), Error> {
        if self.thinking == Some(Thinking::Enabled { budget_tokens: 0 }) {
            return Err(Error::invalid("thinking budget cannot be zero"));
        }
        if self.parallel_tool_calls.is_some() && tools.is_empty() {
            return Err(Error::invalid("parallel tool calls need published tools"));
        }
        self.hints.validate()
    }
}

/// The effort name `protocol` receives for `request`: the reasoning table of `docs/design.md`.
///
/// Messages receives the neutral effort as it is, and its thinking control separately; so does
/// Bedrock Converse, whose encoder decides what of either it can carry for the model. Chat
/// Completions and Responses have no effort `max` and no thinking control: `max` is sent as their
/// highest, `xhigh`; adaptive thinking is their own behaviour and sends nothing; disabled thinking
/// is the effort `none`.
///
/// # Errors
/// Refuses, on Chat Completions and Responses, a thinking budget and disabled thinking beside an
/// effort other than `none`.
pub fn wire_effort(protocol: Protocol, request: &TurnRequest) -> Result<Option<String>, Error> {
    let effort = request.sampling.reasoning_effort.as_deref();
    let wire = match protocol {
        Protocol::Messages | Protocol::BedrockConverse => {
            return Ok(effort.map(ToOwned::to_owned));
        }
        Protocol::ChatCompletions => "Chat Completions",
        Protocol::Responses => "Responses",
    };
    match request.controls.thinking {
        None | Some(Thinking::Adaptive) => {
            Ok(effort.map(|effort| if effort == "max" { "xhigh" } else { effort }.to_owned()))
        }
        Some(Thinking::Enabled { .. }) => {
            Err(Error::unsupported(format!("{wire} has no thinking budget")))
        }
        Some(Thinking::Disabled) => match effort {
            None | Some("none") => Ok(Some("none".to_owned())),
            Some(_) => Err(Error::unsupported(format!(
                "{wire} cannot hold disabled thinking beside an effort"
            ))),
        },
    }
}

/// The warning a client emits once a turn has succeeded, naming the hints `wire` did not send;
/// `None` when it sent them all.
///
/// Emitted after the turn and never before output: a warning on the sink counts as visible
/// output, and an attempt that showed output is no longer eligible for fallback.
pub fn advisory_warning(wire: &str, omitted: &[Advisory]) -> Option<StreamEvent> {
    if omitted.is_empty() {
        return None;
    }
    let names = omitted
        .iter()
        .map(|advisory| advisory.name())
        .collect::<Vec<_>>()
        .join(", ");
    Some(StreamEvent::Warning {
        code: ADVISORY_OMITTED.to_owned(),
        message: format!("{wire} carries no {names}; the advisory hint was not sent"),
    })
}

/// The longest field name a by-name refusal copies.
const MAX_FIELD_NAME_BYTES: usize = 64;

/// The path of `key` under `at`, for a refusal that names a field.
///
/// A field name is caller text, so only a bounded name of the wires' own shape (ASCII letters,
/// digits, `_` and `-`, at most 64 bytes) is copied; any other is written `?`. `at` is a dotted
/// path from the body root (`messages[].content[]`), empty for the root itself.
pub fn field_path(at: &str, key: &str) -> String {
    let key = if (1..=MAX_FIELD_NAME_BYTES).contains(&key.len())
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
