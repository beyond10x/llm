---
format: aep.planning-md/3
id: review-result:adversary-w22-llm-parity-credential-sources-pass-1
kind: review-result
status: archived
title: Wave 2026-10-05-w22 adversary, llm story:parity-credential-sources, pass 1
relations:
- reviews: story:parity-credential-sources
revision: 2
transitions:
- {from: "active", to: "archived", at: "2026-10-08T10:43:37Z", actor: "human:timo", revision: 2}
---
```
unit: llm/parity-credential-sources, phase 1 7c737519 plus the uncommitted phase 2 in ~/.local/state/worktree/trees/b10x/llm/llm-w22-parity-credential-sources
verdict: green (no case red against the real code; 1 NEEDS-CHANGE doc finding)
cases: executed 609→615, red 0 (4 new cases go red on mutated copies)
origin: introduced 7 / pre-existing 0 / undecided 0
wrote-outside-worktree: 8 logs under ~/.cache/ga-wave-2026-10-05-w22/llm-parity-credential-sources/scratch/
needs-coordinator: yes (C8 status; keep the two adversary test files)
```

Cases added: `crates/llm-credentials/tests/adversary_pointer.rs` (differential against `serde_json::Value::pointer`; empty pointer; 4096 binding limit; 500k-deep document) and `crates/llm-providers/tests/adversary_line_terminator.rs` (every terminator combination but one trailing refused; size bound on the presented token). Mutants caught: `~0` decoded before `~1`, empty pointer refused, limit removed. Conformance is not vacuous: mutant set A fails exactly the 5 targeted scenarios (717 total, 712 passed); mutant B (`ReferenceError` Display without the reference) fails 9.

Suite after the cases: `cargo test --workspace --locked --no-fail-fast` exit 0, 615 passed; credentials all features 50 passed.

| file:line | finding | verdict / origin | what reaches it |
|---|---|---|---|
| docs/harness-parity.md:79 | C8 marked `not needed` on an open decision-blocker that says not to silently remove subscription coverage; dropped from Gaps while M5/M6 stay gap. | NEEDS-CHANGE / introduced | the store |
| crates/llm-credentials/src/pointer.rs:83 | `~0`-before-`~1` decoding survives the unit's suite. | CONFIRMED / introduced | caught by adversary_pointer.rs |
| crates/llm-credentials/src/pointer.rs:66 | refusing the empty pointer survives the unit's suite. | CONFIRMED / introduced | same |
| crates/llm-credentials/src/pointer.rs:36 | removing the 4096-binding limit survives the unit's suite. | CONFIRMED / introduced | same |
| docs/harness-parity.md:72 | C1 covered although Harness's cited input (leading spaces plus newline) is refused by llm; no decision cited. | CONFIRMED / introduced | the row |
| crates/llm-credentials/src/file.rs:48 | `read`/`ReferenceError` has no non-test caller; `prepare_auth` uses `resolve`, so the refusal an operator sees does not name the reference. | CONFIRMED / introduced | grep |
| crates/llm-routing/src/fallback.rs:141 | a non-string or non-JSON pointer document becomes `Unavailable`, which fallback treats as eligible; Harness refuses `Unauthorized` without fallback. Read, not traced. | CONFIRMED / introduced | spec scenario `pointer-non-string` with `eligible()` |

Could not break: no value, path or variable name in Debug/Display; `~2`, trailing `~`, leading-zero, `-`, `+1` indices; non-string values refused; duplicate keys; 500k nesting; raw non-UTF-8 environment values refused at presentation; every terminator variant; each feature alone with `--no-default-features`; every parity citation and the 87/32/20/7 counts.

```findings
[
{"file":"docs/harness-parity.md","line":79,"category":"acceptance","severity":"warning","verdict":"NEEDS-CHANGE","origin":"introduced","message":"C8 is marked not needed and dropped from Gaps on the strength of an open decision-blocker that forbids silently removing subscription coverage, while M5/M6 stay gap for the same presentation"},
{"file":"crates/llm-credentials/src/pointer.rs","line":83,"category":"mutant","severity":"warning","verdict":"CONFIRMED","origin":"introduced","message":"decoding ~0 before ~1 (so ~01 becomes /) survives the unit's suite; adversary_pointer.rs differential test catches it"},
{"file":"crates/llm-credentials/src/pointer.rs","line":66,"category":"mutant","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"refusing the documented-valid empty pointer at construction survives the unit's suite"},
{"file":"crates/llm-credentials/src/pointer.rs","line":36,"category":"mutant","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"removing the documented 4096-binding TooManyReferences check survives the unit's suite"},
{"file":"docs/harness-parity.md","line":72,"category":"acceptance","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"C1 is covered although the input of Harness's own cited test (leading spaces plus newline) is refused by llm, and the divergence cites no decision"},
{"file":"crates/llm-credentials/src/file.rs","line":48,"category":"judgement","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"ReferenceError-returning read has no non-test caller; prepare_auth uses resolve, so the refusal an operator sees still does not name the reference"},
{"file":"crates/llm-routing/src/fallback.rs","line":141,"category":"judgement","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"non-string or non-JSON pointer documents map to Unavailable, which the router treats as fallback-eligible, where Harness refuses Unauthorized without fallback"}
]
```
