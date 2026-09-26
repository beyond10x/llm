---
format: aep.planning-md/1
id: review-result:adversary-opaque-pass-1
kind: review-result
status: active
title: 'Adversary, unattributed opaque state, pass 1: byte-for-byte claim false, item references and empty types carried'
relations:
- reviews: story:unattributed-opaque-state
revision: 1
---
unit: story:unattributed-opaque-state, the uncommitted working tree at `/home/timo/.local/state/worktree/trees/b10x/llm/wave3-opaque` on base `6ce4253`
verdict: CONFIRMED (3 red cases, 2 of them warnings; no blocker)
cases: executed 272→278, red 3
origin: introduced 5, pre-existing 0, undecided 0
wrote-outside-worktree: 4 files, all under the assigned `/home/timo/.cache/llm-wave-3/opaque/adversary-1/`
needs-coordinator: none

**1. `git --no-pager diff --stat`**

```
 147 files changed, 1034 insertions(+), 351 deletions(-)
```

That line is the same before and after my work. All 147 paths are the implementor's uncommitted change. My only writes are three new, untracked test files, so they do not appear in `diff --stat`:

```
?? crates/llm-core/tests/adversary_wave3_opaque.rs
?? crates/llm-messages/tests/adversary_wave3_opaque.rs
?? crates/llm-responses/tests/adversary_wave3_opaque.rs
```

I changed no implementation file and ran no mutation in place.

**2. Cases added. Each file was run alone, red output below is verbatim.**

| File / case | Asserts | Now |
|---|---|---|
| messages `a_bound_thinking_block_goes_out_exactly_as_it_came_in` | `docs/messages.md:28-30`: a thinking block that comes in as raw bytes and is bound goes out as the same bytes | **red** |
| responses `a_reference_to_provider_stored_state_is_still_refused_on_ingress` | an `item_reference` entry is refused (`docs/responses.md:43`, "replayed whole"; the doc comment on `ingest_request` refuses "provider-stored conversations") | **red** |
| responses `an_entry_whose_type_is_empty_names_no_type_and_is_refused` | `docs/responses.md:90-91`: an entry that names no type is refused | **red** |
| core `a_refused_binding_leaves_no_earlier_item_bound` | "all or none", with a bindable item before a refused one (the unit's own case has only one item) | green, catches an in-place partial-bind mutant |
| core `the_count_is_of_unattributed_items_only_and_bound_state_is_untouched` | the count is 2 with one foreign bound item present; that item is unchanged; a second call returns 0 | green, catches a mutant that counts every opaque item |
| core `an_unattributed_item_carrying_a_provenance_field_is_refused` | a `llm.turn/3` document cannot put a `provenance` field on the unattributed variant | green |

```
thread 'a_bound_thinking_block_goes_out_exactly_as_it_came_in' panicked at crates/llm-messages/tests/adversary_wave3_opaque.rs:32:5:
the block that came in as {"type":"thinking","thinking":"weighing","signature":"sig-1"} went out as: {"max_tokens":512,"messages":[{"content":[{"text":"Summarise the log","type":"text"}],"role":"user"},{"content":[{"signature":"sig-1","thinking":"weighing","type":"thinking"}],"role":"assistant"},{"content":[{"text":"go on","type":"text"}],"role":"user"}],"model":"example/Model-Revision","stream":true}
test result: FAILED. 0 passed; 1 failed
EXIT=101

thread 'a_reference_to_provider_stored_state_is_still_refused_on_ingress' panicked at crates/llm-responses/tests/adversary_wave3_opaque.rs:53:6:
a provider-stored conversation reference is refused, not carried: [UserText { text: "Hi" }, UnattributedOpaque { protocol: Responses, payload: Object {"id": String("msg_stored_elsewhere"), "type": String("item_reference")} }]
thread 'an_entry_whose_type_is_empty_names_no_type_and_is_refused' panicked at crates/llm-responses/tests/adversary_wave3_opaque.rs:64:10:
an empty type names no type: [UserText { text: "Hi" }, UnattributedOpaque { protocol: Responses, payload: Object {"id": String("rs_1"), "type": String("")} }]
test result: FAILED. 0 passed; 2 failed
EXIT=101

core: test result: ok. 3 passed; 0 failed   EXIT=0
```

**3. Suite run, after the cases existed**

```
nice -n 19 cargo test -p b10x-llm-core -p b10x-llm-chat -p b10x-llm-responses -p b10x-llm-messages -p b10x-llm-routing --locked --no-fail-fast
passed 275 failed 3   (278 cases)
test a_bound_thinking_block_goes_out_exactly_as_it_came_in ... FAILED
test a_reference_to_provider_stored_state_is_still_refused_on_ingress ... FAILED
test an_entry_whose_type_is_empty_names_no_type_and_is_refused ... FAILED
error: 2 targets failed:
EXIT=101
```

- **272 before:** read from the implementor's gate log (`/home/timo/.cache/llm-wave-3/opaque/gate-test.log`, 272 passed, 0 failed), not from a run of mine.
- **`--no-fail-fast`:** added so that every crate reports its count despite the failures.
- **Lint and format:** clippy `-D warnings` on the three crates I touched is clean, and `cargo fmt --check` exits 0.

**4. Findings. They cover the working tree at base `6ce4253`.**

| # | Finding | What was measured | What reaches it | Verdict / origin |
|---|---|---|---|---|
| 1 | "Byte for byte" and "exactly as it came in" are false for bytes. The payload is kept as a `serde_json::Value`, whose keys come out sorted. The unit's own round-trip checks compare two re-serialised `Value`s, so key order is invisible to them. The Responses scenario `a-gateway-round-trip-preserves-the-payload-once-the-caller-binds-it` says "byte for byte" in its summary, yet its own `bound_input_json` shows the keys reordered compared with `body_json`. | `crates/llm-messages/tests/adversary_wave3_opaque.rs:32`, exit 101 | `decode_request` takes raw client bytes; any client whose key order is not alphabetical hits it. No caller of `decode_request` in this repository. The JSON value is preserved, so the harm is the false claim. Suggested fix: say "JSON-equal" in `docs/messages.md:28-30`, `docs/responses.md:64,87-89`, `item.rs:35` and the scenario summaries. Keeping real bytes would need the `raw_value` feature in the workspace `Cargo.toml`, which is not the unit's file. | CONFIRMED / introduced |
| 2 | Any entry with a `type` string (`request.rs:465`) becomes continuation state, including `item_reference`, a pointer to an item the provider stored. At base the catch-all refused it (`git show 6ce4253:…request.rs`). Now it is carried, and one `bind_unattributed` call makes it sendable with `store:false`. That contradicts "replayed whole every turn". | `crates/llm-responses/tests/adversary_wave3_opaque.rs:53`, exit 101 | The public `ingest_request` API with a client-supplied body. No in-tree caller. Suggested fix: refuse `item_reference` by name in `input_to_item`. | CONFIRMED / introduced |
| 3 | `Some(_)` accepts `"type": ""`; the docs say an entry that names no type is refused. | `crates/llm-responses/tests/adversary_wave3_opaque.rs:64`, exit 101 | Nothing found that mints an empty type; I built this state myself. | INFEASIBLE / introduced |
| 4 | Judgement: the same `Some(_)` arm treats client-authored entries as model-minted state, e.g. hosted-tool outputs such as `computer_call_output`. Once bound, they skip the tool pairing and result-size checks in `TurnRequest::validate`. The ingress doc comment still claims it refuses "content this version does not carry". | `request.rs:465`, `request.rs:108` | The public `ingest_request` API. No in-tree caller. | CONFIRMED / introduced |
| 5 | Judgement: the rewritten adversary case checks only `encode_request(..).is_err()`, so any error passes. The exact refusal message is pinned only in `projection.rs`. | `crates/llm-messages/tests/adversary_codec.rs:84` | Only the test suite itself. | CONFIRMED / introduced |

**5. What I attacked and could not break**
- **Envelope versions:** `llm.turn/2` and `llm.outcome/3` are refused, and the error names the version. The old literals remain only in refusal scenarios, the coordinator-owned `contracts/suite.json`, `docs/plan` and `docs/verification`.
- **Ingress never binds:** no ingress path creates bound opaque state. Messages passes `None` (`codec.rs:229`), and Responses `input_to_item` has no `Item::Opaque` constructor.
- **Unbound state is refused on every exit:** `validate_for`, Responses `project_request`, Messages `block`, Chat outgoing, Chat ingress encoding and routing (every target rejects) all refuse unbound state. `TurnOutcome::validate_for` refuses it too.
- **Binding:** the protocol check holds, already-bound opaque state is never rebound, binding is all-or-none, and the count is correct.
- **No smuggling:** a document cannot attach a `provenance` to the unattributed variant, and the payload does not leak into the routing explanation.
- **Mutants:** removing the arm in `validate_for`, the Responses egress arm, the arm in `TurnOutcome` or the protocol check is each killed by an existing case.

**6. Paths written outside the worktree**
- `/home/timo/.cache/llm-wave-3/opaque/adversary-1/red-b10x-llm-messages.log`
- `/home/timo/.cache/llm-wave-3/opaque/adversary-1/red-b10x-llm-responses.log`
- `/home/timo/.cache/llm-wave-3/opaque/adversary-1/red-b10x-llm-core.log`
- `/home/timo/.cache/llm-wave-3/opaque/adversary-1/suite.log`
- Incremental build output in the assigned `$HOME/.cache/b10x-target/llm-wave3-opaque`. No second build directory was made. Disk had 28G free after the runs.

**7. Findings block**

```findings
- file: crates/llm-messages/tests/adversary_wave3_opaque.rs
  line: 32
  category: contract-drift
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: docs/messages.md, docs/responses.md, item.rs:35 and the round-trip scenario promise the payload byte for byte, but it is re-serialised through serde_json::Value with sorted keys, and the unit's own checks compare normalised Values so they cannot see it
- file: crates/llm-responses/tests/adversary_wave3_opaque.rs
  line: 53
  category: acceptance
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: the Some(_) arm at request.rs:465 carries an item_reference to provider-stored state as unattributed continuation state, which the base refused, contradicting store:false and replayed whole
- file: crates/llm-responses/tests/adversary_wave3_opaque.rs
  line: 64
  category: boundary
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: an entry with an empty type string is carried instead of refused as naming no type; nothing found produces one
- file: crates/llm-responses/src/request.rs
  line: 465
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: every typed entry, including client-authored hosted-tool outputs, is classified as model-minted opaque state, so once bound it skips TurnRequest::validate tool pairing and size bounds
- file: crates/llm-messages/tests/adversary_codec.rs
  line: 84
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the rewritten adversary case accepts any encode error rather than the named unattributed refusal
```
