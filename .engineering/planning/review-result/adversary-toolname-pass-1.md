---
format: aep.planning-md/1
id: review-result:adversary-toolname-pass-1
kind: review-result
status: active
title: 'Adversary, streamed tool-call name, pass 1: four red cases in the Chat re-emitter and Responses announcement checks'
relations:
- reviews: story:streamed-tool-call-name
revision: 1
---
unit: story:streamed-tool-call-name, uncommitted working tree on base `d5c73e8` at `/home/timo/.local/state/worktree/trees/b10x/llm/wave2-toolname`
verdict: NEEDS-CHANGE
cases: executed 216→220, red 4
origin: introduced 3 / pre-existing 0 / undecided 1
wrote-outside-worktree: 2 paths (scratch directory and suite log, listed in part 6)
needs-coordinator: whether the Chat re-emitter should compare streamed arguments the way the projection parses them (blank means `{}`); I did not take the worktree lease, because the brief gave me no lease step

**1. `git --no-pager diff --stat`**

The output is `25 files changed, 959 insertions(+), 100 deletions(-)`, all the implementor's, and unchanged by me. My two files are untracked, so they show only in `git status` as `??`:
- `crates/llm-chat/tests/adversary_toolname.rs`
- `crates/llm-responses/tests/adversary_toolname.rs`

I touched no implementation file.

**2. Cases added (each file run alone, before the suite)**

| # | Case | What it asserts | Now |
|---|---|---|---|
| A | chat `a_chat_stream_relayed_through_the_chat_reemitter_closes_when_its_arguments_are_blank` | Chat bytes with `"arguments":" "` go through `project_response_bytes`, then `IngressStream::chunk`/`close`; `close` accepts the projection's own outcome | red |
| B | chat `a_call_completed_at_close_and_an_unannounced_call_take_consecutive_wire_indices` | an announced call with no streamed arguments, then an unannounced call, get wire indices `[0, 1]` | red |
| C | responses `a_finished_call_that_renames_its_announcement_is_not_silently_accepted` | an announced `call-1:file_read` whose terminal item is `call-1:clock` is refused, or matches | red |
| D | responses `a_finished_call_under_another_identifier_than_its_announcement_is_not_silently_accepted` | same, with the terminal item under `call-2` | red |

Red output, verbatim:
```
A: the re-emitter refused the projection's own outcome: Err(Error { code: Protocol, message: "model outcome contradicts the tool calls already streamed", dispatch: NotSent, retry_after_ms: None, observation: None })
B: assertion `left == right` failed  left: [Number(0), Number(2)]  right: [Number(0), Number(1)]
C: announced call-1:file_read, outcome carries ["call-1:clock"]
D: announced call-1:file_read, outcome carries ["call-2:file_read"]
```

**3. Suite run, after the cases existed**

- Command: `nice -n 19 cargo test -p b10x-llm-core -p b10x-llm-chat -p b10x-llm-responses -p b10x-llm-messages --locked --no-fail-fast`
- Exit 101. Passed 216, failed 4. The only failing targets are `-p b10x-llm-chat --test adversary_toolname` and `-p b10x-llm-responses --test adversary_toolname`.
- `before` = 216 is this same run with my two targets taken out.
- Clippy with `-D warnings` on chat and responses is clean.

**4. Findings (cover the working tree above)**

| # | file:line | What was measured | What reaches it | Verdict | Origin |
|---|---|---|---|---|---|
| 1 | `crates/llm-chat/src/ingress.rs:525` | `close` compares the relayed text with `serde_json::from_str`. `incoming.rs:470` `parse_arguments` turns blank text into `{}`, and `nonempty` relays `" "` as a delta. So Chat relayed into Chat refuses its own outcome after the stream has been sent. At base, `close` never compared (read with `git show d5c73e8:`). | Two functions in the same crate, chained as `docs/chat.md:146-159` describes. No fixture or provider sends blank arguments. | INFEASIBLE | introduced |
| 2 | `crates/llm-chat/src/ingress.rs:510` | An unannounced call's index is `announced.len() + remaining.len()`. But `remaining` also holds announced calls completed at close, so it gives `[0, 2]` and skips 1. The fix is to count only the unannounced entries. | Both halves are documented paths: an announced call whose arguments never streamed, and "a model that returns its calls only in the outcome". I found nothing that combines them in one turn. | INFEASIBLE | introduced |
| 3 | `crates/llm-responses/src/stream.rs:441` | Responses announces the call at `output_item.added`, but nothing checks the finished item's name against it. The caller sees `file_read` and the outcome says `clock`. Chat refuses the same thing (`incoming.rs:240`). | Only the built fixture. The Chat re-emitter would catch it later, but a direct stream consumer would not. | INFEASIBLE | introduced |
| 4 | `crates/llm-responses/src/stream.rs:441` | Same as 3, but the call identifier changes (`call-1` announced, `call-2` finished). Base code already relayed deltas under `call-1` with no check. | Only the built fixture. | INFEASIBLE | undecided (not run at base) |

**5. Attacked and could not break**
- `port.rs` serde: an announcement missing its name, with an invalid name, or with an extra field is refused.
- Chat: argument fragments that arrive before the name are released once, in order, including the fragment that carries the name.
- Chat: a rename after the announcement is refused at `finish`, with the counters still reported.
- Messages `start_block`: the name goes through the same `decode_block` rule, and the finished block reuses the opening value, so nothing can rename it.
- Ingress: an announcement repeated for the same call does not open a second index, and an announcement arriving after unannounced deltas stays out.

**6. Paths written outside the worktree**
- `/home/timo/.cache/llm-wave-2/toolname/adversary-1/` (directory I created; it is the scratch path the brief assigned)
- `/home/timo/.cache/llm-wave-2/toolname/adversary-1/suite.log`
- Build output went into the assigned `$HOME/.cache/b10x-target/llm-wave2-toolname`.

**7. Findings block**
```findings
- file: crates/llm-chat/src/ingress.rs
  line: 525
  category: contract-drift
  severity: warning
  verdict: INFEASIBLE
  origin: introduced
  message: close refuses a Chat projection outcome whose whitespace-only arguments the projection relayed verbatim and parsed as {}.
- file: crates/llm-chat/src/ingress.rs
  line: 510
  category: boundary
  severity: warning
  verdict: INFEASIBLE
  origin: introduced
  message: an unannounced call's wire index counts announced calls completed at close, so indices skip (0, 2).
- file: crates/llm-responses/src/stream.rs
  line: 441
  category: contract-drift
  severity: warning
  verdict: INFEASIBLE
  origin: introduced
  message: Responses accepts a finished call whose name differs from its announcement, which Chat refuses.
- file: crates/llm-responses/src/stream.rs
  line: 441
  category: contract-drift
  severity: note
  verdict: INFEASIBLE
  origin: undecided
  message: Responses accepts a finished call under a different call_id from the one announced and streamed.
```
