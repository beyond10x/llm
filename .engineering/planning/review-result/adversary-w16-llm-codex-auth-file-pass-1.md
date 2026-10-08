---
format: aep.planning-md/3
id: review-result:adversary-w16-llm-codex-auth-file-pass-1
kind: review-result
status: archived
title: Wave 2026-10-04-w16 adversary, llm story:codex-auth-file, pass 1
relations:
- reviews: story:codex-auth-file
revision: 2
transitions:
- {from: "active", to: "archived", at: "2026-10-08T10:43:37Z", actor: "human:timo", revision: 2}
---
## Adversary pass 1 — llm story:codex-auth-file

Tree: f0957321 + phase 2. Verdict NEEDS-CHANGE; cases 17→25, red 2.
New file: `crates/llm-credentials/tests/adversary_codex.rs` (8 cases).

| # | where | verdict | finding | decision | outcome |
|---|---|---|---|---|---|
| 1 | `crates/llm-credentials/src/codex.rs:170` | NEEDS-CHANGE | `read_to_end` into an unsized buffer regrew 11 times and freed id-token bytes 5 times and access-token bytes once, unzeroized (allocator probe outside the repo; `unsafe_code` is forbidden) | fix: one exact-size read sized from the opened file, require EOF, as `file.rs` | fixed (probe after: [0,0,0]) |
| 2 | `crates/llm-credentials/src/codex.rs:46` | NEEDS-CHANGE | a relative path resolved through the working directory, against spec field `absolute_path` | fix: refuse a non-absolute path as `Unavailable` before opening | fixed |
| 3 | `docs/local-secrets.md:116` | INFEASIBLE | integer `exp` -1 was `Unavailable`; the docs say `Expired` | fix the code to the docs | fixed |
| 4 | `docs/local-secrets.md:117` | CONFIRMED | the Codex section omitted `TooLarge` | fix the docs | fixed |
| 5 | `crates/llm-credentials/src/codex.rs:8` | CONFIRMED | the story promised a default `~/.codex/auth.json`; the code has none | decline: llm does no ambient lookup; the embedding application expands the path; story body updated | no-op |

Residual found by the fix pass: a token written with JSON escapes passes through serde_json's
scratch buffer, freed unzeroized (4 blocks in the probe). Accepted and documented in
`docs/local-secrets.md`: the Codex CLI writes unescaped base64url JWTs.
