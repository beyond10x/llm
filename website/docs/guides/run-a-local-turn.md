---
title: Run a local turn
description: Complete a model turn through the neutral port without a gateway, a network or a credential.
---

# Run a local turn

```bash
cargo run --locked -p b10x-llm-core --example embedded
```

```text
An embedded model turn. (EndTurn; usage None)
```

The source is
[`crates/llm-core/examples/embedded.rs`](https://github.com/beyond10x/llm/blob/main/crates/llm-core/examples/embedded.rs):
a `LocalModel` that implements `Model`, validates the request against its own declared
capabilities, streams one text delta and returns an outcome. `usage None` is the honest answer: the
model reports no counters.

The example completes a turn against an in-process model. There is no gateway, no network call and
no credential, which is the point: the caller depends on the port, not on a vendor SDK.

## What the example demonstrates

- `Model::turn` takes one request, a caller-owned asynchronous sink and a cancellation token, and
  performs exactly **one** attempt.
- Streamed output arrives through the sink, and each emission is awaited, so backpressure is
  bounded rather than buffered without limit.
- The outcome carries a `TurnObservation` pinned to the selected immutable binding.

## What it does not demonstrate

It does not touch a network. For that, [Call a local endpoint](call-a-local-endpoint.md) builds a
`ChatClient` from a catalog and runs the same port over HTTP. No live provider credential has been
used anywhere in this repository, so running a client against a real provider is something you
would be doing first, not repeating.

## Writing your own caller

Depend on the port, not on a binding. A caller that holds `dyn Model` keeps working when the
implementation behind it changes from an embedded fixture to a protocol adapter, because
retries, alternate accounts and fallback are routing's concern and never the client's.

Tool definitions you pass carry a name, a description and a JSON Schema. They confer **no**
execution permission — LLM will not run your tool. When the model emits a tool call, execute it
yourself and supply the result as an item in a subsequent request.
