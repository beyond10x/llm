---
format: aep.planning-md/3
id: review-result:adversary-w19-opencode-go-session-pass-1
kind: review-result
status: active
title: Adversary, pass 1 (story:opencode-go-session, wave 2026-10-26-w19)
relations:
- reviews: story:opencode-go-session
revision: 1
---
unit: story:opencode-go-session, covering `impl/opencode-go-session` at `fa07c044` (unit `ff3ac77b..60ea5b23`, plus the adversary's test commit)
verdict: green
cases: executed 217→221, red 0
origin: introduced 4, pre-existing 0, undecided 0
wrote-outside-worktree: scratch only (go-docs.html, suite.log, a commit message file)
needs-coordinator: no

No failing case. Four note-level findings; none blocks the merge.

Diff (`git diff --stat 60ea5b23..fa07c044`): only `crates/llm-messages/tests/adversary_w19_opencode_session.rs | 127 +`, committed as `fa07c044` by `b10x-bot[bot]`, not pushed.

Cases added (4), all green on their first solo run (`cargo test -p llm-messages --locked --test adversary_w19_opencode_session`):

| Case | What it asserts |
|---|---|
| `the_trigger_compares_the_normalised_url` | fullwidth (IDNA) host, backslashes, uppercase scheme, `%2e` and `zen/..` match; `%2e%2e`, `//`, `:80`, IPv6/IPv4 hosts don't; userinfo and query are refused |
| `a_minted_session_has_the_documented_shape` | value is `llm-<pid hex>-<hex>-<hex>`, at most 62 bytes |
| `two_clients_differ_in_their_sequence_field` | catches a sequence that never advances, which the clock-based uniqueness test misses (reasoned, not run against a mutated copy) |
| `the_last_named_session_wins` | the last `with_session` value is the one sent |

Suite run: `cargo test -p llm-messages --locked --no-fail-fast` (Rust 1.98.0) exited 0: 221 passed, 0 failed. The before count, 217, is the same run minus the 4 cases.

| # | file:line | Verdict / origin | What was measured | What reaches it |
|---|---|---|---|---|
| F1 | `crates/llm-messages/src/client.rs:56` | INFEASIBLE / introduced | `https://opencode.ai./inference/go/anthropic/v1` (trailing-dot host) and `/inference/%67o/` are not recognised, so the header isn't sent | nothing found: takt's endpoint and the docs page both use `https://opencode.ai/inference/go/anthropic/v1` |
| F2 | `crates/llm-models/src/lib.rs:288` | CONFIRMED / introduced | the client is built inside `Port`, and neither `Port`, `CatalogModels` nor `Routes::port` offers `with_session` | `llm-cli infer` and every `llm-models` caller always get a minted session; route fallback between two OpenCode Go targets gives one conversation two sessions |
| F3 | `checks/conformance/src/messages.rs:878` | CONFIRMED / introduced | the scenarios' `stable` fact reads the same accessor twice, so it is always true; it observes no request | the scenario claim "same on two requests" rests on the crate test `the_header_map_run_sends_carries_the_session_the_accessor_reports` |
| F4 | `crates/llm-messages/tests/opencode_session.rs:190` | INFEASIBLE / introduced | the only real-request test is for a non-OpenCode endpoint and checks absence; the OpenCode Go path (`client.rs:233`) is covered only by the private-builder test | a test can't reach `https://opencode.ai` locally: the transport ignores proxies and has no resolver override |

Attacked and not broken: every client in `llm-models` and `llm-cli` is built through `MessagesClient::new`, Bedrock never builds one; the transport passes headers unchanged and marks them sensitive; `MessagesClient` has no `Debug` and no error carries the value; retries and later turns reuse the client's one session; `with_session` on a non-OpenCode client adds no header; named sessions are printable ASCII up to 256 bytes; the base URL cannot break out of the conformance fixture catalog; the `Present` scenarios' header lists are unchanged; the endpoint path matches the live docs page fetched 2026-10-26; an `api_key_header` named `x-opencode-session` is overwritten on OpenCode Go, as `anthropic-version` already is.

```findings
[
  {"file": "crates/llm-messages/src/client.rs", "line": 56, "category": "boundary", "severity": "note", "verdict": "INFEASIBLE", "origin": "introduced", "message": "A trailing-dot host (opencode.ai.) or a percent-encoded path segment (%67o) reaches the same OpenCode Go endpoint but is not recognised, so no x-opencode-session is sent; no configuration found writes either form."},
  {"file": "crates/llm-models/src/lib.rs", "line": 288, "category": "judgement", "severity": "note", "verdict": "CONFIRMED", "origin": "introduced", "message": "with_session is unreachable through llm_models::Port, CatalogModels and Routes::port, so llm-cli and every llm-models caller always send a minted session, and route fallback between two OpenCode Go targets splits one conversation across two sessions."},
  {"file": "checks/conformance/src/messages.rs", "line": 878, "category": "judgement", "severity": "note", "verdict": "CONFIRMED", "origin": "introduced", "message": "The scenarios' stable fact compares two reads of an immutable accessor and cannot be false; the same-value-on-every-request claim rests on the crate test of wire_headers alone."},
  {"file": "crates/llm-messages/tests/opencode_session.rs", "line": 190, "category": "judgement", "severity": "note", "verdict": "INFEASIBLE", "origin": "introduced", "message": "No test sends a real request to an OpenCode Go client to see x-opencode-session on it; the only real-request test checks absence on another endpoint, and run's call to wire_headers is covered only through the private builder, because the transport cannot be pointed at a local socket for https://opencode.ai."}
]
```
