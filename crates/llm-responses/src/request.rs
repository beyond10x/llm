//! The request body, in both directions, from one pinned description of the subset.

use llm_core::{
    CallId, Error, Item, Protocol, Sampling, ToolCall, ToolChoice, ToolName, ToolSpec, TurnRequest,
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

/// Every top-level body field inside the pinned subset. Ingress refuses anything else.
pub const ACCEPTED_BODY_FIELDS: &[&str] = &[
    "include",
    "input",
    "max_output_tokens",
    "model",
    "reasoning",
    "store",
    "stream",
    "temperature",
    "tool_choice",
    "tools",
    "top_p",
];

/// Projects one neutral turn into the request body this wire accepts.
///
/// Absent stays absent: a sampling field nobody set is one the provider decides, and writing its
/// default here would turn that decision into this crate's without anyone making it.
///
/// # Errors
/// Refuses a binding of another protocol, a request the neutral contract already refuses, a
/// request addressed to a different model, a tool name this wire cannot publish, opaque
/// continuation state belonging to any other binding, and unattributed opaque state no caller
/// has bound, with [`Item::UNATTRIBUTED_REFUSAL`].
pub fn project_request(binding: &Binding, request: &TurnRequest) -> Result<Value, Error> {
    binding.validate()?;
    request.validate()?;
    if request.model != binding.provenance().model.as_str() {
        return Err(Error::invalid(
            "request model differs from the selected binding",
        ));
    }
    check_tool_names(&request.tools)?;

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
    if let Some(effort) = &request.sampling.reasoning_effort {
        body.insert("reasoning".to_owned(), json!({"effort": effort}));
    }
    if let Some(choice) = tool_choice_to_wire(&request.tool_choice) {
        body.insert("tool_choice".to_owned(), choice);
    }
    Ok(Value::Object(body))
}

/// The exact bytes of the request body the client sends for one turn.
///
/// The client sends what this returns and serialises the body nowhere else, so a caller or a test
/// holding these bytes holds the request. Without a conversation they are [`project_request`]'s
/// value, compactly encoded; with one, that value plus `prompt_cache_key` equal to the
/// conversation's identifier.
///
/// # Errors
/// Refuses exactly what [`project_request`] refuses, and a body that cannot be encoded.
pub fn encode_request(
    binding: &Binding,
    request: &TurnRequest,
    conversation: Option<&Conversation>,
) -> Result<Vec<u8>, Error> {
    let mut body = project_request(binding, request)?;
    if let (Some(conversation), Value::Object(fields)) = (conversation, &mut body) {
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
/// # Errors
/// Refuses a binding of another protocol, a body that is not an object, a top-level field outside
/// [`ACCEPTED_BODY_FIELDS`], a body addressed to another upstream model, non-streaming or
/// provider-stored conversations and `item_reference` entries pointing into them, content this
/// version does not carry, an `input` entry that names no type, a client-authored entry this
/// version does not model (a hosted-tool output, for example), a tool result outside the pinned
/// envelope, and any request the neutral contract itself refuses.
///
/// An entry of a type in [`CARRIED_ENTRY_TYPES`] — continuation state the model mints — is not
/// refused: it is carried as [`Item::UnattributedOpaque`], JSON-equal to what arrived, and
/// [`project_request`] refuses it until a caller binds it.
pub fn ingest_request(binding: &Binding, body: &Value) -> Result<TurnRequest, Error> {
    binding.validate()?;
    let object = body
        .as_object()
        .ok_or_else(|| Error::invalid("request body is not an object"))?;
    for field in object.keys() {
        if !ACCEPTED_BODY_FIELDS.contains(&field.as_str()) {
            return Err(Error::unsupported(
                "request body carries a field outside the pinned subset",
            ));
        }
    }
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

    let mut instructions = String::new();
    let mut items = Vec::new();
    if let Some(input) = object.get("input") {
        let entries = input
            .as_array()
            .ok_or_else(|| Error::invalid("`input` is not an array"))?;
        for (index, entry) in entries.iter().enumerate() {
            if is_developer_message(entry) {
                if index != 0 {
                    return Err(Error::unsupported(
                        "a standing instruction is carried once, at the head of `input`",
                    ));
                }
                instructions = content_text(entry, "input_text")?;
                continue;
            }
            items.push(input_to_item(entry)?);
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

    let request = TurnRequest {
        model: binding.provenance().model.as_str().to_owned(),
        instructions,
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
        sampling: sampling(object)?,
        tool_choice: wire_to_tool_choice(object.get("tool_choice"))?,
    };
    // A named choice that is not published, a tool result answering no call and every other
    // structural rule are `TurnRequest::validate`'s, not this crate's to restate.
    request.validate()?;
    // **Every pre-flight check egress applies, ingress applies too.** A gateway that accepts a
    // request its own outgoing side then refuses to forward has agreed to something it cannot
    // do, and the caller finds out one hop later. `project_request` applies exactly five checks
    // — the binding's protocol, `TurnRequest::validate`, the model, this one, and the opaque
    // coordinates — and each has its counterpart above. The one deliberate exception is state
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

fn sampling(object: &Map<String, Value>) -> Result<Sampling, Error> {
    let number = |value: Option<&Value>, field: &str| -> Result<Option<f64>, Error> {
        match value {
            None | Some(Value::Null) => Ok(None),
            Some(value) => value
                .as_f64()
                .map(Some)
                .ok_or_else(|| Error::invalid(format!("`{field}` is not a number"))),
        }
    };
    let reasoning_effort = match object.get("reasoning") {
        None | Some(Value::Null) => None,
        Some(Value::Object(reasoning)) => {
            if reasoning.keys().any(|key| key != "effort") {
                return Err(Error::unsupported(
                    "`reasoning` carries a field outside the pinned subset",
                ));
            }
            string(reasoning.get("effort")).map(ToOwned::to_owned)
        }
        Some(_) => return Err(Error::invalid("`reasoning` is not an object")),
    };
    Ok(Sampling {
        temperature: number(object.get("temperature"), "temperature")?,
        top_p: number(object.get("top_p"), "top_p")?,
        reasoning_effort,
    })
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
    if string(object.get("type")) != Some("function") {
        return Err(Error::unsupported(
            "only function tools are inside the pinned subset",
        ));
    }
    if object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "type" | "name" | "description" | "parameters" | "strict"
        )
    }) {
        return Err(Error::unsupported(
            "a published tool carries a field outside the pinned subset",
        ));
    }
    // `strict` is the fourth fixed field, beside `stream`, `store` and `include`, and obeys the
    // same rule: present and exactly `false`. Absent is refused too, because the reprojection
    // writes the key, and a body that comes back carrying a field it did not send is a body the
    // client did not write.
    check_fixed(object, "strict", &json!(false))?;
    let name =
        string(object.get("name")).ok_or_else(|| Error::invalid("a published tool has no name"))?;
    Ok(ToolSpec {
        name: ToolName::new(name).map_err(|_| Error::invalid("invalid tool name"))?,
        description: string(object.get("description"))
            .unwrap_or_default()
            .to_owned(),
        input_schema: object.get("parameters").cloned().unwrap_or(Value::Null),
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

fn input_to_item(entry: &Value) -> Result<Item, Error> {
    match string(entry.get("type")) {
        Some("message") => match string(entry.get("role")) {
            Some("user") => Ok(Item::user(content_text(entry, "input_text")?)),
            Some("assistant") => Ok(Item::assistant(content_text(entry, "output_text")?)),
            _ => Err(Error::unsupported(
                "a message role outside the pinned subset",
            )),
        },
        Some("function_call") => {
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
            let raw = string(entry.get("output"))
                .ok_or_else(|| Error::invalid("a tool result has no `output`"))?;
            let envelope: Value = serde_json::from_str(raw)
                .map_err(|_| Error::unsupported("tool result is outside the pinned envelope"))?;
            let envelope = envelope
                .as_object()
                .ok_or_else(|| Error::unsupported("tool result is outside the pinned envelope"))?;
            let failed = match envelope.get("ok") {
                Some(Value::Bool(ok)) if envelope.len() == 2 && envelope.contains_key("output") => {
                    !ok
                }
                _ => {
                    return Err(Error::unsupported(
                        "tool result is outside the pinned envelope",
                    ));
                }
            };
            Ok(Item::ToolResult {
                call_id: call_id(entry)?,
                output: envelope.get("output").cloned().unwrap_or(Value::Null),
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
fn content_text(entry: &Value, part: &str) -> Result<String, Error> {
    let parts = entry
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::invalid("a message has no `content` array"))?;
    let [only] = parts.as_slice() else {
        return Err(Error::unsupported(
            "a message carries content this version cannot represent part for part",
        ));
    };
    if string(only.get("type")) != Some(part) {
        return Err(Error::unsupported(
            "message content is outside the pinned subset",
        ));
    }
    Ok(string(only.get("text"))
        .ok_or_else(|| Error::invalid("a content part has no text"))?
        .to_owned())
}

fn string(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}
