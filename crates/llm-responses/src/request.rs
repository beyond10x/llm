//! The request body, in both directions, from one pinned description of the subset.

use std::collections::BTreeSet;

use llm_core::{
    Advisory, CallId, Controls, Error, Hints, Item, Protocol, ReasoningSummary, Sampling, ToolCall,
    ToolChoice, ToolName, ToolSpec, TurnRequest, Unforwarded, Verbosity, field_path, wire_effort,
};
use serde_json::{Map, Value, json};

use crate::{Binding, Conversation};

/// The character class this wire publishes a tool name in.
///
/// Read off a live 400 rather than a specification, and recorded in `beyond10x/harness`
/// `crates/harness-responses/src/project.rs`: on 2026-08-23 a toolset named `workspace.read`
/// was answered with `Invalid 'tools[0].name': string does not match pattern. Expected a string
/// that matches the pattern '^[a-zA-Z0-9_-]+$'`. It is refused here rather than in `llm-core`,
/// because it is one provider's restriction and a neutral identifier that enforced it would be
/// shaped by one vendor.
pub const TOOL_NAME_PATTERN: &str = "^[a-zA-Z0-9_-]+$";

/// The `input` entry types ingress carries as unattributed continuation state.
///
/// Only what the **model** mints and a client replays: `reasoning` is the one this projection's
/// stream decoder models as continuation state (`stream.rs`, `output_item`). Every other entry
/// outside the four modelled shapes is the client's own content — a hosted-tool output, a
/// reference to provider-stored state — and carrying it would let a bound copy skip every check a
/// modelled entry gets, so ingress refuses it by name instead.
pub const CARRIED_ENTRY_TYPES: &[&str] = &["reasoning"];

/// The only `include` values this projection sends or accepts.
///
/// `reasoning.encrypted_content` is what carries a model's reasoning across a tool round trip
/// under `store: false`. vLLM v0.27.1 accepts it and returns `encrypted_content: null`, so the
/// reasoning arrives as a plain output item instead; both are handled, neither is required.
pub const INCLUDE: &[&str] = &["reasoning.encrypted_content"];

/// Every top-level body field inside the pinned subset. Ingress refuses anything else by name.
///
/// `client_metadata` and `instructions` are read and never written: the first is client telemetry
/// whose value is not kept, the second is written back as the developer message at the head of
/// `input`.
pub const ACCEPTED_BODY_FIELDS: &[&str] = &[
    "client_metadata",
    "include",
    "input",
    "instructions",
    "max_output_tokens",
    "model",
    "parallel_tool_calls",
    "prompt_cache_key",
    "reasoning",
    "store",
    "stream",
    "temperature",
    "text",
    "tool_choice",
    "tools",
    "top_p",
];

/// The tool types other than `function` that ingress refuses by name.
///
/// Hosted and custom tools a Responses client publishes. The name in the diagnostic is copied from
/// this list, never from the request.
pub const REFUSED_TOOL_TYPES: &[&str] = &[
    "web_search",
    "custom",
    "namespace",
    "tool_search",
    "local_shell",
    "file_search",
    "computer_use_preview",
    "mcp",
    "image_generation",
    "code_interpreter",
];

/// The advisory hints this projection sends: `docs/design.md`, "Advisory hints".
const CARRIED: &[Advisory] = &[
    Advisory::PromptCacheKey,
    Advisory::Verbosity,
    Advisory::ReasoningSummary,
];

/// The hints `request` carries that this wire does not send, in the design's order.
///
/// A gateway can report them before it forwards anything; [`crate::ResponsesClient`] warns
/// `advisory-omitted` naming them once a turn has succeeded.
pub fn omitted_advisories(request: &TurnRequest) -> Vec<Advisory> {
    request.controls.hints.omitted_by(CARRIED)
}

/// The refusal for a key no rule names, at `at`.
fn outside(at: &str, key: &str) -> Error {
    Error::unsupported(format!(
        "request body carries a field outside the pinned subset: {}",
        field_path(at, key)
    ))
}

/// Refuses the first key of `object` that `keys` does not name.
fn closed(object: &Map<String, Value>, at: &str, keys: &[&str]) -> Result<(), Error> {
    match object.keys().find(|key| !keys.contains(&key.as_str())) {
        Some(key) => Err(outside(at, key)),
        None => Ok(()),
    }
}

/// Projects one neutral turn into the request body this wire accepts.
///
/// Absent stays absent: a sampling field nobody set is one the provider decides, and writing its
/// default here would turn that decision into this crate's without anyone making it.
///
/// The effort goes through [`llm_core::wire_effort`]: `max` is sent as `xhigh` and disabled
/// thinking as `none`. `parallel_tool_calls`, `prompt_cache_key`, `text.verbosity` and
/// `reasoning.summary` are written when the turn sets them; the other hints are omitted and
/// listed by [`omitted_advisories`].
///
/// # Errors
/// Refuses a binding of another protocol, a request the neutral contract already refuses, a
/// request addressed to a different model, a tool name this wire cannot publish, a thinking
/// budget and disabled thinking beside an effort, opaque continuation state belonging to any
/// other binding, and unattributed opaque state no caller has bound, with
/// [`Item::UNATTRIBUTED_REFUSAL`].
pub fn project_request(binding: &Binding, request: &TurnRequest) -> Result<Value, Error> {
    binding.validate()?;
    request.validate()?;
    if request.model != binding.provenance().model.as_str() {
        return Err(Error::invalid(
            "request model differs from the selected binding",
        ));
    }
    check_tool_names(&request.tools)?;
    let effort = wire_effort(Protocol::Responses, request)?;

    let mut input = Vec::with_capacity(request.items.len() + 1);
    if !request.instructions.is_empty() {
        input.push(message("developer", "input_text", &request.instructions));
    }
    for item in &request.items {
        input.push(item_to_input(binding, item)?);
    }

    let mut body = Map::new();
    body.insert("model".to_owned(), json!(binding.upstream_model().as_str()));
    body.insert("input".to_owned(), Value::Array(input));
    body.insert(
        "tools".to_owned(),
        Value::Array(request.tools.iter().map(tool_to_wire).collect()),
    );
    body.insert("stream".to_owned(), json!(true));
    // Nothing is retained provider-side: the conversation is the caller's, replayed whole.
    body.insert("store".to_owned(), json!(false));
    body.insert("include".to_owned(), json!(INCLUDE));
    if let Some(limit) = request.max_output_tokens {
        body.insert("max_output_tokens".to_owned(), json!(limit));
    }
    if let Some(temperature) = request.sampling.temperature {
        body.insert("temperature".to_owned(), json!(temperature));
    }
    if let Some(top_p) = request.sampling.top_p {
        body.insert("top_p".to_owned(), json!(top_p));
    }
    let hints = &request.controls.hints;
    let mut reasoning = Map::new();
    if let Some(effort) = effort {
        reasoning.insert("effort".to_owned(), json!(effort));
    }
    if let Some(summary) = hints.reasoning_summary {
        reasoning.insert("summary".to_owned(), json!(summary_name(summary)));
    }
    if !reasoning.is_empty() {
        body.insert("reasoning".to_owned(), Value::Object(reasoning));
    }
    if let Some(choice) = tool_choice_to_wire(&request.tool_choice) {
        body.insert("tool_choice".to_owned(), choice);
    }
    if let Some(parallel) = request.controls.parallel_tool_calls {
        body.insert("parallel_tool_calls".to_owned(), json!(parallel));
    }
    if let Some(key) = &hints.prompt_cache_key {
        body.insert("prompt_cache_key".to_owned(), json!(key));
    }
    if let Some(verbosity) = hints.verbosity {
        body.insert(
            "text".to_owned(),
            json!({"verbosity": verbosity_name(verbosity)}),
        );
    }
    Ok(Value::Object(body))
}

const fn summary_name(summary: ReasoningSummary) -> &'static str {
    match summary {
        ReasoningSummary::Auto => "auto",
        ReasoningSummary::Concise => "concise",
        ReasoningSummary::Detailed => "detailed",
    }
}

const fn verbosity_name(verbosity: Verbosity) -> &'static str {
    match verbosity {
        Verbosity::Low => "low",
        Verbosity::Medium => "medium",
        Verbosity::High => "high",
    }
}

/// The exact bytes of the request body the client sends for one turn.
///
/// The client sends what this returns and serialises the body nowhere else, so a caller or a test
/// holding these bytes holds the request. Without a conversation they are [`project_request`]'s
/// value, compactly encoded; with one, that value plus `prompt_cache_key` equal to the
/// conversation's identifier.
///
/// # Errors
/// Refuses exactly what [`project_request`] refuses, a turn whose own `prompt_cache_key` differs
/// from the conversation's identifier, and a body that cannot be encoded.
pub fn encode_request(
    binding: &Binding,
    request: &TurnRequest,
    conversation: Option<&Conversation>,
) -> Result<Vec<u8>, Error> {
    let mut body = project_request(binding, request)?;
    if let (Some(conversation), Value::Object(fields)) = (conversation, &mut body) {
        // Two keys for one request: neither may silently win.
        if request
            .controls
            .hints
            .prompt_cache_key
            .as_deref()
            .is_some_and(|key| key != conversation.id().as_str())
        {
            return Err(Error::unsupported(
                "the turn's prompt_cache_key differs from the client's conversation",
            ));
        }
        fields.insert(
            "prompt_cache_key".to_owned(),
            json!(conversation.id().as_str()),
        );
    }
    serde_json::to_vec(&body).map_err(|_| Error::invalid("the projected request cannot be encoded"))
}

/// Reads one wire request body back into a neutral turn, for a gateway ingress surface.
///
/// This is the same contract read the other way, not a second one. Every field
/// [`project_request`] writes is read here, and a body carrying anything outside the subset is
/// refused rather than translated with the excess silently dropped.
///
/// Every object this reads is closed: the top level, each modelled `input` item, each content
/// part, an object `tool_choice`, each published tool, `reasoning` and `text` name their keys, and
/// any other key is refused `unsupported` naming its path (`llm_core::field_path`).
///
/// A few fields are read one way, by `docs/design.md` "Client request fields": the top-level
/// `instructions` and the `input_text` parts of the developer messages that open `input`,
/// joined by a line feed, are the instructions, written back as one developer message; an item
/// `id` and `client_metadata` are unforwarded hints whose values are not kept; a
/// `function_call_output` string that is not the pinned envelope is a successful result carrying
/// that string, written back inside the envelope.
///
/// # Errors
/// Refuses a binding of another protocol, a body that is not an object, a key outside the subset
/// at any level, a body addressed to another upstream model, non-streaming or provider-stored
/// conversations and `item_reference` entries pointing into them, content this version does not
/// carry, a developer message after the conversation began, an `input` entry that names no type,
/// a client-authored entry this version does not model (a hosted-tool output, for example), a
/// tool of another type than `function`, a field of the wrong type, and any request the neutral
/// contract itself refuses.
///
/// An entry of a type in [`CARRIED_ENTRY_TYPES`] — continuation state the model mints — is not
/// refused: it is carried as [`Item::UnattributedOpaque`], JSON-equal to what arrived, and
/// [`project_request`] refuses it until a caller binds it.
pub fn ingest_request(binding: &Binding, body: &Value) -> Result<TurnRequest, Error> {
    binding.validate()?;
    let object = body
        .as_object()
        .ok_or_else(|| Error::invalid("request body is not an object"))?;
    closed(object, "", ACCEPTED_BODY_FIELDS)?;
    let model =
        string(object.get("model")).ok_or_else(|| Error::invalid("request body names no model"))?;
    if model != binding.upstream_model().as_str() {
        return Err(Error::invalid(
            "request body names a model other than the selected binding",
        ));
    }
    check_fixed(object, "stream", &json!(true))?;
    check_fixed(object, "store", &json!(false))?;
    check_fixed(object, "include", &json!(INCLUDE))?;

    let mut unforwarded = BTreeSet::new();
    let mut instructions = match object.get("instructions") {
        None => Vec::new(),
        Some(Value::String(text)) => vec![text.clone()],
        Some(_) => return Err(Error::invalid("`instructions` is not a string")),
    };
    let mut items = Vec::new();
    if let Some(input) = object.get("input") {
        let entries = input
            .as_array()
            .ok_or_else(|| Error::invalid("`input` is not an array"))?;
        for entry in entries {
            if is_developer_message(entry) {
                // The neutral turn has no item that places an instruction between two turns.
                if !items.is_empty() {
                    return Err(Error::unsupported(
                        "a standing instruction is carried once, at the head of `input`",
                    ));
                }
                instructions.extend(developer_parts(entry, &mut unforwarded)?);
                continue;
            }
            items.push(input_to_item(entry, &mut unforwarded)?);
        }
    }

    let mut tools = Vec::new();
    if let Some(published) = object.get("tools") {
        let published = published
            .as_array()
            .ok_or_else(|| Error::invalid("`tools` is not an array"))?;
        for tool in published {
            tools.push(wire_to_tool(tool)?);
        }
    }

    let (reasoning_effort, reasoning_summary) = reasoning(object.get("reasoning"))?;
    match object.get("client_metadata") {
        None => {}
        // Client telemetry with no model semantics: its presence is reported, its value dropped.
        Some(Value::Object(_)) => {
            unforwarded.insert(Unforwarded::ClientMetadata);
        }
        Some(_) => return Err(Error::invalid("`client_metadata` is not an object")),
    }
    let controls = Controls {
        thinking: None,
        parallel_tool_calls: match object.get("parallel_tool_calls") {
            None => None,
            Some(Value::Bool(parallel)) => Some(*parallel),
            Some(_) => return Err(Error::invalid("`parallel_tool_calls` is not a boolean")),
        },
        hints: Hints {
            prompt_cache: None,
            prompt_cache_key: match object.get("prompt_cache_key") {
                None => None,
                Some(Value::String(key)) => Some(key.clone()),
                Some(_) => return Err(Error::invalid("`prompt_cache_key` is not a string")),
            },
            verbosity: verbosity(object.get("text"))?,
            reasoning_summary,
            unforwarded,
        },
    };

    let request = TurnRequest {
        model: binding.provenance().model.as_str().to_owned(),
        instructions: instructions.join("\n"),
        items,
        tools,
        max_output_tokens: match object.get("max_output_tokens") {
            None | Some(Value::Null) => None,
            Some(value) => Some(
                value
                    .as_u64()
                    .ok_or_else(|| Error::invalid("`max_output_tokens` is not a count"))?,
            ),
        },
        sampling: sampling(object, reasoning_effort)?,
        tool_choice: wire_to_tool_choice(object.get("tool_choice"))?,
        controls,
    };
    // A named choice that is not published, a tool result answering no call and every other
    // structural rule are `TurnRequest::validate`'s, not this crate's to restate.
    request.validate()?;
    // **Every pre-flight check egress applies, ingress applies too.** A gateway that accepts a
    // request its own outgoing side then refuses to forward has agreed to something it cannot
    // do, and the caller finds out one hop later. `project_request` applies exactly five checks
    // — the binding's protocol, `TurnRequest::validate`, the model, this one, and the opaque
    // coordinates — and each has its counterpart above. The sixth, `wire_effort`, refuses only a
    // thinking control, which this ingress never reads. The one deliberate exception is state
    // ingress carries unattributed: egress refuses it by name until the caller binds it, because
    // that binding is a decision only the caller can make.
    check_tool_names(&request.tools)?;
    Ok(request)
}

/// A field this projection writes with a fixed value must arrive carrying exactly that value.
///
/// **Absence is a different value, not agreement.** On this wire an omitted `stream` is a
/// non-streaming request, an omitted `store` is a conversation the provider keeps, and an
/// omitted `include` asks for no encrypted reasoning content. The neutral turn has nowhere to
/// record any of those choices, so accepting the omission and re-projecting the pinned value
/// answers a request nobody made — the same reasoning this crate applies on the other side when
/// it omits `tool_choice` rather than sending `"auto"`, so that the provider's default stays the
/// provider's.
///
/// Equality is over the whole value, so `include: []` is refused for the same reason
/// `include: ["something_else"]` is. A membership test would read the empty array as agreement,
/// because `all` over no elements is true — the vacuous-truth trap `check_tool_names` guards
/// against below.
fn check_fixed(object: &Map<String, Value>, field: &str, pinned: &Value) -> Result<(), Error> {
    if object.get(field) == Some(pinned) {
        return Ok(());
    }
    Err(Error::unsupported(
        "a field this projection fixes is absent or carries a different value",
    ))
}

fn sampling(
    object: &Map<String, Value>,
    reasoning_effort: Option<String>,
) -> Result<Sampling, Error> {
    let number = |value: Option<&Value>, field: &str| -> Result<Option<f64>, Error> {
        match value {
            None | Some(Value::Null) => Ok(None),
            Some(value) => value
                .as_f64()
                .map(Some)
                .ok_or_else(|| Error::invalid(format!("`{field}` is not a number"))),
        }
    };
    Ok(Sampling {
        temperature: number(object.get("temperature"), "temperature")?,
        top_p: number(object.get("top_p"), "top_p")?,
        reasoning_effort,
    })
}

/// `reasoning`: the effort, any name, and the summary hint. A `null` member is absent.
fn reasoning(value: Option<&Value>) -> Result<(Option<String>, Option<ReasoningSummary>), Error> {
    let reasoning = match value {
        None | Some(Value::Null) => return Ok((None, None)),
        Some(Value::Object(reasoning)) => reasoning,
        Some(_) => return Err(Error::invalid("`reasoning` is not an object")),
    };
    closed(reasoning, "reasoning", &["effort", "summary"])?;
    let effort = match reasoning.get("effort") {
        None | Some(Value::Null) => None,
        Some(Value::String(effort)) => Some(effort.clone()),
        Some(_) => return Err(Error::invalid("`reasoning.effort` is not a string")),
    };
    let summary = match reasoning.get("summary") {
        None | Some(Value::Null) => None,
        Some(Value::String(summary)) => Some(match summary.as_str() {
            "auto" => ReasoningSummary::Auto,
            "concise" => ReasoningSummary::Concise,
            "detailed" => ReasoningSummary::Detailed,
            _ => {
                return Err(Error::unsupported(
                    "`reasoning.summary` is outside the pinned subset",
                ));
            }
        }),
        Some(_) => return Err(Error::invalid("`reasoning.summary` is not a string")),
    };
    Ok((effort, summary))
}

/// `text`: only `verbosity`. Structured output (`text.format`) is outside this design.
fn verbosity(value: Option<&Value>) -> Result<Option<Verbosity>, Error> {
    let text = match value {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Object(text)) => text,
        Some(_) => return Err(Error::invalid("`text` is not an object")),
    };
    closed(text, "text", &["verbosity"])?;
    match text.get("verbosity") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(verbosity)) => match verbosity.as_str() {
            "low" => Ok(Some(Verbosity::Low)),
            "medium" => Ok(Some(Verbosity::Medium)),
            "high" => Ok(Some(Verbosity::High)),
            _ => Err(Error::unsupported(
                "`text.verbosity` is outside the pinned subset",
            )),
        },
        Some(_) => Err(Error::invalid("`text.verbosity` is not a string")),
    }
}

fn check_tool_names(tools: &[ToolSpec]) -> Result<(), Error> {
    for tool in tools {
        let name = tool.name.as_str();
        // `+`, not `*`: the documented class matches one character or more, and `all` over no
        // bytes is true. `ToolName` refuses an empty name today, so this is the pattern being
        // read as written rather than a reachable defect — and a later widening there is exactly
        // what would make it one.
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        {
            // Fixed diagnostics: the offending name is the caller's own input, but an error that
            // echoes request content is one that eventually echoes a prompt.
            return Err(Error::unsupported(
                "a published tool name is outside the character class this wire accepts",
            ));
        }
    }
    Ok(())
}

fn message(role: &str, part: &str, text: &str) -> Value {
    json!({"type": "message", "role": role, "content": [{"type": part, "text": text}]})
}

fn tool_to_wire(tool: &ToolSpec) -> Value {
    json!({
        "type": "function",
        "name": tool.name.as_str(),
        "description": tool.description,
        "parameters": tool.input_schema,
        "strict": false,
    })
}

fn wire_to_tool(value: &Value) -> Result<ToolSpec, Error> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::invalid("a published tool is not an object"))?;
    match string(object.get("type")) {
        Some("function") => {}
        kind => {
            // The name is copied from the fixed list, never from the request.
            return Err(Error::unsupported(
                match REFUSED_TOOL_TYPES
                    .iter()
                    .find(|known| Some(**known) == kind)
                {
                    Some(known) => {
                        format!("a published tool of type `{known}` is outside the pinned subset")
                    }
                    None => "only function tools are inside the pinned subset".to_owned(),
                },
            ));
        }
    }
    closed(
        object,
        "tools[]",
        &["type", "name", "description", "parameters", "strict"],
    )?;
    // `strict` is the fourth fixed field, beside `stream`, `store` and `include`, and obeys the
    // same rule: present and exactly `false`. Absent is refused too, because the reprojection
    // writes the key, and a body that comes back carrying a field it did not send is a body the
    // client did not write.
    check_fixed(object, "strict", &json!(false))?;
    let name =
        string(object.get("name")).ok_or_else(|| Error::invalid("a published tool has no name"))?;
    // A schema is required rather than invented, as on the chat ingress: an omitted one would be
    // published to the model as a null the operator never declared. A present `null` is refused
    // by the turn's own schema check, as it was before.
    let input_schema = object
        .get("parameters")
        .cloned()
        .ok_or_else(|| Error::invalid("a published tool omits its parameters schema"))?;
    Ok(ToolSpec {
        name: ToolName::new(name).map_err(|_| Error::invalid("invalid tool name"))?,
        description: string(object.get("description"))
            .unwrap_or_default()
            .to_owned(),
        input_schema,
    })
}

fn tool_choice_to_wire(choice: &ToolChoice) -> Option<Value> {
    match choice {
        // Absent, not `"auto"`: the model choosing is the provider's default, and sending the
        // word would make that choice this crate's.
        ToolChoice::Auto => None,
        ToolChoice::Required => Some(json!("required")),
        ToolChoice::Named(name) => Some(json!({"type": "function", "name": name.as_str()})),
    }
}

fn wire_to_tool_choice(value: Option<&Value>) -> Result<ToolChoice, Error> {
    match value {
        None | Some(Value::Null) => Ok(ToolChoice::Auto),
        Some(Value::String(word)) if word == "auto" => Ok(ToolChoice::Auto),
        Some(Value::String(word)) if word == "required" => Ok(ToolChoice::Required),
        Some(Value::Object(named)) if string(named.get("type")) == Some("function") => {
            closed(named, "tool_choice", &["type", "name"])?;
            let name = string(named.get("name"))
                .ok_or_else(|| Error::invalid("a named tool choice has no name"))?;
            Ok(ToolChoice::Named(
                ToolName::new(name).map_err(|_| Error::invalid("invalid tool name"))?,
            ))
        }
        Some(_) => Err(Error::unsupported(
            "`tool_choice` is outside the pinned subset",
        )),
    }
}

fn item_to_input(binding: &Binding, item: &Item) -> Result<Value, Error> {
    Ok(match item {
        Item::UserText { text } => message("user", "input_text", text),
        Item::AssistantText { text } => message("assistant", "output_text", text),
        Item::ToolCall(call) => json!({
            "type": "function_call",
            "call_id": call.call_id.as_str(),
            "name": call.name.as_str(),
            "arguments": serde_json::to_string(&call.arguments)
                .map_err(|_| Error::invalid("tool arguments are not encodable"))?,
        }),
        Item::ToolResult {
            call_id,
            output,
            failed,
        } => json!({
            "type": "function_call_output",
            "call_id": call_id.as_str(),
            "output": tool_result_envelope(output, *failed)?,
        }),
        // Verbatim, after all six coordinates match. Reinterpreting it would defeat the point of
        // keeping it opaque, and sending it to another binding is what the check below prevents.
        Item::Opaque {
            provenance,
            payload,
        } => {
            if provenance != binding.provenance() {
                return Err(Error::unsupported(
                    "opaque state belongs to a different serving binding",
                ));
            }
            payload.clone()
        }
        // Carried by ingress, bound by nobody: sending it would make the origin this crate
        // could not observe into one it asserts.
        Item::UnattributedOpaque { .. } => {
            return Err(Error::unsupported(Item::UNATTRIBUTED_REFUSAL));
        }
    })
}

/// The pinned envelope for a tool result.
///
/// This wire's `function_call_output` carries one string and no failure channel, so the neutral
/// `failed` flag has to travel inside it. It is applied to **every** result, successful or not:
/// an envelope used only for failures is indistinguishable from a successful result that happens
/// to be shaped like one, and a round trip that can flip `failed` is a silent corruption of the
/// conversation rather than a translation of it.
fn tool_result_envelope(output: &Value, failed: bool) -> Result<Value, Error> {
    serde_json::to_string(&json!({"ok": !failed, "output": output}))
        .map(Value::String)
        .map_err(|_| Error::invalid("tool result is not encodable"))
}

/// The keys a `message` item carries.
const MESSAGE_KEYS: &[&str] = &["type", "id", "role", "content"];

/// An item's own `id`: a client-assigned identifier no provider that stores nothing can use.
/// Its presence is the `input-item-ids` hint; its value is not kept.
fn item_id(
    entry: &Map<String, Value>,
    unforwarded: &mut BTreeSet<Unforwarded>,
) -> Result<(), Error> {
    match entry.get("id") {
        None => Ok(()),
        Some(Value::String(_)) => {
            unforwarded.insert(Unforwarded::InputItemIds);
            Ok(())
        }
        Some(_) => Err(Error::invalid("an input item's `id` is not a string")),
    }
}

/// The text of every part of one developer message, in order. An empty text is still a part.
fn developer_parts(
    entry: &Value,
    unforwarded: &mut BTreeSet<Unforwarded>,
) -> Result<Vec<String>, Error> {
    let fields = entry
        .as_object()
        .ok_or_else(|| Error::invalid("a message is not an object"))?;
    closed(fields, "input[]", MESSAGE_KEYS)?;
    item_id(fields, unforwarded)?;
    let parts = content_parts(fields)?;
    // A message with no parts would vanish from the body without a trace in the turn.
    if parts.is_empty() {
        return Err(Error::unsupported(
            "a message carries content this version cannot represent part for part",
        ));
    }
    parts
        .iter()
        .map(|part| part_text(part, "input_text"))
        .collect()
}

fn input_to_item(entry: &Value, unforwarded: &mut BTreeSet<Unforwarded>) -> Result<Item, Error> {
    let Some(fields) = entry.as_object() else {
        return Err(Error::unsupported(
            "an input entry names no type this version can carry",
        ));
    };
    match string(entry.get("type")) {
        Some("message") => {
            let part = match string(entry.get("role")) {
                Some("user") => "input_text",
                Some("assistant") => "output_text",
                _ => {
                    return Err(Error::unsupported(
                        "a message role outside the pinned subset",
                    ));
                }
            };
            closed(fields, "input[]", MESSAGE_KEYS)?;
            item_id(fields, unforwarded)?;
            let text = content_text(fields, part)?;
            Ok(if part == "input_text" {
                Item::user(text)
            } else {
                Item::assistant(text)
            })
        }
        Some("function_call") => {
            closed(
                fields,
                "input[]",
                &["type", "id", "call_id", "name", "arguments"],
            )?;
            item_id(fields, unforwarded)?;
            let raw = string(entry.get("arguments"))
                .ok_or_else(|| Error::invalid("a function call has no `arguments`"))?;
            let arguments: Value = serde_json::from_str(raw)
                .map_err(|_| Error::protocol("function call arguments are not JSON"))?;
            if !arguments.is_object() {
                return Err(Error::protocol(
                    "function call arguments are not a JSON object",
                ));
            }
            Ok(Item::ToolCall(ToolCall {
                call_id: call_id(entry)?,
                name: ToolName::new(
                    string(entry.get("name"))
                        .ok_or_else(|| Error::invalid("a function call has no name"))?,
                )
                .map_err(|_| Error::invalid("invalid tool name"))?,
                arguments,
            }))
        }
        Some("function_call_output") => {
            closed(fields, "input[]", &["type", "id", "call_id", "output"])?;
            item_id(fields, unforwarded)?;
            // An array of content items is a shape this version does not carry.
            let raw = string(entry.get("output"))
                .ok_or_else(|| Error::invalid("a tool result's `output` is not a string"))?;
            let (output, failed) = read_tool_result(raw);
            Ok(Item::ToolResult {
                call_id: call_id(entry)?,
                output,
                failed,
            })
        }
        // **Carried, because this side cannot attribute it** — but only what the model mints.
        //
        // A `reasoning` entry is opaque continuation state, and the wire body carries no
        // provenance. Stamping it with the binding doing the reading would be this crate
        // asserting an origin it never observed — and it launders: a payload minted under one
        // binding revision, replayed by a client after the endpoint was repointed, would come
        // back out addressed to the new one.
        //
        // So it is carried as `Item::UnattributedOpaque`: the entry as a JSON value (equal, not
        // byte-identical: keys re-serialize sorted) and the protocol it was read from, and
        // nothing else. `project_request` refuses it by name until a caller binds it; this crate
        // never does. The stream decoder still binds what it decodes, and that is not the same
        // act — it watched this binding produce it.
        Some(kind) if CARRIED_ENTRY_TYPES.contains(&kind) => Ok(Item::UnattributedOpaque {
            protocol: Protocol::Responses,
            payload: entry.clone(),
        }),
        // A pointer to state the provider kept. This projection sends `store: false` and replays
        // the conversation whole, so there is nothing on this side it could point at.
        Some("item_reference") => Err(Error::unsupported(
            "an input entry refers to state the provider stored, which this projection never keeps",
        )),
        Some("") | None => Err(Error::unsupported(
            "an input entry names no type this version can carry",
        )),
        // Client-authored content this version does not model, such as a hosted-tool output.
        // It is not continuation state, and a bound copy would skip every check a modelled
        // entry gets, so it is refused as at the base rather than carried.
        Some(_) => Err(Error::unsupported(
            "an input entry is neither modelled nor continuation state the model mints",
        )),
    }
}

/// A result string read as the pinned envelope when it is exactly one — an object of exactly a
/// boolean `ok` and an `output` — and otherwise as a successful result carrying that string.
///
/// Codex sends a command's output as a plain string. Egress writes it back inside the envelope,
/// so the body changes and the turn does not.
fn read_tool_result(raw: &str) -> (Value, bool) {
    if let Ok(Value::Object(envelope)) = serde_json::from_str::<Value>(raw)
        && envelope.len() == 2
        && let (Some(Value::Bool(ok)), Some(output)) = (envelope.get("ok"), envelope.get("output"))
    {
        return (output.clone(), !ok);
    }
    (Value::String(raw.to_owned()), false)
}

fn is_developer_message(entry: &Value) -> bool {
    string(entry.get("type")) == Some("message") && string(entry.get("role")) == Some("developer")
}

fn call_id(entry: &Value) -> Result<CallId, Error> {
    CallId::new(
        string(entry.get("call_id")).ok_or_else(|| Error::invalid("an entry has no `call_id`"))?,
    )
    .map_err(|_| Error::invalid("invalid call id"))
}

/// The one text part a neutral message item can hold.
///
/// **Exactly one, not however many arrive.** `Item::UserText` holds a single string, so two
/// parts joined into it come back out as one part and the client is answered with a body it did
/// not send; zero parts come back out as one empty part. Egress writes exactly one part, so
/// requiring exactly one here is the symmetric rule rather than a new restriction. Image and
/// audio parts are outside this contract version and refused with the same code.
fn content_text(entry: &Map<String, Value>, part: &str) -> Result<String, Error> {
    let [only] = content_parts(entry)? else {
        return Err(Error::unsupported(
            "a message carries content this version cannot represent part for part",
        ));
    };
    part_text(only, part)
}

fn content_parts(entry: &Map<String, Value>) -> Result<&[Value], Error> {
    entry
        .get("content")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| Error::invalid("a message has no `content` array"))
}

/// One content part of type `kind`: `type` and `text`, and on an `output_text` part an empty
/// `annotations` array, which is what a replayed answer carries and loses nothing.
fn part_text(part: &Value, kind: &str) -> Result<String, Error> {
    let fields = part
        .as_object()
        .filter(|fields| string(fields.get("type")) == Some(kind))
        .ok_or_else(|| Error::unsupported("message content is outside the pinned subset"))?;
    for (key, value) in fields {
        match key.as_str() {
            "type" | "text" => {}
            "annotations"
                if kind == "output_text" && value.as_array().is_some_and(Vec::is_empty) => {}
            _ => return Err(outside("input[].content[]", key)),
        }
    }
    Ok(string(fields.get("text"))
        .ok_or_else(|| Error::invalid("a content part has no text"))?
        .to_owned())
}

fn string(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}
