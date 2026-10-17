---
format: aep.planning-md/3
id: review-result:adversary-w08-messages-pass-1
kind: review-result
status: active
title: Adversary, pass 1, story:messages-ingress-client-fields
relations:
- reviews: story:messages-ingress-client-fields
revision: 1
---
unit: story:messages-ingress-client-fields
verdict: red
cases: executed n/a (no build allowed), written 4
origin: introduced 2, pre-existing 0, undecided 0
write-outside-worktree: none
needs-coordinator: yes

These findings cover head 800d9c5e plus one untracked test file. Nothing was run, so every verdict below comes from reading the code.

**1. Diff of what I touched.** `git diff --stat` is empty. `git status` shows one untracked test file and nothing else:
`?? crates/llm-messages/tests/adversary_w08_fields.rs`

**2. Cases written** in `~/.local/state/worktree/trees/b10x/llm/llm-w08-messages/crates/llm-messages/tests/adversary_w08_fields.rs`:

| Case | What it asserts | Expected now |
|---|---|---|
| `a_request_without_cache_marks_round_trips_to_an_equal_turn` | A request with no `cache_control` anywhere goes through decode, `encode_request`, decode and comes back equal. It covers thinking (3 forms), `disable_parallel_tool_use` true/false and effort `max`. | **RED** (CONFIRMED-BY-READING) |
| `a_cache_mark_on_an_assistant_tool_use_block_is_the_prompt_cache_hint` | A `1h` mark on an assistant `tool_use` gives `prompt_cache = Some(1h)` | green; it catches a mutant the suite misses |
| `a_one_hour_mark_on_a_tool_alone_is_the_prompt_cache_hint` | A mark on a tool alone sets the hint | green; closes a coverage gap |
| `a_one_hour_tool_mark_beside_a_default_system_mark_is_two_lifetimes` | A tool marked `1h` beside a system block with no `ttl` is refused with `Messages cache marks carry more than one lifetime` | green; closes a coverage gap |

**3. Suite run:** none, because builds are not allowed in this pass. The coordinator runs it.

**4. Findings**

- **F1 (acceptance, warning, CONFIRMED-BY-READING, introduced).** The round trip of a request without marks is not equal.
  - **Measured:** `crates/llm-messages/src/codec.rs:147` and `:163` always place `{"type":"ephemeral"}` breakpoints. Since this unit, decode reads them back into a hint (`codec.rs:271`). So the first turn has `prompt_cache: None` and the second has `Some(PromptCache{ttl:None})`.
  - **Why the suite is green:** every round-trip test seeds marks into the input (`tests/client_request_fields.rs:48-50`). Every egress scenario seeds `"hints":{"prompt_cache":{}}`, for example `contracts/messages/scenarios/adaptive-thinking-is-projected-as-read.yaml`.
  - **What reaches it:** any Messages client that sends no `cache_control`. In production, though, the re-decoded projection is only reached by tests and conformance.
  - **Fix options (coordinator's call):** narrow the acceptance to "with marks", or document the projection's breakpoints as one-way the way the joined system is.
- **F2 (mutant, note, CONFIRMED-BY-READING, introduced).** A `cache_control` on `tool_use` is never exercised.
  - **Measured:** if `"tool_use"` is removed from the markable match at `codec.rs:345`, no case or scenario fails.
  - **What reaches it:** a client marking an assistant `tool_use`, which the design explicitly allows. My case 2 catches it.
- **F3 (judgement, note, CONFIRMED-BY-READING, introduced).** The unit edited `docs/verification/messages-falsification.json`, which is not in the brief's "yours" list.
  - **Why:** the edit appears forced, because `tests/falsification.rs:23` recomputes the diagnostics from the source.
  - **Needs-coordinator:** accept the edit or move it to a scratch patch.

**5. Attacked and could not break**

| Area | What I checked |
|---|---|
| Fixture | Byte-identical to the source body, checked with `cmp` |
| Two lifetimes | `5m` equals no lifetime; `1h` against either is refused |
| `thinking` | `display` and `budget_tokens` on adaptive/disabled refused by path |
| `system` | Joined with a line feed; an empty array decodes to empty instructions; a non-text block is refused |
| `metadata` | Value never kept; extra keys refused by path |
| `disable_parallel_tool_use` | Inverted correctly in both directions, including the `{"type":"auto"}` creation |
| Client warning | Emitted after `finish`, absent on a failed turn |
| Omitted advisories | Order matches the design's table |
| Wire-name doc count | 97 = 73 + 24, matches |

**6. Paths written outside the worktree:** none.

```findings
- file: crates/llm-messages/src/codec.rs
  line: 271
  category: acceptance
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: a request carrying no cache marks does not round-trip to an equal turn, because the projection always places breakpoints and ingress now reads them back as a prompt-cache hint; the unit's tests and egress scenarios hide this by seeding marks
- file: crates/llm-messages/src/codec.rs
  line: 345
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: no case or scenario puts a cache mark on a tool_use block, so dropping tool_use from the ingress markable set stays green
- file: docs/verification/messages-falsification.json
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the unit edited a file outside its brief's file assignment, apparently forced by tests/falsification.rs, and did not name it as a deviation
```
