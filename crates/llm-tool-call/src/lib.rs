#![forbid(unsafe_code)]

//! One forced tool call on any model, and a Responses model preset over the operator's Codex
//! login (story call-tool-helper). Moved from Loom's `intake-model` with its public API unchanged.
//!
//! Two things live here:
//!
//! - [`codex_model`] builds an [`llm_core::Model`] for the operator's Codex subscription: llm's
//!   `ResponsesClient` bound to [`CODEX_BASE_URL`], authenticated by llm's `CodexAuthFile` with the
//!   access token of the Codex CLI's `auth.json`. [`codex_model_at`] is the same with the endpoint
//!   and the login path given explicitly, which is what tests use.
//! - [`call_tool`] forces one named tool on any [`llm_core::Model`] and returns that call's
//!   arguments, or a typed [`ModelError`].
//!
//! The Codex login is read-only here. This crate never writes `auth.json` and never refreshes its
//! token: llm reads the file on every request, an expired token is refused as
//! [`ModelError::ExpiredCredential`], and running `codex` renews it. Building a model reads
//! nothing; a missing or expired login is refused when [`call_tool`] runs, before any request
//! leaves.
//!
//! [`call_tool`] takes a `&dyn Model`, so a caller's tests can pass a recorded model and never
//! reach the Codex binding.

use std::{
    cell::RefCell,
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::Arc,
};

use llm_core::{
    BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item, Model, Protocol,
    StreamEvent, StreamSink, ToolChoice, ToolSpec, TurnRequest,
};
use llm_credentials::{
    ResolvedSecret, SecretError, SecretRef, SecretResolver,
    codex::{CodexAuthError, CodexAuthFile},
};
use llm_http::{HttpClient, Limits};
use llm_providers::{
    Account, BaseUrl, BindingDocument, Endpoint, Provider, ServedModel, ServingModel,
};
use llm_responses::ResponsesClient;
use serde_json::Value;

/// The Responses endpoint the Codex CLI uses for a `ChatGPT` subscription login.
pub const CODEX_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";

/// The one credential reference the Codex binding names.
const LOGIN_REFERENCE: &str = "codex-login";

/// The context and output bounds the binding declares. `call_tool` never asks for an output
/// limit, so these only bound what llm accepts locally; the backend applies its own.
const CONTEXT_WINDOW: u64 = 400_000;
const MAX_OUTPUT_TOKENS: u64 = 128_000;

/// Why a model call gave no usable answer.
#[derive(Debug)]
pub enum ModelError {
    /// The model answered without calling the forced tool.
    NoToolCall,
    /// The model called a tool other than the forced one.
    WrongTool,
    /// The model called the forced tool more than once, so no single answer exists.
    MultipleToolCalls(usize),
    /// The endpoint could not be reached, or the connection failed before the answer completed.
    Transport(Error),
    /// There is no Codex login to read: no `auth.json`, no access token in it, or neither
    /// `CODEX_HOME` nor `HOME` set. The message says to run `codex`.
    MissingCredential(String),
    /// The Codex login's access token has expired. The message says to run `codex`.
    ExpiredCredential(String),
    /// The Codex login exists but cannot be used: not a regular file, unreadable, too large, a
    /// relative path, or a token whose expiry cannot be read.
    UnusableCredential(String),
    /// The model binding could not be built: an invalid model identifier or endpoint URL.
    Setup(Error),
    /// Any other refusal from the model or its endpoint.
    Model(Error),
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoToolCall => f.write_str("the model answered without calling the forced tool"),
            Self::WrongTool => f.write_str("the model called a tool other than the forced one"),
            Self::MultipleToolCalls(count) => write!(
                f,
                "the model called the forced tool {count} times; exactly one call is an answer"
            ),
            Self::Transport(error) => write!(f, "the model endpoint failed: {error}"),
            Self::MissingCredential(message)
            | Self::ExpiredCredential(message)
            | Self::UnusableCredential(message) => f.write_str(message),
            Self::Setup(error) => write!(f, "the model binding cannot be built: {error}"),
            Self::Model(error) => write!(f, "the model call was refused: {error}"),
        }
    }
}

impl std::error::Error for ModelError {}

/// The Codex login file: `$CODEX_HOME/auth.json`, else `$HOME/.codex/auth.json`.
///
/// Takes the two variables' values rather than reading the environment, so it can be tested
/// without changing the process environment. An empty value counts as unset. The first variable
/// set decides: a relative `CODEX_HOME` is refused rather than skipped for `HOME`, because llm
/// refuses a relative login path on every call and the operator named that directory.
///
/// # Errors
/// [`ModelError::MissingCredential`] when neither is set, or when the one that decides is a
/// relative path; the message names the variable.
pub fn codex_auth_path(
    codex_home: Option<&OsStr>,
    home: Option<&OsStr>,
) -> Result<PathBuf, ModelError> {
    if let Some(codex_home) = codex_home.filter(|value| !value.is_empty()) {
        return Ok(absolute("CODEX_HOME", codex_home)?.join("auth.json"));
    }
    if let Some(home) = home.filter(|value| !value.is_empty()) {
        return Ok(absolute("HOME", home)?.join(".codex").join("auth.json"));
    }
    Err(ModelError::MissingCredential(
        "neither CODEX_HOME nor HOME is set, so there is no Codex login to read; run `codex` to log in"
            .to_owned(),
    ))
}

/// `value` of the environment variable `name` as a directory, refused unless it is absolute.
fn absolute<'a>(name: &str, value: &'a OsStr) -> Result<&'a Path, ModelError> {
    let path = Path::new(value);
    if path.is_absolute() {
        return Ok(path);
    }
    Err(ModelError::MissingCredential(format!(
        "{name} is the relative path {}, so the Codex login cannot be located; set {name} to an absolute directory",
        path.display()
    )))
}

/// The operator's Codex subscription as a model: [`codex_model_at`] with [`CODEX_BASE_URL`] and
/// the login [`codex_auth_path`] finds from `CODEX_HOME` and `HOME`.
///
/// # Errors
/// [`ModelError::MissingCredential`] when neither variable is set; otherwise as
/// [`codex_model_at`].
pub fn codex_model(model_id: &str) -> Result<impl Model + use<>, ModelError> {
    let auth_path = codex_auth_path(
        std::env::var_os("CODEX_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )?;
    codex_model_at(model_id, CODEX_BASE_URL, &auth_path)
}

/// A Codex Responses model at `base_url`, authenticated by the access token of the Codex login at
/// `auth_path`, which should be absolute (a relative path is refused on every call).
///
/// `model_id` is both the identifier the turn carries and the model name sent upstream. Nothing
/// is read or sent here: the login is read on every call, by [`call_tool`], before the request.
///
/// # Errors
/// [`ModelError::Setup`] for a model identifier or endpoint URL llm refuses.
pub fn codex_model_at(
    model_id: &str,
    base_url: &str,
    auth_path: &Path,
) -> Result<impl Model + use<>, ModelError> {
    let id = |value: &str| {
        Id::new(value).map_err(|_| ModelError::Setup(Error::invalid("invalid identifier")))
    };
    let reference = SecretRef::new(LOGIN_REFERENCE)
        .map_err(|_| ModelError::Setup(Error::invalid("invalid credential reference")))?;
    let binding = BindingDocument::new(
        Provider {
            id: id("openai")?,
            category: id("hosted")?,
        },
        Account {
            id: id("codex-subscription")?,
            provider_id: id("openai")?,
            auth_kind: llm_core::AuthKind::Bearer,
            billing_kind: llm_core::BillingKind::Subscription,
            secret_reference_id: Some(reference.clone()),
            api_key_header: None,
        },
        Endpoint {
            id: id("codex-backend")?,
            account_id: id("codex-subscription")?,
            base_url: BaseUrl::new(base_url).map_err(ModelError::Setup)?,
        },
        ServedModel {
            id: id(model_id)?,
            upstream_name: id(model_id)?,
        },
        ServingModel {
            id: id("codex-responses")?,
            endpoint_id: id("codex-backend")?,
            model_id: id(model_id)?,
            protocol: Protocol::Responses,
            capabilities: Capabilities {
                tools: true,
                tool_choice: true,
                temperature: false,
                top_p: false,
                reasoning_efforts: Vec::new(),
                context_window: CONTEXT_WINDOW,
                max_output_tokens: MAX_OUTPUT_TOKENS,
            },
        },
    )
    .bind()
    .map_err(ModelError::Setup)?;
    let http = HttpClient::new(Limits::default()).map_err(ModelError::Setup)?;
    let login = CodexLogin {
        file: CodexAuthFile::new(reference, auth_path),
    };
    ResponsesClient::new(binding, http, Arc::new(login)).map_err(ModelError::Setup)
}

tokio::task_local! {
    /// The typed refusal of the Codex login, kept for the [`call_tool`] whose turn caused it.
    ///
    /// llm hands a resolver's refusal on as an untyped `Unauthorized` or `Unavailable` error, so
    /// the typed [`CodexAuthError`] travels beside it, scoped to one call.
    static LOGIN_REFUSAL: RefCell<Option<CodexAuthError>>;
}

/// llm's `CodexAuthFile`, keeping its typed refusal for the call in progress.
///
/// The slot is task-local: the refusal is kept only when this resolver runs on the task that
/// awaits [`call_tool`]. A [`Model`] that runs its turn on a spawned task (`tokio::spawn`) resolves
/// outside the slot, and `call_tool` then sees only llm's untyped `Unauthorized` or `Unavailable`
/// refusal, returned as [`ModelError::Model`].
struct CodexLogin {
    file: CodexAuthFile,
}

impl SecretResolver for CodexLogin {
    fn resolve<'a>(
        &'a self,
        reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move {
            self.file.read(reference).await.map_err(|refusal| {
                let kind = refusal.kind();
                // Outside `call_tool` there is no slot, and the untyped kind is all there is.
                let _ = LOGIN_REFUSAL.try_with(|slot| slot.replace(Some(refusal)));
                kind
            })
        })
    }
}

/// Forces `tool` on `model` and returns the arguments of the one call it makes.
///
/// One turn, one attempt: `instructions` as the standing instruction, `items` as the
/// conversation, `tool` as the only tool, chosen by name. No output limit is set, because the
/// Codex backend may refuse one.
///
/// # Errors
/// [`ModelError::NoToolCall`] or [`ModelError::WrongTool`] when the answer is not one call to
/// `tool`, [`ModelError::MultipleToolCalls`] for more than one, [`ModelError::Transport`] when
/// the connection fails or the stream stops before its answer completes, the credential variants
/// when the Codex login cannot be used (before anything is sent), and [`ModelError::Model`] for any
/// other refusal, including arguments that are not a JSON object.
///
/// A Codex login refusal is typed only when the model resolves its credential on the task that
/// awaits this call; a model that spawns its turn onto another task gets [`ModelError::Model`]
/// for it instead (see `CodexLogin`). A refusal kept from a turn that still succeeded, such as a
/// model that falls back to another after the login is refused, does not override its answer.
pub async fn call_tool(
    model: &dyn Model,
    instructions: &str,
    items: Vec<Item>,
    tool: ToolSpec,
) -> Result<Value, ModelError> {
    let name = tool.name.clone();
    let mut request = TurnRequest::new(model.provenance().model.as_str(), items);
    request.instructions = instructions.to_owned();
    request.tools = vec![tool];
    request.tool_choice = ToolChoice::Named(name.clone());
    let outcome = LOGIN_REFUSAL
        .scope(RefCell::new(None), async {
            model
                .turn(&request, &mut Discard, &Cancel::new())
                .await
                // The slot is read only for a failed turn.
                .map_err(|error| match LOGIN_REFUSAL.with(RefCell::take) {
                    Some(refusal) => credential_error(&refusal),
                    None => classify(error),
                })
        })
        .await?;
    let calls: Vec<_> = outcome.tool_calls().collect();
    match calls.as_slice() {
        [] => Err(ModelError::NoToolCall),
        calls if calls.iter().any(|call| call.name != name) => Err(ModelError::WrongTool),
        [call] if !call.arguments.is_object() => Err(ModelError::Model(
            Error::protocol(ARGUMENTS_NOT_AN_OBJECT).with_dispatch(Dispatch::Accepted),
        )),
        [call] => Ok(call.arguments.clone()),
        calls => Err(ModelError::MultipleToolCalls(calls.len())),
    }
}

/// llm's refusal of a call whose arguments are not a JSON object, raised by its Responses stream
/// decoder (`crates/llm-responses/src/stream.rs` line 512 in this workspace). `call_tool` gives a
/// recorded model's answer the same refusal; a unit test pins the text against llm.
const ARGUMENTS_NOT_AN_OBJECT: &str = "function call arguments are not a JSON object";

/// The typed Codex login refusal as a [`ModelError`]; its message names the file and says to run
/// `codex`, and never carries the token.
fn credential_error(refusal: &CodexAuthError) -> ModelError {
    let message = refusal.to_string();
    match refusal.kind() {
        SecretError::Missing => ModelError::MissingCredential(message),
        SecretError::Expired => ModelError::ExpiredCredential(message),
        _ => ModelError::UnusableCredential(message),
    }
}

/// A turn's failure as a [`ModelError`].
fn classify(error: Error) -> ModelError {
    match pinned_refusal(&error) {
        Some(Pinned::WrongTool) => ModelError::WrongTool,
        Some(Pinned::NoToolCall) => ModelError::NoToolCall,
        Some(Pinned::CutOff) => ModelError::Transport(error),
        None if error.code == ErrorCode::Transport => ModelError::Transport(error),
        None => ModelError::Model(error),
    }
}

/// An llm `Protocol` refusal that [`pinned_refusal`] recognises by its message.
#[derive(Debug, PartialEq, Eq)]
enum Pinned {
    WrongTool,
    NoToolCall,
    /// The stream stopped before its terminal event: the answer never completed.
    CutOff,
}

/// llm's `Protocol` refusals that `call_tool` types, told apart by their fixed messages.
///
/// llm reports these only as `ErrorCode::Protocol` with a fixed message. The sources, in this
/// workspace:
///
/// | source | message | here |
/// | --- | --- | --- |
/// | `crates/llm-core/src/turn.rs:383` | `model output contains a duplicate call or unpublished tool` | [`ModelError::WrongTool`] |
/// | `crates/llm-core/src/turn.rs:390` | `model called a tool other than the required named tool` | [`ModelError::WrongTool`] |
/// | `crates/llm-core/src/turn.rs:415` | `model terminal reason contradicts its tool obligations` | [`ModelError::NoToolCall`] |
/// | `crates/llm-responses/src/stream.rs:328` | `the stream ended before the response reached a terminal state` | [`ModelError::Transport`] |
///
/// The first three are `TurnOutcome::validate_for`. `call_tool` publishes only the forced tool,
/// so line 383 means a call to a tool it never offered (or the forced tool called twice under
/// one call id), and line 415 under a forced tool means the model ended its turn without a call.
/// The last is the Responses decoder finding no terminal object: the connection stopped before
/// the answer completed. The unit tests below produce each message from llm itself, so an llm
/// upgrade that changes one fails them.
fn pinned_refusal(error: &Error) -> Option<Pinned> {
    if error.code != ErrorCode::Protocol {
        return None;
    }
    match error.message.as_str() {
        "model output contains a duplicate call or unpublished tool"
        | "model called a tool other than the required named tool" => Some(Pinned::WrongTool),
        "model terminal reason contradicts its tool obligations" => Some(Pinned::NoToolCall),
        "the stream ended before the response reached a terminal state" => Some(Pinned::CutOff),
        _ => None,
    }
}

/// A sink that drops the streamed events: `call_tool` reads only the finished outcome.
struct Discard;

impl StreamSink for Discard {
    fn emit(&mut self, _event: StreamEvent) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async { Ok(()) })
    }
}

#[cfg(test)]
mod tests {
    use super::{ARGUMENTS_NOT_AN_OBJECT, ModelError, Pinned, classify, pinned_refusal};
    use llm_core::{
        CallId, Error, Id, Item, Protocol, Provenance, StopReason, ToolCall, ToolChoice, ToolName,
        ToolSpec, TurnObservation, TurnOutcome, TurnRequest,
    };
    use serde_json::json;

    fn provenance() -> Provenance {
        let id = |value: &str| Id::new(value).expect("fixture identifier");
        Provenance {
            protocol: Protocol::Responses,
            provider: id("fixture"),
            account: id("fixture"),
            endpoint: id("fixture"),
            model: id("fixture-model"),
            binding_revision: id("rev-1"),
        }
    }

    fn tool(name: &str) -> ToolSpec {
        ToolSpec {
            name: ToolName::new(name).expect("tool name"),
            description: String::new(),
            input_schema: json!({"type": "object"}),
        }
    }

    /// A request forcing `forced`, publishing `published`.
    fn request(forced: &str, published: &[&str]) -> TurnRequest {
        let mut request = TurnRequest::new("fixture-model", vec![Item::user("hi")]);
        request.tools = published.iter().map(|name| tool(name)).collect();
        request.tool_choice = ToolChoice::Named(ToolName::new(forced).expect("tool name"));
        request
    }

    fn call(name: &str) -> Item {
        Item::ToolCall(ToolCall {
            call_id: CallId::new("call_1").expect("call id"),
            name: ToolName::new(name).expect("tool name"),
            arguments: json!({}),
        })
    }

    /// The error llm itself raises for `items` and `stop_reason` answering `request`.
    fn refusal(request: &TurnRequest, items: Vec<Item>, stop_reason: StopReason) -> Error {
        let mut observation = TurnObservation::new(provenance());
        observation.final_usage = true;
        TurnOutcome {
            stop_reason,
            items,
            observation,
        }
        .validate_for(request, &provenance())
        .expect_err("llm refuses this answer")
    }

    #[test]
    fn each_forced_tool_refusal_llm_raises_is_classified() {
        // turn.rs:383, a tool the request never published.
        let error = refusal(
            &request("forced", &["forced"]),
            vec![call("other")],
            StopReason::ToolCalls,
        );
        assert!(pinned_refusal(&error) == Some(Pinned::WrongTool), "{error}");
        // turn.rs:390, a published tool that is not the forced one.
        let error = refusal(
            &request("forced", &["forced", "other"]),
            vec![call("other")],
            StopReason::ToolCalls,
        );
        assert!(pinned_refusal(&error) == Some(Pinned::WrongTool), "{error}");
        // turn.rs:415, the turn ended without a call.
        let error = refusal(
            &request("forced", &["forced"]),
            vec![Item::assistant("no call")],
            StopReason::EndTurn,
        );
        assert!(
            pinned_refusal(&error) == Some(Pinned::NoToolCall),
            "{error}"
        );
        // turn.rs:415, a tool-call stop with no call in it.
        let error = refusal(
            &request("forced", &["forced"]),
            vec![Item::assistant("no call")],
            StopReason::ToolCalls,
        );
        assert!(
            pinned_refusal(&error) == Some(Pinned::NoToolCall),
            "{error}"
        );
    }

    #[test]
    fn other_protocol_refusals_are_not_tool_refusals() {
        for error in [
            Error::protocol("successful outcome lacks terminal evidence"),
            Error::invalid("model called a tool other than the required named tool"),
        ] {
            assert_eq!(pinned_refusal(&error), None, "{error}");
        }
    }

    /// What llm's Responses decoder makes of `payloads`.
    fn decoded(payloads: &[serde_json::Value]) -> Result<TurnOutcome, Error> {
        let binding = llm_responses::Binding::new(
            provenance(),
            Id::new("fixture-model").expect("fixture identifier"),
        );
        llm_responses::decode_stream(&binding, payloads).result
    }

    /// stream.rs:328, a stream that stops before its terminal event: a transport failure.
    #[test]
    fn a_stream_without_its_terminal_event_is_classified_as_transport() {
        let error = decoded(&[json!({
            "type": "response.created",
            "response": {"id": "resp_1", "status": "in_progress"}
        })])
        .expect_err("no terminal event, no turn");
        assert_eq!(pinned_refusal(&error), Some(Pinned::CutOff), "{error}");
        assert!(
            matches!(classify(error), ModelError::Transport(_)),
            "a cut-off stream is a transport failure"
        );
    }

    /// stream.rs:512, the refusal `call_tool` repeats for a recorded model's non-object arguments.
    #[test]
    fn the_non_object_arguments_refusal_is_the_one_llm_raises() {
        let call = json!({
            "type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "forced",
            "arguments": "\"not an object\"", "status": "completed"
        });
        let error = decoded(&[
            json!({"type": "response.created", "response": {"id": "resp_1", "status": "in_progress"}}),
            json!({"type": "response.output_item.done", "output_index": 0, "item": call}),
            json!({"type": "response.completed", "response": {
                "id": "resp_1", "model": "fixture-model", "status": "completed", "output": [call],
                "usage": {"input_tokens": 1, "output_tokens": 1}}}),
        ])
        .expect_err("llm refuses non-object arguments");
        assert_eq!(error.message, ARGUMENTS_NOT_AN_OBJECT, "{error}");
        assert!(
            matches!(classify(error), ModelError::Model(_)),
            "not a pinned refusal"
        );
    }
}
