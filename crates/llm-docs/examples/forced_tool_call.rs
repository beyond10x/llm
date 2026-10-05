//! Force one tool on a model and read the call's JSON arguments.
//!
//! Run with `cargo run --locked -p llm-docs --example forced_tool_call`. The model here is
//! recorded in-process, so nothing is sent and no credential is read; swap in
//! `b10x_llm_tool_call::codex_model("<model>")?` to ask a real model through a Codex login.
use b10x_llm_tool_call::{ModelError, call_tool};
use llm_core::{
    BoxFuture, CallId, Cancel, Capabilities, Error, Id, Item, Model, Protocol, Provenance,
    StopReason, StreamSink, ToolCall, ToolName, ToolSpec, TurnObservation, TurnOutcome,
    TurnRequest,
};
use serde_json::json;

/// A model that answers every turn with one call to the tool the request forced.
struct Recorded {
    target: Provenance,
    capabilities: Capabilities,
}

impl Model for Recorded {
    fn provenance(&self) -> &Provenance {
        &self.target
    }
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }
    fn turn<'a>(
        &'a self,
        request: &'a TurnRequest,
        _sink: &'a mut dyn StreamSink,
        _cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        Box::pin(async move {
            request.validate_for(&self.target, &self.capabilities)?;
            let call = ToolCall {
                call_id: CallId::new("call-1").map_err(|_| Error::invalid("call id"))?,
                name: request.tools[0].name.clone(),
                arguments: json!({"severity": "high", "component": "importer"}),
            };
            Ok(TurnOutcome {
                stop_reason: StopReason::ToolCalls,
                items: vec![Item::ToolCall(call)],
                observation: TurnObservation {
                    final_usage: true,
                    ..TurnObservation::new(self.target.clone())
                },
            })
        })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let model = Recorded {
        target: Provenance {
            protocol: Protocol::Responses,
            provider: Id::new("recorded")?,
            account: Id::new("none")?,
            endpoint: Id::new("in-process")?,
            model: Id::new("triage")?,
            binding_revision: Id::new("recorded-v1")?,
        },
        capabilities: Capabilities {
            tools: true,
            tool_choice: true,
            ..Capabilities::text(8192, 1024)
        },
    };
    let tool = ToolSpec {
        name: ToolName::new("classify_report")?,
        description: "Classify one bug report".to_owned(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "severity": {"enum": ["low", "medium", "high"]},
                "component": {"type": "string"}
            },
            "required": ["severity", "component"]
        }),
    };
    let report = vec![Item::user("The importer drops every row after a retry.")];

    match call_tool(&model, "Classify the report.", report, tool).await {
        Ok(arguments) => println!("{arguments}"),
        Err(ModelError::NoToolCall) => println!("the model answered without calling the tool"),
        Err(error) => return Err(format!("{error:?}").into()),
    }
    Ok(())
}
