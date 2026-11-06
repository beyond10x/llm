use crate::{Error, Id};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Protocol {
    Responses,
    Messages,
    ChatCompletions,
    /// Amazon Bedrock's Converse wire. Only `llm-bedrock`'s `BedrockClient` serves it: it is
    /// signed per request, so no binding or catalog can name it. A routes document names it
    /// only on a `bedrock` endpoint, which stays outside the catalog it expands into.
    BedrockConverse,
}

/// Why a binding or catalog cannot name [`Protocol::BedrockConverse`], and why no base URL
/// builds a request URL for it.
pub const BEDROCK_CONVERSE_REFUSAL: &str =
    "bedrock-converse is served by llm-bedrock's BedrockClient, not by a binding";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthKind {
    Anonymous,
    Bearer,
    ApiKey,
    /// A caller-supplied subscription token. Sent as a bearer, but only over Messages, where the
    /// route adds its own subscription presentation; never billed as metered use.
    SubscriptionOauth,
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
///
/// The struct may gain declarations, so it is not built by literal outside this crate: build one
/// with [`Capabilities::text`] and the `with_*` setters.
///
/// ```
/// use llm_core::Capabilities;
///
/// let capabilities = Capabilities::text(200_000, 64_000)
///     .with_tools(true)
///     .with_tool_choice(true)
///     .with_reasoning_efforts(["low", "high"]);
/// assert!(capabilities.tools && !capabilities.temperature);
/// ```
///
/// ```compile_fail,E0639
/// use llm_core::Capabilities;
///
/// let capabilities = Capabilities {
///     tools: true,
///     tool_choice: true,
///     temperature: false,
///     top_p: false,
///     reasoning_efforts: vec!["low".to_owned(), "high".to_owned()],
///     context_window: 200_000,
///     max_output_tokens: 64_000,
///     output_streaming: true,
///     input_during_output: false,
/// };
/// assert!(capabilities.tools && !capabilities.temperature);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
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
    /// Whether the model's text reaches the sink incrementally, as it is generated, rather than
    /// only at the end. Every client in this workspace streams. Absent from a document means
    /// true, and true is not written, so a document from before the field keeps its bytes.
    #[serde(default = "streams", skip_serializing_if = "is_true")]
    pub output_streaming: bool,
    /// Whether input can reach a turn still producing output. False for every client here: each
    /// posts one HTTP request and reads server-sent events, and none speaks the Responses WebSocket
    /// mode steering needs (`docs/design.md`, "Input into a running turn"). Absent from a document
    /// means false, and false is not written.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub input_during_output: bool,
    /// Whether the model takes a JSON Schema answer format (`controls.output_format`). `None`
    /// is not declared: selection does not reject the target, and its wire carries the format or
    /// refuses it before any I/O. `Some(false)` is rejected by selection and refused by
    /// [`crate::TurnRequest::validate_for`]. Absent from a document means `None`, and `None` is
    /// not written, so a document from before the field keeps its bytes.
    #[serde(
        default,
        deserialize_with = "declared",
        skip_serializing_if = "Option::is_none"
    )]
    pub structured_output: Option<bool>,
    /// Whether the model takes an image in a user turn (`Item::UserImage`), declared as
    /// `structured_output` is.
    #[serde(
        default,
        deserialize_with = "declared",
        skip_serializing_if = "Option::is_none"
    )]
    pub image_input: Option<bool>,
    /// The most texts one embed request to the model may carry; a larger request is sent in
    /// batches of at most this many, in order. Absent sends every text in one request, and
    /// absent is not written, so a document from before the field keeps its bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embed_batch_limit: Option<u32>,
}

const fn streams() -> bool {
    true
}

/// A declaration present in a document is a boolean; only its absence leaves it undeclared.
fn declared<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<bool>, D::Error> {
    bool::deserialize(deserializer).map(Some)
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes the field by reference"
)]
const fn is_true(value: &bool) -> bool {
    *value
}

impl Capabilities {
    /// A text-only declaration: output streams and no input reaches a running turn, as every
    /// client in this workspace does; every optional setting is refused until explicitly enabled,
    /// and structured output and image input are not declared.
    pub const fn text(context_window: u64, max_output_tokens: u64) -> Self {
        Self {
            tools: false,
            tool_choice: false,
            temperature: false,
            top_p: false,
            reasoning_efforts: Vec::new(),
            context_window,
            max_output_tokens,
            output_streaming: true,
            input_during_output: false,
            structured_output: None,
            image_input: None,
            embed_batch_limit: None,
        }
    }

    #[must_use]
    pub const fn with_tools(mut self, tools: bool) -> Self {
        self.tools = tools;
        self
    }

    #[must_use]
    pub const fn with_tool_choice(mut self, tool_choice: bool) -> Self {
        self.tool_choice = tool_choice;
        self
    }

    #[must_use]
    pub const fn with_temperature(mut self, temperature: bool) -> Self {
        self.temperature = temperature;
        self
    }

    #[must_use]
    pub const fn with_top_p(mut self, top_p: bool) -> Self {
        self.top_p = top_p;
        self
    }

    /// The reasoning effort names the model accepts, in place of any declared before.
    #[must_use]
    pub fn with_reasoning_efforts<I, S>(mut self, efforts: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.reasoning_efforts = efforts.into_iter().map(Into::into).collect();
        self
    }

    #[must_use]
    pub const fn with_output_streaming(mut self, output_streaming: bool) -> Self {
        self.output_streaming = output_streaming;
        self
    }

    #[must_use]
    pub const fn with_input_during_output(mut self, input_during_output: bool) -> Self {
        self.input_during_output = input_during_output;
        self
    }

    /// Declares whether the model takes structured output; set the field to `None` to leave it
    /// undeclared again.
    #[must_use]
    pub const fn with_structured_output(mut self, structured_output: bool) -> Self {
        self.structured_output = Some(structured_output);
        self
    }

    /// Declares whether the model takes image input; set the field to `None` to leave it
    /// undeclared again.
    #[must_use]
    pub const fn with_image_input(mut self, image_input: bool) -> Self {
        self.image_input = Some(image_input);
        self
    }

    /// Declares the most texts one embed request may carry; set the field to `None` to send
    /// every text in one request again.
    #[must_use]
    pub const fn with_embed_batch_limit(mut self, texts: u32) -> Self {
        self.embed_batch_limit = Some(texts);
        self
    }

    /// Checks the declaration against what `protocol` can carry. Binding, catalog and routes
    /// documents are checked with it; [`Capabilities::validate`] does not apply it, so a model
    /// built in code keeps its own claim.
    ///
    /// # Errors
    /// Refuses input during output, which no protocol in this workspace carries, and output that
    /// does not stream, since every protocol in this workspace streams its output. The message
    /// names the rule and never a value. Refuses [`Protocol::BedrockConverse`] by name: no
    /// document serves it.
    pub fn validate_protocol(&self, protocol: Protocol) -> Result<(), Error> {
        match protocol {
            Protocol::BedrockConverse => {
                return Err(Error::unsupported(BEDROCK_CONVERSE_REFUSAL));
            }
            Protocol::Responses | Protocol::Messages | Protocol::ChatCompletions => {
                if self.input_during_output {
                    return Err(Error::invalid(
                        "no protocol in this workspace carries input during output",
                    ));
                }
                if !self.output_streaming {
                    return Err(Error::invalid(
                        "every protocol in this workspace streams its output",
                    ));
                }
            }
        }
        Ok(())
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
