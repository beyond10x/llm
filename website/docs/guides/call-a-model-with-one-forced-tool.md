---
title: Call a model with one forced tool
sidebar_position: 3
description: Force one named tool on any model with call_tool and read the call's JSON arguments, then point the same call at a Codex login.
lede: call_tool turns a model into a typed function call that returns one tool call, or a ModelError that says why not.
source: crates/llm-tool-call (lib.rs, tests/forced_tool_call.rs), crates/llm-docs/examples/forced_tool_call.rs
---

# Call a model with one forced tool

`b10x-llm-tool-call` asks a model for exactly one call to a tool you name, and returns that call's
arguments as JSON. It is the shape an agent uses to classify, route or extract: the model fills in
a schema, and the caller decides what to do with the result. llm never runs the tool.

## Run it

```bash
cargo run --locked -p llm-docs --example forced_tool_call
```

```text
{"component":"importer","severity":"high"}
```

## The program

The model here is recorded in-process, so the run sends nothing and reads no credential.
`call_tool` takes any `&dyn Model`, so the same call works on a `ResponsesClient`, a
`MessagesClient`, a `ChatClient` or a test double.

```rust title="crates/llm-docs/examples/forced_tool_call.rs"
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
```

`call_tool(model, instructions, items, tool)` sends one turn: `instructions` as the standing
instruction, `items` as the conversation, and `tool` as the only tool, chosen by name. It sets no
output limit, because the Codex backend may refuse one.

## What comes back

| Result | When |
| --- | --- |
| `Ok(arguments)` | The model called the forced tool once with a JSON object |
| `ModelError::NoToolCall` | The model answered without calling the tool |
| `ModelError::WrongTool` | The model called another tool |
| `ModelError::MultipleToolCalls(n)` | The model called it more than once, so no single answer exists |
| `ModelError::Transport(error)` | The endpoint could not be reached, or the stream stopped before the answer completed |
| `ModelError::MissingCredential`, `ExpiredCredential`, `UnusableCredential` | The Codex login cannot be used; nothing was sent, and the message says to run `codex` |
| `ModelError::Setup(error)` | The model binding could not be built |
| `ModelError::Model(error)` | Any other refusal, including arguments that are not a JSON object |

## Ask a real model through a Codex login

Replace the recorded model with the Codex preset:

```rust
let model = b10x_llm_tool_call::codex_model("<model>")?;
```

`codex_model` binds a `ResponsesClient` to the Codex backend and authenticates it with the access
token of the operator's Codex login, at `$CODEX_HOME/auth.json` or else `~/.codex/auth.json`.
Building the model reads nothing; the login is read on every call, before the request leaves, and
never written. An expired login is `ModelError::ExpiredCredential`: run `codex` to renew it.
`codex_model_at(model, base_url, auth_path)` takes the endpoint and the login path explicitly.

This run was not made for this page: it needs a Codex login, and the gate never uses one. The
crate's tests run the preset against a local server that answers like the Codex backend.
