---
format: aep.planning-md/3
id: review-result:security-w25-web-identity-pass-1
kind: review-result
status: active
title: Wave 2026-11-04-w25 security review, llm story:bedrock-web-identity, pass 1
relations:
- reviews: story:bedrock-web-identity
revision: 1
---
```
unit: beyond10x/llm story:bedrock-web-identity, integration head 196f113c8 plus test commit db0e48263
verdict: CONFIRMED (no invariant broken, no finding)
cases: executed 20→32, red 0
origin: introduced 0 / pre-existing 0 / undecided 0
needs-coordinator: none (the routes-document kind was left out by design)
```

Invariants checked, all held: no secret in a `Debug` form, message, log or panic; the token and keys
zeroized (by reading: a memory test needs `unsafe`, which the workspace forbids; the access key id
stays a plain `String` as `AwsCredentials` already had it); the token read on every fetch, never kept;
no redirect followed, no ambient proxy reached, refused endpoints refused; the token only in the
POST body, never the URL; `post_form` keeps the transport's TLS, redirect, proxy, deadline and body
bounds, and keeps a failing body zeroized and out of its `Debug` form; the size bound applies before
buffering; no new dependency; the public API change is additive.

Cases added (all green), in `crates/llm-bedrock/tests/security_w25_web_identity.rs` (7) and
`crates/llm-http/tests/security_w25_form_exchange.rs` (5).

```findings
[]
```
