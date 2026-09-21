//! Outgoing Chat Completions projection of the declared neutral subset.
mod common;

use common::{binding, binding_at, capable};
use llm_chat::project_request;
use llm_core::{
    CallId, Capabilities, ErrorCode, Item, Protocol, Sampling, ToolCall, ToolChoice, ToolName,
    ToolSpec, TurnRequest,
};
use serde_json::json;

fn tool(name: &str) -> ToolSpec {
    ToolSpec {
        name: ToolName::new(name).expect("fixture tool"),
        description: "Look a city up".to_owned(),
        input_schema: json!({"type":"object","properties":{"city":{"type":"string"}}}),
    }
}

fn call(call_id: &str, name: &str) -> ToolCall {
    ToolCall {
        call_id: CallId::new(call_id).expect("fixture call"),
        name: ToolName::new(name).expect("fixture tool"),
        arguments: json!({"city":"Kyoto"}),
    }
}

#[test]
fn a_text_turn_projects_the_upstream_model_messages_and_usage_options() {
    let binding = binding();
    let mut request = TurnRequest::new("small", vec![Item::user("Hello")]);
    request.instructions = "Be brief".to_owned();
    request.max_output_tokens = Some(256);
    request.sampling = Sampling {
        temperature: Some(0.5),
        top_p: None,
        reasoning_effort: Some("high".to_owned()),
    };

    let body = project_request(&request, &binding, true).expect("projected");

    assert_eq!(
        body,
        json!({
            // The wire carries the binding's upstream name, never the caller's route alias.
            "model": "Qwen/Qwen3-8B",
            "messages": [
                {"role": "system", "content": "Be brief"},
                {"role": "user", "content": "Hello"}
            ],
            "max_completion_tokens": 256,
            "temperature": 0.5,
            "reasoning_effort": "high",
            "stream": true,
            "stream_options": {"include_usage": true}
        })
    );
}

#[test]
fn an_absent_sampling_field_stays_absent_on_the_wire() {
    let body = project_request(
        &TurnRequest::new("small", vec![Item::user("Hello")]),
        &binding(),
        false,
    )
    .expect("projected");

    assert_eq!(
        body,
        json!({
            "model": "Qwen/Qwen3-8B",
            "messages": [{"role": "user", "content": "Hello"}],
            "stream": false
        })
    );
}

#[test]
fn consecutive_assistant_text_and_tool_calls_share_one_assistant_message() {
    let mut request = TurnRequest::new(
        "small",
        vec![
            Item::user("Weather?"),
            Item::assistant("Let me look."),
            Item::ToolCall(call("call-1", "lookup")),
            Item::ToolCall(call("call-2", "lookup")),
            Item::ToolResult {
                call_id: CallId::new("call-1").expect("fixture call"),
                output: json!("sunny"),
                failed: false,
            },
            Item::ToolResult {
                call_id: CallId::new("call-2").expect("fixture call"),
                output: json!({"celsius": 21}),
                failed: false,
            },
        ],
    );
    request.tools = vec![tool("lookup")];

    let body = project_request(&request, &binding(), true).expect("projected");

    assert_eq!(
        body["messages"],
        json!([
            {"role": "user", "content": "Weather?"},
            {
                "role": "assistant",
                "content": "Let me look.",
                "tool_calls": [
                    {"id": "call-1", "type": "function",
                     "function": {"name": "lookup", "arguments": "{\"city\":\"Kyoto\"}"}},
                    {"id": "call-2", "type": "function",
                     "function": {"name": "lookup", "arguments": "{\"city\":\"Kyoto\"}"}}
                ]
            },
            {"role": "tool", "tool_call_id": "call-1", "content": "sunny"},
            {"role": "tool", "tool_call_id": "call-2", "content": "{\"celsius\":21}"}
        ])
    );
    assert_eq!(
        body["tools"],
        json!([{
            "type": "function",
            "function": {
                "name": "lookup",
                "description": "Look a city up",
                "parameters": {"type":"object","properties":{"city":{"type":"string"}}}
            }
        }])
    );
}

#[test]
fn a_failed_tool_result_is_never_projected_as_a_successful_one() {
    let mut request = TurnRequest::new(
        "small",
        vec![
            Item::ToolCall(call("call-1", "lookup")),
            Item::ToolResult {
                call_id: CallId::new("call-1").expect("fixture call"),
                output: json!(""),
                failed: true,
            },
        ],
    );
    request.tools = vec![tool("lookup")];

    let body = project_request(&request, &binding(), true).expect("projected");
    let content = body["messages"][1]["content"]
        .as_str()
        .expect("tool content is text")
        .to_owned();

    // An empty failed result must not reach the model as an empty success.
    assert_ne!(content, "");
    assert!(content.contains("\"ok\":false"), "{content}");
}

#[test]
fn a_named_tool_choice_projects_the_function_object() {
    let mut request = TurnRequest::new("small", vec![Item::user("Weather?")]);
    request.tools = vec![tool("lookup")];
    request.tool_choice = ToolChoice::Named(ToolName::new("lookup").expect("fixture tool"));
    let body = project_request(&request, &binding(), true).expect("projected");
    assert_eq!(
        body["tool_choice"],
        json!({"type": "function", "function": {"name": "lookup"}})
    );

    request.tool_choice = ToolChoice::Required;
    let body = project_request(&request, &binding(), true).expect("projected");
    assert_eq!(body["tool_choice"], json!("required"));

    request.tool_choice = ToolChoice::Auto;
    let body = project_request(&request, &binding(), true).expect("projected");
    assert_eq!(body.get("tool_choice"), None);
}

#[test]
fn opaque_state_is_refused_rather_than_dropped() {
    let binding = binding();
    let request = TurnRequest::new(
        "small",
        vec![Item::Opaque {
            provenance: binding.provenance().clone(),
            payload: json!({"encrypted": "x"}),
        }],
    );
    let error = project_request(&request, &binding, true).expect_err("refused");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert!(error.message.contains("opaque"), "{}", error.message);
}

#[test]
fn a_binding_serving_another_protocol_is_refused() {
    let binding = binding_at(Protocol::Responses, capable(), "http://127.0.0.1:8000/v1");
    let error = project_request(
        &TurnRequest::new("small", vec![Item::user("Hello")]),
        &binding,
        true,
    )
    .expect_err("refused");
    assert_eq!(error.code, ErrorCode::InvalidRequest);
}

#[test]
fn a_setting_the_binding_does_not_declare_is_refused_before_any_request_is_built() {
    let binding = binding_at(
        Protocol::ChatCompletions,
        Capabilities::text(32_768, 4_096),
        "http://127.0.0.1:8000/v1",
    );
    let mut request = TurnRequest::new("small", vec![Item::user("Hello")]);
    request.sampling.temperature = Some(0.5);
    let error = project_request(&request, &binding, true).expect_err("refused");
    assert_eq!(error.code, ErrorCode::Unsupported);
}

#[test]
fn the_request_model_must_be_the_selected_binding_alias() {
    let error = project_request(
        &TurnRequest::new("another-model", vec![Item::user("Hello")]),
        &binding(),
        true,
    )
    .expect_err("refused");
    assert_eq!(error.code, ErrorCode::InvalidRequest);
}
