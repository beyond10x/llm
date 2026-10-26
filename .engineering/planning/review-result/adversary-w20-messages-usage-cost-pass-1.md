---
format: aep.planning-md/3
id: review-result:adversary-w20-messages-usage-cost-pass-1
kind: review-result
status: active
title: Adversary, wave 2026-10-26-w20, story:messages-usage-cost, pass 1
relations:
- reviews: story:messages-usage-cost
revision: 1
---
unit: messages-usage-cost
verdict: red
cases: executed 1314→1323, red 1
origin: introduced 3, pre-existing 0, undecided 0
wrote-outside-worktree: scratch root of the wave (6 entries, listed in part 6) | none in other places
needs-coordinator: no

The verdict is red because two cases fail. One fails in every build. The other fails only when `llm-messages` is built without the workspace. The workspace suite, which `task check` runs, therefore shows 1 red. The before count is 1323 minus my 9 cases, taken from the same run. No other run produced it.

**1. Diff stat** (`39346593..289a17f0`): only a test file changed.
```
 .../llm-messages/tests/adversary_w20_usage_cost.rs | 440 +++++++++++++++++++++
```

**2. Cases added** (file `crates/llm-messages/tests/adversary_w20_usage_cost.rs`, 9 cases)

| Case | Asserts | Now |
|---|---|---|
| `a_decimal_cost_wider_than_a_double_is_carried_as_the_same_value` | a cost written with more digits than an `f64` holds is carried as written, as the docs promise ("the same JSON value") | red in every build |
| `a_shortest_f64_cost_is_carried_as_the_same_number` | a cost a producer wrote as the shortest text of an `f64` comes back as the same number | red with `-p llm-messages`, green with `--workspace` |
| `a_delta_refused_for_its_stop_reason_shows_no_cost` | a delta refused for its stop reason (empty, or a second one) shows no warning | green; it catches a code change the unit's suite misses (below) |
| 6 probes: the 1025-byte bound, UTF-8 byte counting, `cost` refused by path at 6 places outside the usage object, a held start cost against 5 stream error types, a cut before answering through `MessagesClient`, a held cost shown before a preserved unknown event | — | green |

Red output, from running the case file alone with `cargo test -p llm-messages --test adversary_w20_usage_cost`, exit 101:
```
left: "message_delta.usage.cost: 0.00042"
right: "message_delta.usage.cost: 0.000420000000000000001"
the cost 0.009027474764568267 was carried as 0.009027474764568269: not the same JSON value
```
- **Why the second case depends on the build:** with `--workspace`, `ess-primitives` switches on `serde_json`'s `float_roundtrip` feature. `llm-messages` does not enable it itself.
- **How often it bites without that feature** (a throwaway Rust probe with `serde_json` 1.0.151): 230,167 of 2,000,000 random shortest-text costs between 1e-7 and 1e2 came back as a different number. With the feature on, none did.
- **The missed code change:** in a scratch copy I moved the warning so it is emitted before the stop reason is checked. Against that copy, `usage_extension` (11), `streaming` (47) and `projection` (21) all still pass, exit 0. My new case fails.

**3. Suite runs** (after the cases existed)
- `cargo test --workspace --locked --no-fail-fast`: exit 101; 1322 passed, 1 failed (the decimal case), 2 ignored.
- `cargo test -p llm-messages --locked --no-fail-fast`: exit 101; 239 passed, 2 failed.
- I reformatted the file afterwards for `fmt` and clippy without changing what it tests, then ran the workspace suite again: same result.
- `cargo fmt --check` exit 0; clippy on `-p llm-messages --tests` exit 0.

**4. Findings** (they cover commit `289a17f0`, which is `39346593` plus the test file)

| id | Verdict | What reaches it |
|---|---|---|
| f1, decimal wider than an `f64` (`crates/llm-messages/src/usage.rs:169`; docs `docs/messages.md:33`, `:228`) | INFEASIBLE | No producer shown to write over 17 significant digits; the cost's real shape is unknown |
| f2, depends on `float_roundtrip` (`usage.rs:169`) | INFEASIBLE | The harness CLI gets `float_roundtrip` through `jsonschema` and `ess-primitives`, checked on the primary checkout, which pins llm 0.8.1. For the 0.20.0 build the trial ran: I don't know |
| f3, stop-reason refusal not covered (`crates/llm-messages/src/decode.rs:672`) | CONFIRMED | The unit's suite; a test gap, not a code defect |
| f4, `the_warning_code_is_the_public_constant` only pins a constant | CONFIRMED | Test quality only |

Suggested fixes, not applied:
- f1: say in the docs that numbers are carried as `f64`.
- f2: enable `float_roundtrip` on `llm-messages`' own `serde_json` dependency.

**5. What I attacked and could not break:**
- **Path refusals:** `cost` stays refused, by its path, under `cache_creation`, `output_tokens_details` and `server_tool_use`, inside `delta`, at `message_delta` level and at `message.cost`. `decode_message` refuses it too.
- **Transactional and held warnings:** a refused usage object shows no warning, and a held one is not released after a refusal.
- **Retries:** a held cost leaves overloaded, api, rate-limit, timeout and invalid-request errors exactly as retriable as without it. A cut through the client stays retriable, and fallback counting sees nothing.
- **Bound:** 1024 bytes is carried and 1025 is refused, counted in UTF-8 bytes.
- **Order:** a held cost is shown before a preserved unknown event.
- **By reading only:** the Bedrock invoke stream (an exception before answering is handled before the decoder runs), and the ingress encoder (drops warnings, so a gateway relaying the stream never passes the cost on).

**6. Paths written outside the worktree** (all under the wave's scratch root, `adversary/`):
- `probe/` (26M, Rust float probe with its `target/`)
- `mutant/` (485M, mutated source copy with its `target/`)
- `commit-msg.Zep7iC`
- `suite-workspace.log`, `suite-workspace-2.log`, `suite-workspace-3.log`, `suite-llm-messages.log`

Neither `target/` is deleted.

**7. Findings block**
```findings
[
  {"file": "crates/llm-messages/src/usage.rs", "line": 169, "category": "acceptance", "severity": "warning", "verdict": "INFEASIBLE", "origin": "introduced", "message": "A cost with more significant digits than an f64 holds is carried as a different number, which contradicts the promise in docs/messages.md that it is carried as the same JSON value (test adversary_w20_a_decimal_cost_wider_than_a_double_is_carried_as_the_same_value)."},
  {"file": "crates/llm-messages/src/usage.rs", "line": 169, "category": "property", "severity": "warning", "verdict": "INFEASIBLE", "origin": "introduced", "message": "llm-messages does not enable serde_json float_roundtrip, so without it about 11.5% of costs written as the shortest text of an f64 come back as a neighbouring number; the workspace suite cannot see this because feature unification turns the feature on there (test adversary_w20_a_shortest_f64_cost_is_carried_as_the_same_number)."},
  {"file": "crates/llm-messages/src/decode.rs", "line": 672, "category": "mutant", "severity": "warning", "verdict": "CONFIRMED", "origin": "introduced", "message": "The unit's suite stays green when the delta's cost warning is emitted before the stop reason is checked; the new case catches that change (test adversary_w20_a_delta_refused_for_its_stop_reason_shows_no_cost)."},
  {"file": "crates/llm-messages/tests/usage_extension.rs", "line": 343, "category": "judgement", "severity": "note", "verdict": "CONFIRMED", "origin": "introduced", "message": "the_warning_code_is_the_public_constant only checks a constant's value and tests no behaviour (test the_warning_code_is_the_public_constant)."}
]
```

Commit `289a17f0d2401ddf402dd1ac4695e20e1005e24b`, author and committer `b10x-bot[bot]`, not pushed.
