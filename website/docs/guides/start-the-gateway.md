---
title: Start the gateway
description: Compose the authenticated single-owner gateway with a route inventory, and query its probes and inspection routes with curl.
---

# Start the gateway

This guide composes `b10x-llm-gateway` in a small program and queries it. It serves liveness,
readiness and route inspection; it does not answer model requests.

## The program

The only dependency is `b10x-llm-gateway`.

```rust
use llm_gateway::{
    AuthKind, BillingKind, Gateway, GatewayConfig, Label, OwnerToken, RouteInventory,
    RouteSummary, SharedSecretVerifier, TargetLimits, TargetProvenance, TargetSummary,
};
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The embedding resolves the owner credential itself, once, from a source it chose.
    let material = std::env::var("GATEWAY_OWNER_TOKEN")?.into_bytes();
    let verifier = Arc::new(SharedSecretVerifier::new(OwnerToken::new(material)?)?);

    let target = TargetSummary {
        target_id: Label::new("coding-primary")?,
        position: 0,
        provenance: TargetProvenance {
            protocol: Label::new("chat-completions")?,
            provider: Label::new("my-lab")?,
            account: Label::new("local")?,
            endpoint: Label::new("local-vllm")?,
            model: Label::new("small")?,
            binding_revision: Label::new("1a6ea40f98c8eef26ff66e7c753dda816f45a1aa3488bb31f62ef25a89c328a1")?,
        },
        auth_kind: AuthKind::Anonymous,
        billing_kind: BillingKind::SelfHosted,
        limits: TargetLimits { context_window: Some(4096), max_output_tokens: Some(1024) },
    };
    let route = RouteSummary::new(Label::new("coding")?, Label::new("code")?, false, vec![target])?;
    let inventory = RouteInventory::new(vec![route])?;

    let handle = Gateway::bind(GatewayConfig::new("127.0.0.1:8080".parse()?), verifier, inventory)?;
    handle.mark_ready();
    println!("serving on {}", handle.local_addr());
    std::thread::sleep(std::time::Duration::from_secs(30));
    println!("{:?}", handle.shutdown());
    Ok(())
}
```

The identifiers match what [Explain a route](explain-a-route.md) prints for the example catalog.
The gateway crate does not depend on `llm-routing`, so building the inventory from a catalog is the
embedding's job. Pass identifiers only: whatever bytes you put in a `Label` are what the owner will
see.

Run it with a random owner token of at least 32 bytes:

```bash
export GATEWAY_OWNER_TOKEN="$(head -c 48 /dev/urandom | base64 | tr -d '/+=\n')"
cargo run
```

A shorter token is refused at composition with `SecretTooShort`.

## Query it

Liveness needs no credential:

```bash
curl -i http://127.0.0.1:8080/health
```

```text
HTTP/1.1 200 OK
content-type: application/json
content-length: 17
cache-control: no-store
connection: close

{"status":"live"}
```

Inspection without the credential is refused, and so is an unknown path, with the same code:

```bash
curl http://127.0.0.1:8080/v1/routes
curl -X POST http://127.0.0.1:8080/whatever
```

```json
{"error":{"code":"credential-absent","message":"no owner credential was presented"}}
```

With the owner token:

```bash
curl -H "Authorization: Bearer $GATEWAY_OWNER_TOKEN" http://127.0.0.1:8080/v1/routes
```

```json
{"route_count":1,"routes":[{"route_id":"coding","alias":"code","fallback_enabled":false,"target_count":1,"targets":[{"target_id":"coding-primary","position":0,"protocol":"chat-completions","provider":"my-lab","account":"local","endpoint":"local-vllm","model":"small","binding_revision":"1a6ea40f98c8eef26ff66e7c753dda816f45a1aa3488bb31f62ef25a89c328a1","auth_kind":"anonymous","billing_kind":"self-hosted","context_window":4096,"max_output_tokens":1024}]}]}
```

An unknown alias, and a wrong token:

```json
{"error":{"code":"route-unknown","message":"no such route alias"}}
{"error":{"code":"credential-rejected","message":"the presented owner credential was rejected"}}
```

After 30 seconds the program shuts down and prints what it served, for example:

```text
ShutdownReport { accepted: 6, completed: 6, in_flight_at_signal: 0 }
```

`completed` equals `accepted` after a graceful stop. It counts every accepted connection, refused
ones included.

[The gateway](../concepts/gateway.md) lists every refusal code and bound.
