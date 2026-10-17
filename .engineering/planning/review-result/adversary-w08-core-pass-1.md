---
format: aep.planning-md/3
id: review-result:adversary-w08-core-pass-1
kind: review-result
status: active
title: Adversary, pass 1, story:neutral-turn-controls
relations:
- reviews: story:neutral-turn-controls
revision: 1
---
unit: story:neutral-turn-controls
verdict: red
cases: executed 42→45, red 3
origin: introduced 2, pre-existing 0, undecided 0
wrote-outside-worktree: ~/.cache/llm-w08/core/adversary/suite.log, ~/.cache/llm-w08/core/adversary/suite-nff.log
needs-coordinator: no

**Covers:** the 602327b5 tree plus my one untracked test file. Both findings are serde read-side gaps. Both are INFEASIBLE because no code in the repository deserializes a `TurnDocument` or a `Thinking` from outside input (grep of `crates/*/src`).

**1. Diff.** `git diff --stat` is empty. `git status --short` shows only `?? crates/llm-core/tests/adversary_w08_controls.rs`, which is a test file. No implementation file was touched.

**2. Cases added**, in `crates/llm-core/tests/adversary_w08_controls.rs`. All three are red now. The red output below came from running this file alone, before the suite ran. rustfmt reflowed the file afterwards, so the panic lines 20/41/54 are now 23/44/56.

| Case | Asserts | Red output |
|---|---|---|
| `adaptive_thinking_carrying_a_budget_is_refused` | `{"mode":"adaptive","budget_tokens":2048}` is refused, both as a bare `Thinking` and inside an `llm.turn/4` document | `a budget beside adaptive thinking was read as Ok(Adaptive)` |
| `disabled_thinking_carrying_a_budget_or_an_unknown_key_is_refused` | `/4` with `{"mode":"disabled","budget_tokens":0}`, or with an unknown key `tone`, is refused | `{"budget_tokens":0,"mode":"disabled"} read as Ok(TurnDocument { ... thinking: Some(Disabled) ... })` |
| `an_llm_turn_3_document_carrying_a_controls_key_is_refused` | `/3` carrying `controls: {}`, `{"hints":{}}` or all-null fields is refused | `llm.turn/3 with controls {} was read; llm 0.6.0 refuses it as an unknown field` |

**3. Gate runs**, after the cases existed:

| Command | Result |
|---|---|
| `cargo test -p b10x-llm-core` | exit 101; stops at the first failing binary, `adversary_w08_controls` (0 passed, 3 failed) |
| `cargo test -p b10x-llm-core --no-fail-fast` | exit 101; 45 ran, 42 passed, 3 failed. Without my file: 42 (lib 6, adversary_wave3_opaque 3, controls 14, dependency_boundary 1, embedding 12, observations 2, retry_class 4) |
| `cargo clippy -p b10x-llm-core --all-targets -- -D warnings` | clean, `Finished` with no warnings |

**4. Findings**

| # | file:line | Verdict / origin | What was measured | What reaches it |
|---|---|---|---|---|
| F1 | `crates/llm-core/src/controls.rs:19` | INFEASIBLE / introduced | Serde ignores extra keys on the internally tagged unit variants `Adaptive` and `Disabled`, even with `deny_unknown_fields`. That breaks the unit's spec ("budget_tokens: present exactly when the mode is Enabled", `spec/domains/inference.yaml`) and the `TurnDocument` doc ("Unknown … fields refuse deserialization") | Nothing found: only tests read `TurnDocument`. Fix: decode `Thinking` via `try_from` a raw struct `{mode, budget_tokens: Option<u64>}` with `deny_unknown_fields` |
| F2 | `crates/llm-core/src/turn.rs:486` | INFEASIBLE / introduced | `try_from` compares the version with `controls.is_empty()`, so a `/3` document that carries a `controls` key holding nothing is accepted. The design says "a `/3` document carrying controls is refused". At the base, 8c0aab4e, `TurnRequest` had `deny_unknown_fields` and no `controls` field, so 0.6.0 refuses this document and 0.7.0 reads it | Nothing found: the writer never emits it. Fix: give the raw document `controls: Option<Controls>` and refuse `/3` when it is `Some` |

**5. Attacked and not broken**
- `wire_effort`: every row of the table on all 3 wires; `max` becomes `xhigh`; disabled thinking with effort `none` gives `none`.
- `validate_for`: `max` against bindings listing `xhigh`, listing `max`, or listing only `xhigh` on Messages.
- `validate`: cache key of 512 bytes accepted, 513 refused, empty refused; zero budget refused; parallel tool calls without tools refused.
- `omitted_by` / `advisories`: order is the Advisory order, including the unforwarded set.
- `advisory_warning`: returns nothing for no omission and names each omitted hint otherwise.
- `field_path`: same rule as `path` in `crates/llm-messages/src/lib.rs:65`.
- 0.6.0 fixture: field order matches the base `TurnRequest`.
- ESS enum casing differs from serde (`FiveMinutes` vs `5m`). That was already the repository's convention (`llm.catalog.Protocol`), so it is not raised.

**6. Paths written outside the worktree:** `~/.cache/llm-w08/core/adversary/suite.log` and `~/.cache/llm-w08/core/adversary/suite-nff.log`, both in the assigned scratch directory. Build output went to the tree's own `target/`. I held my own session lease (`adversary-w08-core-controls`) while working and released it before this report.

```findings
- file: crates/llm-core/src/controls.rs
  line: 19
  category: contract-drift
  severity: warning
  verdict: INFEASIBLE
  origin: introduced
  message: serde ignores extra keys on the internally tagged unit variants Adaptive and Disabled, so a budget_tokens or unknown key beside them is accepted, against inference.yaml and the TurnDocument strict-decoding promise
- file: crates/llm-core/src/turn.rs
  line: 486
  category: contract-drift
  severity: warning
  verdict: INFEASIBLE
  origin: introduced
  message: an llm.turn/3 document carrying an empty or all-null controls key is accepted, though the design refuses /3 with controls and llm 0.6.0 refuses the same bytes
```
