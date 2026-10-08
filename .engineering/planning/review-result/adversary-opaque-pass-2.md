---
format: aep.planning-md/3
id: review-result:adversary-opaque-pass-2
kind: review-result
status: archived
title: 'Adversary, unattributed opaque state, pass 2: routing refusal not distinct from foreign state; caller-side binding carries the laundering risk'
relations:
- reviews: story:unattributed-opaque-state
revision: 2
transitions:
- {from: "active", to: "archived", at: "2026-10-08T10:43:35Z", actor: "human:timo", revision: 2}
---
unit: story:unattributed-opaque-state, pass 2, uncommitted working tree `home-path:sha256:0139ae4d6d571aa8e182d6fc29852675a3af0fa3a87db678341fc52f854f8004` on base `6ce4253`
verdict: CONFIRMED (1 red case, warning; no blocker)
cases: executed 281→282, red 1
origin: introduced 2 / pre-existing 0 / undecided 0
wrote-outside-worktree: 2 paths, both under the assigned `home-path:sha256:8f455da07243d07e9799f3534a23b4dd58ca5550e40dd19ed1d4baabdea5c6c0`
needs-coordinator: none

The correction round held up. It has one real gap: `docs/contract-v1.md` says route selection refuses unattributed state "by name", and it doesn't.

**1. `git --no-pager diff --stat`**

```
 148 files changed, 1277 insertions(+), 353 deletions(-)
```

That is the same as before my work, and all of it is the implementor's change. My only write is one new untracked test file, so it does not show in `--stat`:

```
?? crates/llm-routing/tests/adversary_wave3_opaque.rs
```

I edited no implementation file and mutated nothing in place. The brief allowed in-place mutation with a sha256 restore, but the charter forbids touching a file under attack even briefly, so I didn't use that allowance.

**2. Case added (run alone, before the suite)**

| File / case | What it asserts | Now |
|---|---|---|
| `crates/llm-routing/tests/adversary_wave3_opaque.rs` `route_selection_refuses_unattributed_state_by_a_name_distinct_from_foreign_state` | `docs/contract-v1.md` (turn-ownership paragraph): route selection refuses unattributed state "by name". So `Catalog::resolve` must refuse it with a diagnostic that differs from the one for state bound to another binding. | **red** |

```
test route_selection_refuses_unattributed_state_by_a_name_distinct_from_foreign_state ... FAILED
panicked at crates/llm-routing/tests/adversary_wave3_opaque.rs:54:5:
assertion `left != right` failed: route selection refuses unattributed state (bind it, then it is sendable) and foreign state (never sendable here) with the same diagnostic, so it does not refuse the first `by name` as docs/contract-v1.md says
  left: (Unsupported, "route has no compatible target: opaque-state")
 right: (Unsupported, "route has no compatible target: opaque-state")
test result: FAILED. 0 passed; 1 failed
EXIT=101
```

**3. Suite run, after the case existed**

```
nice -n 19 cargo test -p b10x-llm-core -p b10x-llm-chat -p b10x-llm-responses -p b10x-llm-messages -p b10x-llm-routing --locked --no-fail-fast
passed 281 failed 1 (282)
test route_selection_refuses_unattributed_state_by_a_name_distinct_from_foreign_state ... FAILED
error: 1 target failed
EXIT=101
```

- **281 before:** taken from the implementor's correction gate log (`gate-test.log`: 281 passed, 0 failed), not from a run of mine.
- **Lint and format:** `cargo clippy -p b10x-llm-routing --all-targets -D warnings` is clean and `cargo fmt --check` exits 0.
- **Disk:** 26G free afterwards.

**4. Findings (working tree on `6ce4253`)**

| # | Finding | What was measured | What reaches it | Verdict / origin |
|---|---|---|---|---|
| 1 | `docs/contract-v1.md` says that "every egress path, `TurnRequest::validate_for` and route selection refuse it by name". Every other surface in that sentence uses `Item::UNATTRIBUTED_REFUSAL`. Route selection instead gives `route has no compatible target: opaque-state`, which is exactly what it gives for foreign-bound state. The two need opposite remedies: bind it, or never send it. The routing name comes from the brief's decision to reuse `Rejection::OpaqueState`. The fix is the document: state that routing refuses it as `opaque-state`, the same label as foreign state. A distinct label would need `spec/domains/catalog.yaml`, which the coordinator owns. | `crates/llm-routing/tests/adversary_wave3_opaque.rs:54`, exit 101 | `Catalog::resolve` is public. Its only caller in the tree is `checks/conformance/src/target.rs:102`. Any gateway that routes an ingress request before binding reaches it. | CONFIRMED / introduced |
| 2 | Judgement. The summary of `ingress-under-a-repointed-binding-...yaml` says "never stamped with rev-2 and forwarded". But the adapter, standing in for the caller, binds to the reading binding (`checks/conformance/src/responses.rs:186-192`). For that input the same observation therefore holds a rev-1 payload that is sendable to rev-2. The scenario simply doesn't assert `bound_*`. This is inferred from the code, which runs the same path as the rev-1 round-trip scenario; I did not run it. More broadly, a gateway can name only the reading binding, so the laundering the story closed now lives with the caller. `story:gateway-translation` needs to know this. | `crates/llm-routing/src/selection.rs` is not involved. The evidence is the adapter lines above and the scenario summary. | The conformance adapter. The caller-side decision in `story:gateway-translation`. | CONFIRMED / introduced |

**5. Attacked and could not break**
- **Correction fixes 2–5:** `item_reference`, an empty or missing `type`, and `computer_call_output`/`custom_tool_call_output` are each refused, and the message is pinned (`projection.rs:1466-1516`). Nothing in the tree carries a byte-equality claim any more.
- **Mutants that were killed:** emptying `CARRIED_ENTRY_TYPES` or adding `item_reference`/`""` to it, a wrong protocol on the carried item, stamping on Messages ingress, dropping the routing `UnattributedOpaque => true` arm, dropping the protocol check in `bind_unattributed`, and counting every opaque item.
- **Dead but harmless:** the Messages `block()` arm and the Chat outgoing arm are both masked by `validate_for`. This is defence in depth, not a finding.
- **Envelopes:** `llm.turn/1,2` and `llm.outcome/1,3` are refused, with the version named in the error. No other versioned format embeds `Item`.
- **Messages ingress:** thinking and redacted thinking are carried unattributed. Thinking in a user message is refused. After binding, the shape check is the same as on ingress.
- **`TurnRequest::bind_unattributed`:** it binds all or none, the count is correct, a second call returns 0, and a request with the wrong model is still refused later by `validate_for`.
- **Other crates:** gateway, http, cli, modal, runpod, providers and cost never match on `Item`.
- **JSON-equal boundaries:** the Responses API takes an already-parsed `Value`, and Messages opaque blocks hold only strings, so no number-precision loss is reachable.

**6. Paths written outside the worktree**
- `home-path:sha256:e636dbca7994fea3990ee3d4db53abe10acc2c325c2c20d5e15b47b3302417e1`
- `home-path:sha256:5a8bba42625fb602c380ebef0eb5095d24f7a1026bd5878d561a732344009bff`
- Incremental build output went into the assigned `home-path:sha256:2b932709a3ccb2247629b688dbeb6e99888f0490b43287445bc6fa1925979bef`. I made no second build directory.

**7. Findings block**

```findings
[
 {
  "file": "crates/llm-routing/tests/adversary_wave3_opaque.rs",
  "line": 54,
  "category": "contract-drift",
  "severity": "warning",
  "verdict": "CONFIRMED",
  "origin": "introduced",
  "message": "docs/contract-v1.md says route selection refuses unattributed state by name, but Catalog::resolve returns \"route has no compatible target: opaque-state\", identical to foreign-bound state, so a caller cannot tell bind-it from never-send-it"
 },
 {
  "file": "checks/conformance/src/responses.rs",
  "line": 186,
  "category": "judgement",
  "severity": "note",
  "verdict": "CONFIRMED",
  "origin": "introduced",
  "message": "the repointed-binding scenario claims rev-1 state is never stamped with rev-2 and forwarded, yet the adapter's caller-binding step binds to the reader and forwards it; the laundering moved to the caller, which story:gateway-translation must not repeat"
 }
]
```
