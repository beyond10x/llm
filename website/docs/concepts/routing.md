---
title: Routing
description: A strict TOML catalog, ordered opt-in selection, capability admission, and an explanation that resolves no secret.
---

# Routing

`llm-routing` parses a strict `llm.catalog/1` TOML document and explains capability-aware selection
without performing any I/O.

## What a catalog declares

Providers, accounts, endpoints, models, serving models, routes and ordered targets are separate
tables with operator-defined ids. There is no built-in-name-only resolver: nothing resolves because
a string happened to look like a vendor's model name.

```toml
format = "llm.catalog/1"

[[accounts]]
id = "remote"
provider_id = "my-lab"
auth_kind = "bearer"
billing_kind = "metered"
secret_reference_id = "lab-llm-token"

[[serving_models]]
id = "remote-large"
endpoint_id = "remote-models"
model_id = "large"
protocol = "responses"
[serving_models.capabilities]
tools = true
context_window = 32768
max_output_tokens = 8192
```

## Selection is ordered and opt-in

`fallback_enabled` defaults to **false**; omission is false. When it is off, only the first target
of a route is eligible and every other candidate is reported with the rejection reason
`fallback-disabled`. When it is on, selection may take the first compatible target among the
explicitly named alternatives — never a target the route did not name.

Admission requires a caller-supplied **input-token upper bound** that must be valid for every
candidate. An unknown input count refuses admission rather than guessing.

## Explanation is safe by construction

The explanation exposes route id, alias, configuration digest, the selected target, and for each
candidate its provenance, authentication kind, billing kind, declared capabilities and rejection
reasons. It contains no prompt, no opaque payload and no secret, because explaining resolves
nothing.

See [Explain a route](../guides/explain-a-route.md) for the real output of the shipped example.

## What routing does not do yet

Runtime fallback *after an attempted request* is not implemented. The design constrains it in
advance: only defined failures **before output becomes visible** are eligible; request
capabilities, opaque-state compatibility, deadlines and remaining spending limits constrain every
alternative; and an ambiguously accepted request is never replayed on the assumption that it was
free. No semantic downgrade, no cross-account switch outside the named chain, and no fallback after
an exposed partial stream.
