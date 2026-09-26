---
format: aep.planning-md/1
id: review-result:adversary-toolname-pass-2
kind: review-result
status: active
title: 'Adversary, streamed tool-call name, pass 2: three duplicate-announcement cases, three unguarded refusals'
relations:
- reviews: story:streamed-tool-call-name
revision: 1
---
unit: story:streamed-tool-call-name, uncommitted working tree on base `d5c73e8` at `/home/timo/.local/state/worktree/trees/b10x/llm/wave2-toolname` (`stream.rs` sha `753136d…`, `ingress.rs` sha `d78ee96…`)
verdict: INFEASIBLE (3 red cases, all on streams I built; 3 surviving mutants, killed by green cases I added)
cases: executed 220→226, red 3
origin: introduced 6 / pre-existing 0 / undecided 0
wrote-outside-worktree: 1 directory, `/home/timo/.cache/llm-wave-2/toolname/adversary-2/` (660M build dir inside it, listed in part 6)
needs-coordinator: decide whether "a producer emits it once per call" (`port.rs`) should be enforced by the three decoders, or reworded as a promise that holds only for well-formed streams

**1. `git --no-pager diff --stat`**

`25 files changed, 1118 insertions(+), 101 deletions(-)`. This is the implementor's diff, the same as when I started. My three files are untracked, so they show only in `git status`:
- `crates/llm-chat/tests/adversary_toolname_2.rs`
- `crates/llm-responses/tests/adversary_toolname_2.rs`
- `crates/llm-messages/tests/adversary_toolname_2.rs`

I touched no implementation file. The `stream.rs` and `ingress.rs` hashes match the ones in the correction's records. I ran `rustfmt` on my own three files only.

**2. Cases added (each file run alone, before the suite)**

| # | Case | Asserts | Now |
|---|---|---|---|
| E | chat `close_refuses_an_outcome_that_renames_an_announced_call` | `close` refuses with Protocol when `call-1` was announced as `lookup` and the outcome names it `clock` | green |
| F | chat `close_refuses_an_outcome_that_leaves_an_announced_call_out` | `close` refuses with Protocol when the outcome does not carry the announced call | green |
| G | chat `a_call_identifier_opened_at_two_indices_is_announced_once` | the projection emits at most one `ToolCallStarted` per call id | **red** |
| H | responses `a_repeated_opening_item_is_announced_once` | the same `output_item.added` sent twice gives one announcement | green |
| I | responses `one_call_identifier_opened_by_two_items_is_announced_once` | two items with one `call_id` give at most one announcement | **red** |
| J | messages `one_tool_use_id_opened_by_two_blocks_is_announced_once` | two `tool_use` blocks with one `id` give at most one announcement | **red** |

Red output from the solo runs, verbatim:
```
G: call-1 was announced 2 times: [ToolCallStarted { call_id: CallId("call-1"), name: ToolName("lookup") }, ToolArgumentsDelta { call_id: CallId("call-1"), delta: "{}" }, ToolCallStarted { call_id: CallId("call-1"), name: ToolName("lookup") }, ToolArgumentsDelta { call_id: CallId("call-1"), delta: "{}" }]
I: call-1 announced 2 times, result Ok("accepted"): [ToolCallStarted { call_id: CallId("call-1"), name: ToolName("file_read") }, ToolArgumentsDelta { call_id: CallId("call-1"), delta: "{}" }, ToolCallStarted { call_id: CallId("call-1"), name: ToolName("file_read") }, ToolArgumentsDelta { call_id: CallId("call-1"), delta: "{}" }]
J: call-1 announced 2 times, outcome Err(Protocol): [ToolCallStarted { call_id: CallId("call-1"), name: ToolName("lookup") }, ToolCallStarted { call_id: CallId("call-1"), name: ToolName("lookup") }]
```

E, F and H are green on the current tree. I ran them against mutated copies in my scratch directory, never the worktree:

| Mutation | Other cases in the crate | New cases failing |
|---|---|---|
| `ingress.rs:518` → `if false && announced.name != call.name` | 72 chat cases stay green | E (and G, already red) |
| `ingress.rs:530` → `if false && seen.contains(&false)` | 72 stay green | F (and G) |
| `stream.rs:456-458`, the `contains_key` guard removed | 58 responses cases stay green | H (and I) |

No chat scenario could catch the two `ingress.rs` mutants: all three scenarios that carry `events_json` are `accepted: true` (checked with grep).

**3. Suite run, after the cases existed**

- Command: `nice -n 19 cargo test -p b10x-llm-core -p b10x-llm-chat -p b10x-llm-responses -p b10x-llm-messages --locked --no-fail-fast`
- Exit 101. Passed 223, failed 3. Only the three `--test adversary_toolname_2` targets failed.
- `before` = 220: the same run with my 6 cases subtracted by target. It matches pass 1's "after" count, and pass 1's 4 cases are now green.
- `cargo clippy -D warnings` on my three test targets exits 0. `cargo fmt --check` on the three crates exits 0.

**4. Findings (cover the working tree above)**

| # | file:line | What was measured | What reaches it | Verdict | Origin |
|---|---|---|---|---|---|
| 1 | `crates/llm-responses/src/stream.rs:456` | The duplicate guard checks `item_id` only, so two items with one `call_id` announce twice. The decode returns `Ok`, and the Chat re-emitter then relays both items' arguments under index 0. This breaks `port.rs:19` "emits it once per call". Fix: also skip when `announced` already holds that `call_id`. | Only the stream I built. No server has been shown to reuse a `call_id`. | INFEASIBLE | introduced |
| 2 | `crates/llm-chat/src/incoming.rs:175` | Two indices with one `id` are both announced. `validate_for` refuses the duplicate only later, after both announcements have gone out. | Only the stream I built. | INFEASIBLE | introduced |
| 3 | `crates/llm-messages/src/decode.rs:335` | Two `tool_use` blocks with one `id` are both announced before the finish refuses with Protocol. | Only the stream I built. | INFEASIBLE | introduced |
| 4 | `crates/llm-chat/src/ingress.rs:518` | The "another name" refusal promised at `docs/chat.md:155-159` had no case and no scenario, and no falsification record aims at it. Case E now kills it. | `IngressStream::close`, a public API. The conformance adapter is its only non-test caller. | CONFIRMED | introduced |
| 5 | `crates/llm-chat/src/ingress.rs:530` | The same gap for "an announced call the outcome does not carry". Case F now kills it. | Same as 4. | CONFIRMED | introduced |
| 6 | `crates/llm-responses/src/stream.rs:456` | Removing the `contains_key` guard left the responses suite green. Case H now kills it. | Only a server that repeats `output_item.added`; none shown. | CONFIRMED | introduced |

For 4 to 6, the fix is to add a mutation record for each to `docs/verification/{chat,responses}-falsification.json`, and for 4 and 5 a scenario too.

**5. Attacked and could not break**
- Pass 1 cases A to D are all green now.
- Chat re-emitter: whitespace arguments, and the indices of an unannounced call next to a call completed at close.
- Chat projection: name arriving after the id, an invalid id or name, a later valid name replacing an invalid one, and a rename refused at finish with the counters kept.
- Responses: several calls interleaved, names swapped between two calls, an announced call missing from the terminal object, and a deferred refusal outranking the contradiction check.
- Messages: the name rule applied at block open, the finished block reusing the opening value, and a block with no argument fragments.
- I found no production caller of `IngressStream` or of Responses `decode_stream` outside the conformance adapter.

**6. Paths written outside the worktree**
- `/home/timo/.cache/llm-wave-2/toolname/adversary-2/` (the assigned scratch directory), containing:
  - `mutate.py`
  - `copy/` (9.8M copy of the worktree source, used for the mutations; restored after)
  - `target/` (660M build dir for the copy, left in place)
  - logs: `alone-chat.log`, `alone-responses.log`, `alone-messages.log`, `suite.log`, `mut-ingress-renamed-outcome-accepted.log`, `mut-ingress-omitted-announced-call-accepted.log`, `mut-responses-repeated-opening-item-reannounced.log`
- The worktree builds went into the assigned `$HOME/.cache/b10x-target/llm-wave2-toolname`.
- I took the worktree session lease `adversary-2-toolname` and released it.

**7. Findings block**
```findings
- file: crates/llm-responses/src/stream.rs
  line: 456
  category: contract-drift
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: two opening items sharing one call_id each emit ToolCallStarted and the decode is accepted, contrary to port.rs "once per call".
- file: crates/llm-chat/src/incoming.rs
  line: 175
  category: contract-drift
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: two wire indices opened under one call id are both announced before the duplicate is refused downstream.
- file: crates/llm-messages/src/decode.rs
  line: 335
  category: contract-drift
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: two tool_use blocks sharing one id are both announced before finish refuses the turn.
- file: crates/llm-chat/src/ingress.rs
  line: 518
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: the documented refusal of an outcome renaming an announced call had no case, scenario or falsification record; the added case E kills it.
- file: crates/llm-chat/src/ingress.rs
  line: 530
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: the documented refusal of an outcome omitting an announced call had no case, scenario or falsification record; the added case F kills it.
- file: crates/llm-responses/src/stream.rs
  line: 456
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: removing the repeated-opening-item guard left the responses suite green; the added case H kills it.
```
