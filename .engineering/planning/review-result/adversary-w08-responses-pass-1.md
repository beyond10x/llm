---
format: aep.planning-md/3
id: review-result:adversary-w08-responses-pass-1
kind: review-result
status: active
title: Adversary, pass 1, story:responses-ingress-client-fields
relations:
- reviews: story:responses-ingress-client-fields
revision: 1
---
unit: story:responses-ingress-client-fields (worktree llm-w08-responses at 8e5ef6c9, base 602327b5)
verdict: green
cases: executed n/a (no build allowed), written 10
origin: introduced 5, pre-existing 0, undecided 0
wrote-outside-worktree: none
needs-coordinator: yes. Please compile and run `crates/llm-responses/tests/adversary_w08_fields.rs`. Also decide how acceptance's "effort `max` sent as `xhigh`" round trip should be read (finding 4).

**1. Diff.** `git --no-pager diff --stat` is empty. `git status --short` shows one untracked file, `?? crates/llm-responses/tests/adversary_w08_fields.rs`. I changed no implementation file and nothing under `.engineering/`.

**2. Cases written** (all in `~/.local/state/worktree/trees/b10x/llm/llm-w08-responses/crates/llm-responses/tests/adversary_w08_fields.rs`). None has run. From reading the code, I expect all 10 to pass at 8e5ef6c9. Seven of them fail if one line of the implementation is deleted, a deletion the unit's own suite does not notice.

| Case | What it asserts | Change it catches | Expected now |
|---|---|---|---|
| `an_id_on_a_function_call_alone_is_the_input_item_ids_hint` | an `id` only on the `function_call` → `unforwarded == {InputItemIds}` | deleting request.rs:721 | pass |
| `an_id_on_a_function_call_output_alone_is_the_input_item_ids_hint` | the same for `function_call_output` | deleting request.rs:743 | pass |
| `an_id_on_a_developer_message_alone_is_the_input_item_ids_hint` | the same for a developer message | deleting request.rs:675 | pass |
| `a_non_string_id_is_invalid_on_every_item_kind_that_carries_one` | an `id: 7` on a function_call, function_call_output, developer message or assistant message → `InvalidRequest` | the same 3 lines; the unit only pins a user message | pass |
| `a_key_on_a_developer_message_is_refused_by_its_path` | developer `status` key → `...: input[].status` | deleting request.rs:674 | pass |
| `an_empty_annotations_array_on_an_assistant_part_is_accepted_and_dropped` | `output_text` with `annotations: []` is accepted, and the turn round-trips without it | refusing every `annotations` key at :848; the unit tests only the refusals | pass |
| `a_reasoning_item_id_stays_in_the_payload_and_is_not_an_item_id_hint` | a reasoning `id` is kept in the payload and does not set `unforwarded` | adding `item_id` to the reasoning branch | pass |
| `a_developer_message_after_a_carried_reasoning_item_is_refused` | `[reasoning, developer, user]` → the standing-instruction refusal | an "after a message" check | pass |
| `joined_instructions_are_never_written_back_as_a_top_level_field` | egress writes no top-level `instructions` and no `client_metadata`, and sends one joined developer message | — | pass |
| `an_envelope_with_other_key_order_and_whitespace_is_still_the_envelope` | `" { \"output\": 3, \"ok\": false } "` → `output 3, failed true` | — | pass |

**3. Suite run.** Not run, because builds are banned for this pass. The coordinator runs it.

**4. Findings from reading** (they cover 8e5ef6c9):
- **Findings 1–2, CONFIRMED-BY-READING, warning, introduced.** At `crates/llm-responses/tests/client_fields.rs:623` and `:463`, removing any of request.rs:674, :675, :721 or :743 leaves the unit's suite and its scenarios green. The rules behind those lines come from the brief and the design. Codex sends ids on every item (the captures), so this path is reached. The cases in part 2 close the gap.
- **Finding 3, CONFIRMED-BY-READING, note, introduced.** At request.rs:848, nothing tests the accepted side of `annotations: []` on ingress. The case in part 2 covers it. I found nothing that shows a client sending it.
- **Finding 4, SUSPECTED, note, introduced, needs-coordinator.** Acceptance lists "effort `max` sent as `xhigh`" among round trips that must come back as an equal turn. The unit tests a body carrying `xhigh` round-tripping (`client_fields.rs:264`, scenario `ingress-round-trips-xhigh.yaml`). A body carrying `max` ingests as a turn with `max`, goes out as `xhigh` and comes back as a turn with `xhigh`, which is not equal. Both readings of the acceptance cannot hold at once, so I wrote no case. The unit's reading can be defended.
- **Finding 5, SUSPECTED, INFEASIBLE, note, introduced.** At request.rs:793 (`read_tool_result`), a client's successful plain-string output that happens to be exactly `{"ok":false,"output":…}` is read as a failed result. The spec's own R18 comment calls a flipped `failed` silent corruption. The design approves this rule, and I found nothing showing a client reaches it (Codex prefixes `exec_command` output with "Chunk ID:").

**5. Attacked without finding a break:**
- every unknown key is refused by its path at top level, `tool_choice`, `tools[]`, `reasoning` and `text`, and the `?` sanitising works;
- the names in REFUSED_TOOL_TYPES are copied from the list, not from the request;
- instructions join, including empty parts and multiple leading developer messages;
- `summary: null` is treated as absent;
- `encode_request` refuses a cache key that differs from the conversation, and sends an equal or absent key correctly;
- `wire_effort` refusals;
- `omitted_advisories` follows the design's order (enum order in `Advisory`, `BTreeSet<Unforwarded>`);
- the client warns only after `validate_for` succeeds;
- conformance `body_preserved` and `request_preserved` hold, checked by working through the 15 egress/ingress scenarios by hand;
- both fixtures are byte-identical to the captures;
- `ACCEPTED_BODY_FIELDS` is sorted;
- no older test still pins the old messages.

**6. Paths written outside the worktree:** none.

```findings
- file: crates/llm-responses/tests/client_fields.rs
  line: 623
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "CONFIRMED-BY-READING: removing item_id at request.rs:721 or :743 (function_call / function_call_output id) leaves the unit's suite green; covered by adversary_w08_fields.rs id cases"
- file: crates/llm-responses/tests/client_fields.rs
  line: 463
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "CONFIRMED-BY-READING: removing closed() or item_id() from developer_parts (request.rs:674-675) leaves the suite green; no test puts a key or id only on a developer message"
- file: crates/llm-responses/src/request.rs
  line: 848
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "CONFIRMED-BY-READING: nothing tests the accepted side of an empty annotations array on output_text, so refusing every annotations key would stay green"
- file: crates/llm-responses/tests/client_fields.rs
  line: 264
  category: acceptance
  severity: note
  verdict: NEEDS-CHANGE
  origin: introduced
  message: "SUSPECTED: acceptance lists effort max sent as xhigh among round trips to an equal turn, but a max body comes back as xhigh; the unit tests an xhigh body instead and the coordinator should settle the reading"
- file: crates/llm-responses/src/request.rs
  line: 793
  category: judgement
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: "SUSPECTED: a client's successful plain-string output that is exactly {\"ok\":false,\"output\":...} is read as failed, the flip spec R18 calls silent corruption; design-sanctioned and no client shown to reach it"
```
