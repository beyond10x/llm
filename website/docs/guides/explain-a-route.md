---
title: Explain a route
sidebar_position: 5
description: Read the routing library's explanation of a catalog, and see why a candidate was refused.
lede: The explanation names the selected target and every other candidate's rejection, without resolving a secret or sending a request.
source: crates/llm-routing/examples/explain.rs, examples/catalog.toml
---

# Explain a route

```bash
cargo run --locked -p b10x-llm-routing --example explain
```

The example reads
[`examples/catalog.toml`](https://github.com/beyond10x/llm/blob/main/examples/catalog.toml), which
declares a self-hosted provider with one anonymous local endpoint and one authenticated remote
endpoint, and a `coding` route whose two targets are ordered.

## The output

```json
{
  "route_id": "coding",
  "alias": "code",
  "config_digest": "1365b05fe63542f8211e21a9a2566c71f2a6d44ec9bdc564ba46c592a861825f",
  "input_tokens": 512,
  "selected_target_id": "coding-primary",
  "targets": [
    {
      "target_id": "coding-primary",
      "serving_model_id": "local-small",
      "position": 0,
      "provenance": {
        "protocol": "chat-completions",
        "provider": "my-lab",
        "account": "local",
        "endpoint": "local-vllm",
        "model": "small",
        "binding_revision": "1a6ea40f98c8eef26ff66e7c753dda816f45a1aa3488bb31f62ef25a89c328a1"
      },
      "auth_kind": "anonymous",
      "billing_kind": "self-hosted",
      "capabilities": {
        "tools": false,
        "tool_choice": false,
        "temperature": false,
        "top_p": false,
        "reasoning_efforts": [],
        "context_window": 4096,
        "max_output_tokens": 1024
      },
      "rejections": []
    },
    {
      "target_id": "coding-secondary",
      "serving_model_id": "remote-large",
      "position": 1,
      "provenance": {
        "protocol": "responses",
        "provider": "my-lab",
        "account": "remote",
        "endpoint": "remote-models",
        "model": "large",
        "binding_revision": "5dd662a7d1b9e5280a10e6b691ff04c79326d1562d18d3206e9d1bd141ed9a13"
      },
      "auth_kind": "bearer",
      "billing_kind": "metered",
      "capabilities": {
        "tools": true,
        "tool_choice": true,
        "temperature": true,
        "top_p": true,
        "reasoning_efforts": ["medium", "high"],
        "context_window": 32768,
        "max_output_tokens": 8192
      },
      "rejections": ["fallback-disabled"]
    }
  ]
}
```

## How to read it

- **`config_digest`** identifies the validated catalog. Two equivalent files that differ only in
  row order or decimal spelling share a digest; changing a declared fact changes it.
- **`binding_revision`** hashes the complete validated binding, including the endpoint URL,
  upstream model, auth reference and capabilities. It is the sixth coordinate that
  [opaque continuation state](../concepts/neutral-boundary.md) is checked against, so repointing
  `local-vllm` at another URL invalidates prior opaque state — deliberately.
- **`rejections: ["fallback-disabled"]`** is the second target's only reason for being ineligible.
  The route sets `fallback_enabled = false`, and omission is false. Turn it on and the same target
  becomes a named alternative.
- **`auth_kind`** and **`billing_kind`** are independent. `anonymous` + `self-hosted` and
  `bearer` + `metered` are both ordinary bindings.

## What the explanation is not

It resolved no secret — `lab-llm-token` appears in the catalog as a reference and nowhere in the
output. It sent no request, provisioned nothing and contains no prompt or opaque payload. Listing
or explaining routes must never do any of those things.
