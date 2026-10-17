//! A Claude Code and a Codex first turn, each decoded by its own wire's ingress and projected onto
//! the other two wires (`story:client-turns-cross-wire`, `docs/design.md` "Client request
//! fields"). Every assertion reads a value from the projected body, so a projection that drops a
//! field fails here.
use llm_core::{Advisory, Id, TurnRequest};
use llm_providers::Binding;
use llm_routing::Catalog;
use serde_json::{Value, json};

/// Claude Code 2.1.293's first turn as llm-gateway authored it from published sources.
const CLAUDE_CODE: &str =
    include_str!("../../../crates/llm-messages/tests/fixtures/claude-code-first-turn.json");
/// Codex 0.160.0's first turn, captured against a loopback recorder and sanitized.
const CODEX: &str =
    include_str!("../../../crates/llm-responses/tests/fixtures/codex/first-turn.json");

/// One model, `code`, served on each of the three wires. Capabilities name `high`, `xhigh` and
/// `max`, so effort is never what refuses a projection here.
const CATALOG: &str = r#"
format = "llm.catalog/1"

[[providers]]
id = "lab"
category = "self-hosted"

[[accounts]]
id = "local"
provider_id = "lab"
auth_kind = "anonymous"
billing_kind = "self-hosted"

[[endpoints]]
id = "local"
account_id = "local"
base_url = "http://127.0.0.1:8000/v1"

[[models]]
id = "code"
upstream_name = "served-code"

[[serving_models]]
id = "chat"
endpoint_id = "local"
model_id = "code"
protocol = "chat-completions"
[serving_models.capabilities]
tools = true
tool_choice = true
temperature = true
top_p = true
reasoning_efforts = ["high", "xhigh"]
context_window = 200000
max_output_tokens = 32000

[[serving_models]]
id = "responses"
endpoint_id = "local"
model_id = "code"
protocol = "responses"
[serving_models.capabilities]
tools = true
tool_choice = true
temperature = true
top_p = true
reasoning_efforts = ["high", "xhigh"]
context_window = 200000
max_output_tokens = 32000

[[serving_models]]
id = "messages"
endpoint_id = "local"
model_id = "code"
protocol = "messages"
[serving_models.capabilities]
tools = true
tool_choice = true
temperature = true
top_p = true
reasoning_efforts = ["high", "xhigh", "max"]
context_window = 200000
max_output_tokens = 32000

[[routes]]
id = "code"
alias = "code"

[[targets]]
id = "code-chat"
route_id = "code"
serving_model_id = "chat"
position = 0
"#;

fn binding(catalog: &Catalog, id: &str) -> Binding {
    catalog.binding(&Id::new(id).unwrap()).unwrap().clone()
}

fn responses_binding(binding: &Binding, upstream: &str) -> llm_responses::Binding {
    llm_responses::Binding::new(binding.provenance().clone(), Id::new(upstream).unwrap())
}

fn claude_code_turn(catalog: &Catalog) -> TurnRequest {
    let reading = binding(catalog, "messages");
    llm_messages::decode_request(CLAUDE_CODE.as_bytes(), reading.provenance())
        .unwrap()
        .request
}

fn codex_turn(catalog: &Catalog) -> TurnRequest {
    let body: Value = serde_json::from_str(CODEX).unwrap();
    // The body names the alias the client was given; ingress reads it as the upstream name.
    let reading = responses_binding(&binding(catalog, "responses"), "code");
    llm_responses::ingest_request(&reading, &body).unwrap()
}

fn tool_names(tools: &Value, path: &[&str]) -> Vec<String> {
    tools
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| {
            let mut value = tool;
            for key in path {
                value = &value[*key];
            }
            value.as_str().unwrap().to_owned()
        })
        .collect()
}

const CLAUDE_CODE_SYSTEM: &str = "[attribution block: client version and conversation fingerprint]\nYou are Claude Code, an interactive command line tool.";

#[test]
fn claude_codes_first_turn_crosses_to_chat_completions() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let turn = claude_code_turn(&catalog);
    let body = llm_chat::project_request(&turn, &binding(&catalog, "chat"), true).unwrap();
    assert_eq!(
        body["messages"][0],
        json!({"role": "system", "content": CLAUDE_CODE_SYSTEM})
    );
    assert_eq!(body["reasoning_effort"], json!("xhigh"));
    let object = body.as_object().unwrap();
    assert!(
        !object.keys().any(|key| key.contains("thinking")),
        "adaptive thinking sends nothing: {body}"
    );
    assert_eq!(tool_names(&body["tools"], &["function", "name"]), ["Read"]);
    assert_eq!(llm_chat::omitted_advisories(&turn), [Advisory::PromptCache]);
}

#[test]
fn claude_codes_first_turn_crosses_to_responses() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let turn = claude_code_turn(&catalog);
    let body = llm_responses::project_request(
        &responses_binding(&binding(&catalog, "responses"), "served-code"),
        &turn,
    )
    .unwrap();
    assert_eq!(
        body["input"][0],
        json!({"type": "message", "role": "developer",
            "content": [{"type": "input_text", "text": CLAUDE_CODE_SYSTEM}]})
    );
    assert_eq!(body["reasoning"], json!({"effort": "xhigh"}));
    assert_eq!(tool_names(&body["tools"], &["name"]), ["Read"]);
    assert_eq!(body["tools"][0]["type"], json!("function"));
    assert_eq!(
        llm_responses::omitted_advisories(&turn),
        [Advisory::PromptCache]
    );
}

fn codex_instructions(body: &Value) -> String {
    let mut parts = vec![body["instructions"].as_str().unwrap().to_owned()];
    for part in body["input"][0]["content"].as_array().unwrap() {
        parts.push(part["text"].as_str().unwrap().to_owned());
    }
    parts.join("\n")
}

#[test]
fn codexs_first_turn_crosses_to_chat_completions() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let captured: Value = serde_json::from_str(CODEX).unwrap();
    let turn = codex_turn(&catalog);
    let body = llm_chat::project_request(&turn, &binding(&catalog, "chat"), true).unwrap();
    assert_eq!(body["parallel_tool_calls"], json!(true));
    assert_eq!(body["prompt_cache_key"], captured["prompt_cache_key"]);
    assert_eq!(body["reasoning_effort"], json!("high"));
    assert_eq!(
        body["messages"][0],
        json!({"role": "system", "content": codex_instructions(&captured)})
    );
    assert_eq!(
        tool_names(&body["tools"], &["function", "name"]),
        tool_names(&captured["tools"], &["name"])
    );
    assert_eq!(body["tools"].as_array().unwrap().len(), 4);
    assert_eq!(
        llm_chat::omitted_advisories(&turn),
        [
            Advisory::ReasoningSummary,
            Advisory::InputItemIds,
            Advisory::ClientMetadata
        ]
    );
}

#[test]
fn codexs_first_turn_crosses_to_messages() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let captured: Value = serde_json::from_str(CODEX).unwrap();
    let mut turn = codex_turn(&catalog);
    // Messages requires an output limit and Codex sends none; the caller supplies one, as the
    // gateway does.
    turn.max_output_tokens = Some(8192);
    let body: Value = serde_json::from_slice(
        &llm_messages::encode_request(&turn, &binding(&catalog, "messages")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        body["tool_choice"],
        json!({"type": "auto", "disable_parallel_tool_use": false})
    );
    assert_eq!(body["output_config"], json!({"effort": "high"}));
    assert_eq!(
        body["system"],
        json!([{"type": "text", "text": codex_instructions(&captured),
            "cache_control": {"type": "ephemeral"}}])
    );
    assert_eq!(
        tool_names(&body["tools"], &["name"]),
        tool_names(&captured["tools"], &["name"])
    );
    assert_eq!(body["tools"].as_array().unwrap().len(), 4);
    assert_eq!(
        llm_messages::omitted_advisories(&turn),
        [
            Advisory::PromptCacheKey,
            Advisory::ReasoningSummary,
            Advisory::InputItemIds,
            Advisory::ClientMetadata
        ]
    );
}
