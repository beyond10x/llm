---
format: aep.planning-md/3
id: review-result:security-w26-web-identity-route-pass-1
kind: review-result
status: active
title: Wave 2026-11-05-w26 security review, llm story:bedrock-web-identity-route, pass 1
relations:
- reviews: story:bedrock-web-identity-route
revision: 1
---
```
unit: beyond10x/llm story:bedrock-web-identity-route, commits 532ebbae4 and 6ef1e525f
verdict: CONFIRMED (one note), overall GREEN
cases: executed 8→15 (feature file), 6→9 (no features), red 0
origin: introduced 1 / pre-existing 0 / undecided 0
```

Held: every `auth` field other than `kind` is refused in YAML and TOML without echo; no STS value
comes from the document, and a caller endpoint gets the `WebIdentityCredentials::new` rule (https, or
http to loopback); the session name is validated; with no role supplied the route is refused when its
port is built and never falls back to container credentials; the token is re-read at each fetch
through `FileResolver::mounted`, including a kubelet-style volume swap; no token in Debug, errors or
STS error text; public API additive only. Cases committed in
`crates/llm-models/tests/adversary_w26_web_identity_routes.rs` (57afeba6a, merged 00634e1b2; a fixture
key id was lengthened so the common secret check no longer matched it).

```findings
- file: contracts/ess-inputs.yaml
  line: 370
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the six aws-web-identity scenarios exercise Load only; the token-file and role wiring are held by the Rust tests
```
