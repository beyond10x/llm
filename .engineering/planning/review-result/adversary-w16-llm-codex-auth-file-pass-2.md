---
format: aep.planning-md/3
id: review-result:adversary-w16-llm-codex-auth-file-pass-2
kind: review-result
status: active
title: Wave 2026-10-04-w16 adversary, llm story:codex-auth-file, pass 2
relations:
- reviews: story:codex-auth-file
revision: 1
---
## Adversary pass 2 — llm story:codex-auth-file

Tree: f0957321 + phase 2 + pass-1 fixes and tests. Verdict CONFIRMED red; cases 28→33, red 4.
New file: `crates/llm-credentials/tests/adversary2_codex.rs` (5 cases).

| # | where | verdict | finding | decision | outcome |
|---|---|---|---|---|---|
| 1 | `crates/llm-credentials/src/codex.rs:192` | INFEASIBLE | a FIFO at the path blocked `File::open` forever, holding one of 8 read permits and runtime shutdown | fix: open O_NONBLOCK and refuse a non-regular file as `Unavailable`, as `file.rs` | fixed |
| 2 | `crates/llm-credentials/src/codex.rs:213` | INFEASIBLE | a clock before the epoch was clamped to 0 | fix: signed whole seconds | fixed |
| 3 | `crates/llm-credentials/src/codex.rs:202` | INFEASIBLE | derived `Deserialize` accepted arrays for the document and `tokens` | fix: objects only | fixed |
| 4 | `crates/llm-credentials/src/codex.rs:249` | INFEASIBLE | a JWT payload that is an array was read positionally as `exp` | fix: objects only | fixed |

Held: 3200 resolves, 32 at a time, while the file was atomically replaced: no refusals, no mixed
tokens, each version matches its token (a mutant taking the size from the path refused 96).

After the fixes: `b10x-llm-credentials --all-features` 35 passed, 0 failed. The coordinator ran
`clippy --target x86_64-pc-windows-gnu -p b10x-llm-credentials --all-targets --all-features`:
exit 0. macOS was not compiled locally; CI's `native-adapter-builds` covers it.
