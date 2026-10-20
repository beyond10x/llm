//! Adversary pass 2 on the tool-result unit (wave 2026-10-20-w09): the Responses depth claims
//! `docs/design.md` "Failed and rejected tool results" makes.
use llm_core::{CallId, Id, Item, Protocol, Provenance, ToolCall, ToolName, ToolSpec, TurnRequest};
use llm_responses::{Binding, ingest_request, project_request};
use serde_json::{Value, json};

fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
}

fn binding() -> Binding {
    Binding::new(
        Provenance {
            protocol: Protocol::Responses,
            provider: id("my-lab"),
            account: id("local"),
            endpoint: id("local-vllm"),
            model: id("small"),
            binding_revision: id("rev-1"),
        },
        id("example/Small-Model"),
    )
}

fn nested(levels: usize) -> Value {
    let mut output = json!("sunny");
    for _ in 0..levels {
        output = Value::Array(vec![output]);
    }
    output
}

fn turn_with_result(output: Value, failed: bool) -> TurnRequest {
    let call_id = CallId::new("call-1").expect("fixture call");
    let name = ToolName::new("lookup").expect("fixture tool");
    let mut request = TurnRequest::new(
        "small",
        vec![
            Item::user("Weather?"),
            Item::ToolCall(ToolCall {
                call_id: call_id.clone(),
                name: name.clone(),
                arguments: json!({}),
            }),
            Item::ToolResult {
                call_id,
                output,
                failed,
            },
        ],
    );
    request.tools = vec![ToolSpec {
        name,
        description: "Look up".to_owned(),
        input_schema: json!({"type": "object"}),
    }];
    request
}

/// The text of the one `function_call_output` in a projected body: the envelope as sent.
fn envelope_text(body: &Value) -> String {
    body["input"]
        .as_array()
        .expect("input is an array")
        .iter()
        .find(|entry| entry["type"] == "function_call_output")
        .and_then(|entry| entry["output"].as_str())
        .expect("one function_call_output carrying a string")
        .to_owned()
}

/// `docs/design.md` and `docs/responses.md` state the limit: ingress parses the envelope at most
/// 127 levels deep, so an output nested more than 126 levels deep, failed or not, reads back as
/// a successful string carrying the envelope's text. Every result is enveloped, so a successful
/// output past the limit changes too, not only a failed one.
#[test]
fn adv_an_output_nested_past_the_parse_limit_failed_or_not_reads_back_as_the_envelopes_text() {
    for failed in [false, true] {
        let request = turn_with_result(nested(127), failed);
        let body = project_request(&binding(), &request).expect("projects");
        let ingested = ingest_request(&binding(), &body).expect("reads back");
        assert_eq!(
            ingested.items.last(),
            Some(&Item::ToolResult {
                call_id: CallId::new("call-1").expect("fixture call"),
                output: json!(envelope_text(&body)),
                failed: false,
            }),
            "an output nested 127 levels deep (failed: {failed}) is past serde_json's 128-level \
             parse limit, so it reads back as the envelope's text, as docs/design.md states"
        );
    }
}
