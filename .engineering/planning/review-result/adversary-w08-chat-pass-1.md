---
format: aep.planning-md/3
id: review-result:adversary-w08-chat-pass-1
kind: review-result
status: active
title: Adversary, pass 1, story:chat-ingress-closed-levels
relations:
- reviews: story:chat-ingress-closed-levels
revision: 1
---
unit: story:chat-ingress-closed-levels
verdict: red
cases: executed n/a (no build allowed), written 2
origin: introduced 3, pre-existing 0, undecided 0
write-outside-worktree: ~/.cache/llm-w08/chat/adversary (empty directory; `mkdir -p` ran, and I can't tell whether it already existed)
needs-coordinator: yes

Nothing was compiled or run. Each "red" below is the result I expect from reading the code at head d88d1ae9, against base 602327b5. Two of the unit's own doc sentences disagree with the code. The closed levels, `strict`, the type refusals, the round trips, the effort mapping and the warning order all held under attack.

**1. Proof of what I touched**

`git --no-pager diff --stat` is empty because the new file is untracked. `git status --short`:
```
?? crates/llm-chat/tests/adversary_w08_fields.rs
```
That is the only change, and it is a test file. I edited no implementation file and nothing under `.engineering/`.

**2. Cases added** (`~/.local/state/worktree/trees/b10x/llm/llm-w08-chat/crates/llm-chat/tests/adversary_w08_fields.rs`)

| Test | Asserts | Expected now |
|---|---|---|
| `an_explicit_null_for_every_optional_key_is_read_as_absent` | 20 optional keys across all levels: `null` decodes to the same turn, `stream` and `include_usage` as the key left out | red, one row: `tools: refused (InvalidRequest: chat request tools is not an array)` |
| `a_read_key_of_the_wrong_type_is_refused_as_invalid` | `tool_choice` 5 or `true`, and `type: 5` on a tool call, a tool or a content part, are refused as `InvalidRequest` | red, all 5 rows: refused as `Unsupported` |

**3. Suite run:** not run, because of the build lock. The coordinator should run `cargo test -p b10x-llm-chat --test adversary_w08_fields`, then the full suite.

**4. Findings**

| # | file:line | Finding | Basis | Verdict | Origin | What reaches it |
|---|---|---|---|---|---|---|
| 1 | docs/chat.md:158 against crates/llm-chat/src/ingress.rs:104, 302-304 | The page says an explicit `null` for an optional key counts as absent. `"tools": null` is refused as invalid instead, although a request without `tools` decodes. It is the only optional key where this happens. | CONFIRMED-BY-READING | NEEDS-CHANGE | introduced: the sentence is new in this diff; the code is the same at base (`git show 602327b5`, line 84) | any client body with `"tools": null`; I don't know which clients send it |
| 2 | docs/chat.md:157 against ingress.rs:384-386, 261-265, 307-311, 409-413 | The page says a wrong-typed value at a key the decoder reads is refused as invalid. A non-string, non-object `tool_choice`, or a non-string `type`, is refused as unsupported. It is still refused; only the error code differs. | CONFIRMED-BY-READING | CONFIRMED | introduced (the sentence is new; the code is unchanged from base) | only malformed clients |
| 3 | crates/llm-chat/src/client.rs:109-113 | If the sink refuses the advisory warning, a turn that already succeeded fails (TooLarge, dispatch Accepted) and the caller loses the outcome over a hint. `local_endpoint.rs` pins this and `docs/chat.md` states it; `docs/design.md` does not decide it. | CONFIRMED-BY-READING (judgement, no case) | CONFIRMED | introduced | a bounded or closing sink at the end of a turn |

Fixes, which I did not apply:
- **Finding 1:** either decode `None | Some(Value::Null)` for `tools` as no tools (that widens what is accepted, which the brief says not to do), or reword the sentence to list the keys where `null` counts as absent. If the doc is reworded, flip or drop the `tools` row of my case.
- **Finding 2:** reword the sentence to "refused rather than read as absent", or move those arms to `Error::invalid`.

**5. What I attacked and could not break**
- The path in each closed-level refusal, the role-specific key sets (including a key another role reads), and that only a bounded key name is copied into the refusal.
- The first unknown key is reported in a fixed order: serde_json has no `preserve_order`, so keys come out sorted.
- `strict` true, false, absent or of another type.
- Tool-call and tool-choice `type` checked before the closed check.
- `content: null` and `tool_calls: null` keep their old meaning.
- `parallel_tool_calls` without tools is refused by `validate`, as the design says.
- Verbosity values, and the round trip with both `parallel_tool_calls` values.
- `wire_effort` for `max`, disabled, a budget, and disabled beside an effort; `validate_for` checks the name the wire receives.
- `CARRIED` and the `Advisory` order.
- The warning comes after `validate_for`, never on failure, and the Chat ingress stream drops it.
- The scenario `wire_request` strings match sorted serde_json output.
- Mutants in the key constants, `is_function_type`, the verbosity mapping and `CARRIED` are each caught by the unit's own tests.

**6. Paths outside the worktree:** ~/.cache/llm-w08/chat/adversary (empty).

```findings
[
  {
    "file": "docs/chat.md",
    "line": 158,
    "category": "contract-drift",
    "severity": "warning",
    "verdict": "NEEDS-CHANGE",
    "origin": "introduced",
    "message": "the page says explicit null for an optional key is absent, but \"tools\": null is refused as invalid (ingress.rs:104, 302-304) while omitting tools decodes"
  },
  {
    "file": "docs/chat.md",
    "line": 157,
    "category": "contract-drift",
    "severity": "note",
    "verdict": "CONFIRMED",
    "origin": "introduced",
    "message": "the page says a wrong-typed value at a read key is refused as invalid, but a non-string or non-object tool_choice and a non-string type on a tool call, tool or content part are refused as unsupported"
  },
  {
    "file": "crates/llm-chat/src/client.rs",
    "line": 109,
    "category": "judgement",
    "severity": "note",
    "verdict": "CONFIRMED",
    "origin": "introduced",
    "message": "a sink refusing the advisory warning turns an already-succeeded turn into a TooLarge failure and drops its outcome, a choice the design does not make"
  }
]
```
