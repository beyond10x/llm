---
format: aep.planning-md/3
id: review-result:adversary-github-zero-u2-pass-1
kind: review-result
status: active
title: Adversary, pass 1 (story:bedrock-retry-invariant-held, wave 2026-11-09-github-zero)
relations:
- reviews: story:bedrock-retry-invariant-held
revision: 1
---
unit: llm unit 2, story:bedrock-retry-invariant-held, worktree `llm-gh-zero-u2` at `1aaeba93` plus my untracked test file
verdict: green (one CONFIRMED mutant gap, warning)
cases: executed 170→173, red 0
origin: introduced 1, pre-existing 0, undecided 0
wrote-outside-worktree: 7 paths, all in the assigned scratch-adv directory
needs-coordinator: none

**1. Diff stat**

`git --no-pager diff --stat` prints nothing because no tracked file changed. `git status --short` shows the one file I added:
```
?? crates/llm-bedrock/tests/adversary_w2026_11_09_u2.rs
```
It is a test file only. No implementation file was touched.

**2. Cases added** (`crates/llm-bedrock/tests/adversary_w2026_11_09_u2.rs`)

- `throttling_after_output_the_caller_was_not_shown_is_final_on_both_wires`: the opening event, then output the caller sees nothing of, then `throttlingException`. Asserts the sink got nothing, the code is `RateLimited` with `Accepted` dispatch, and the error is not retriable. The silent output is:
  - Converse: a `redactedContent` reasoning delta, a signature-only reasoning delta, or a `messageStop` with no content.
  - InvokeModel: a `redacted_thinking` block start, an empty `thinking` block start plus a `signature_delta`, or a `message_delta` stop with no content.
- `a_cut_after_output_the_caller_was_not_shown_is_final_on_both_wires`: the same bodies cut short by `content-length`. Asserts nothing shown, `Transport`, not retriable.
- `every_transient_exception_is_retriable_before_content_and_final_after_on_both_wires`: on both wires, each of the five transient types is retriable right after the opening and final after a tool start. `modelStreamErrorException` is final even before content.

All three are **green on the real tree**. Run alone, before the suite: `3 passed; 0 failed`, exit 0. None of them was ever red against the real code, so there is no red output from the tree to quote.

Red against a mutated copy in scratch (`mutate.py`). The mutant sets `answered` only when the sink was shown an event, at `converse.rs:916` and `llm-messages/src/decode.rs:374`:
```
Converse a redacted reasoning delta: the model answered, so throttling after it is final
Converse a redacted reasoning delta: the model answered, so a cut after it is final
```
With only the InvokeModel half mutated:
```
AnthropicInvoke a redacted_thinking block start: the model answered, so throttling after it is final
AnthropicInvoke a redacted_thinking block start: the model answered, so a cut after it is final
test result: FAILED. 1 passed; 2 failed
```
Under the mutant on both wires, the whole llm-bedrock suite except my file passes: 170 of 170, the unit's two new cases included (`client: 28 passed`). Exit was 101, from my file only.

**3. Suite run** (after the cases existed)

`RUSTUP_TOOLCHAIN=1.98.0 b10x-build-slot cargo test -p llm-bedrock`: exit 0, every binary `ok`. Per binary: lib 22, adversary_d262 2, adversary_w2026_11_09_u2 3, w21 2+2, w25 structured 3, w25 web identity 13, w26 2, client 28, converse 20, credentials 21, eventstream 13, image_input 5, security 1+7, sigv4 6, structured_output 6, system_messages 2, web_identity 1+14, doc 0. That is 173 in total. The 170 "before" figure is the same run's per-binary counts without my binary; I made no separate deselected run.

**4. Findings**

| file:line | verdict | origin | what was measured | what reaches it |
|---|---|---|---|---|
| `crates/llm-bedrock/tests/client.rs:298` | CONFIRMED | introduced | `silent_on` uses a `toolUse` / `tool_use` block start, and that start shows the caller a `ToolCallStarted`. The asserts at `:595` and `:614` only check that no text was shown. A mutant that sets `answered` only when something was shown survives the whole llm-bedrock suite (170/170) on both wires. My cases kill it. | Real Bedrock output: redacted or signature-only reasoning on Converse, `redacted_thinking` or empty `thinking` on InvokeModel. The current code is correct here, because it sets `answered` by event kind (`converse.rs:916`, `llm-messages/src/decode.rs:374`). |

Fix to name, not apply: either adopt my file, or switch `silent_on` to a reasoning or redacted block and assert `events == []`. `llm-messages/tests/retry_class.rs` already does this with `SILENT_THINKING`. The AGENTS.md row is still `UNMAPPED` on this tree, as the brief said; the coordinator is to name the tests.

**5. Attacked, not broken**

- Code defect on either wire for reasoning, redacted reasoning, an empty thinking block, or a stop with no content: none. All of them are final.
- `message_start` / `messageStart` counted as answered: the unit's before-content cases catch it.
- `answered` set only on text deltas, or only on `contentBlockDelta`: the unit's tool-start cases catch it.
- The other transient exceptions (`internalServerException`, `serviceUnavailableException`, `modelNotReadyException`, `modelTimeoutException`) through both decoders: classified correctly on both wires.
- `modelStreamErrorException` before content: final, as documented.
- `ping` counted as answered on InvokeModel: `llm-messages` tests cover it, and the recorded InvokeModel stream has no `ping`, so I wrote no llm-bedrock case for it.
- HTTP-level 429/503 before the stream: classified by `errors::http_error` the same way for both wires, and the existing `client.rs` refusal cases cover it.
- A clean end of body at a frame boundary: a non-retriable protocol error either way, which is the conservative direction, so not a finding.

**6. Paths written outside the worktree** (all under the unit's assigned `scratch-adv/` directory)

- `case-alone.log`
- `suite.log`
- `mutant-run.log`
- `mutant-suite.log`
- `mutate.py`
- `mutant/`: a copy of the tree without `target`, `.git`, `website` and `.engineering`. Its `converse.rs` and `decode.rs` are mutated.
- `mutant-target/`: the build directory for the mutant.

The session lease `adv-llm-u2` is released.

```findings
- file: crates/llm-bedrock/tests/client.rs
  line: 298
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "the unit's silent output is a tool block start the caller is shown, so an answered-only-when-shown mutant survives all 170 llm-bedrock cases on both wires"
```
