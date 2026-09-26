---
title: Call a local endpoint
description: Declare an anonymous vLLM-compatible server in TOML, build a Chat Completions client from the catalog, and run one turn through the neutral port.
---

# Call a local endpoint

This guide sends one real turn to a Chat Completions server on your own machine, such as vLLM. It
uses the `local-small` serving model from
[`examples/catalog.toml`](https://github.com/beyond10x/llm/blob/main/examples/catalog.toml), which
declares an anonymous, self-hosted account:

```toml
[[accounts]]
id = "local"
provider_id = "my-lab"
auth_kind = "anonymous"
billing_kind = "self-hosted"

[[endpoints]]
id = "local-vllm"
account_id = "local"
base_url = "http://127.0.0.1:8000/v1"

[[models]]
id = "small"
upstream_name = "example/Small-Model"
```

Replace `base_url`, `upstream_name` and the declared capabilities with your server's facts.
`upstream_name` is the model name the server answers to.

## The program

Dependencies: `b10x-llm-core`, `b10x-llm-chat`, `b10x-llm-http`, `b10x-llm-credentials`,
`b10x-llm-routing`, and `tokio` with the `rt` and `macros` features. See
[Getting started](../getting-started.md#evaluate-a-crate-from-your-own-project) for the Git
dependency form.

```rust
use llm_chat::ChatClient;
use llm_core::{BoxFuture, Cancel, Id, Item, Model, TurnRequest, VecSink};
use llm_credentials::{ResolvedSecret, SecretError, SecretRef, SecretResolver};
use llm_http::{HttpClient, Limits};
use llm_routing::Catalog;
use std::sync::Arc;

/// The local target is anonymous, so nothing is ever resolved.
struct NoSecrets;
impl SecretResolver for NoSecrets {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async { Err(SecretError::Missing) })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = Catalog::parse(&std::fs::read_to_string("examples/catalog.toml")?)?;
    let binding = catalog
        .binding(&Id::new("local-small")?)
        .ok_or("no such serving model")?
        .clone();
    let client = ChatClient::new(binding, HttpClient::new(Limits::default())?, Arc::new(NoSecrets));

    let request = TurnRequest::new("small", vec![Item::user("Hallo")]);
    let mut sink = VecSink::new(64, 64 * 1024);
    let outcome = client.turn(&request, &mut sink, &Cancel::new()).await?;

    println!("text:           {}", sink.text());
    println!("stop reason:    {:?}", outcome.stop_reason);
    println!("upstream model: {:?}", outcome.observation.upstream_model.map(|m| m.to_string()));
    println!("usage:          {:?}", outcome.observation.usage);
    println!("final usage:    {}", outcome.observation.final_usage);
    Ok(())
}
```

`Catalog::binding` takes a **serving-model** id and returns the validated binding for it. The
request names `small`, the catalog's own model id, which must match the binding's model.

## What happens

The client sends one `POST` to `http://127.0.0.1:8000/v1/chat/completions` with this body:

```json
{"messages":[{"content":"Hallo","role":"user"}],"model":"example/Small-Model","stream":true,"stream_options":{"include_usage":true}}
```

- `model` is the declared `upstream_name`, not the caller's `small`.
- `stream_options.include_usage` is always sent, so the server is asked for counters.
- No `Authorization` header is sent, because the account is anonymous. The resolver is never
  called.

Replayed against the vLLM response fixture in `crates/llm-chat/fixtures/vllm-text-no-usage.sse`,
the program prints:

```text
text:           Guten Tag
stop reason:    EndTurn
upstream model: Some("Qwen/Qwen3-8B")
usage:          None
final usage:    true
```

Read the last three lines carefully:

- **`upstream model`** is what the server said served the request. It differs from the declared
  `example/Small-Model`, and LLM reports the difference instead of hiding it.
- **`usage: None`**: this fixture's server sent no counters. They are unknown, not zero.
- **`final usage: true`**: the stream terminated, so what was reported is all there will be.

## Next

- Use `bearer` or `api-key` accounts with a real resolver:
  [Resolve a local secret](resolve-a-local-secret.md).
- Put several targets behind one alias and fall back between them:
  [Routing](../concepts/routing.md#ordered-fallback).
- `MessagesClient::new` has the same inputs but returns a `Result`, because it refuses a binding
  whose protocol is not `messages`.
