---
format: aep.planning-md/3
id: review-result:adversary-w24-chat-deltas-pass-1
kind: review-result
status: active
title: Adversary pass 1 on story:chat-counts-every-output-delta (wave 2026-10-30-w24)
relations:
- reviews: story:chat-counts-every-output-delta
revision: 1
---
unit: story:chat-counts-every-output-delta (tree llm-w24-chat at 63f0d3b44, plus 3 untracked test files)
verdict: red
cases: executed 272→276, red 4
origin: introduced 7, pre-existing 0, undecided 0
wrote-outside-worktree: the unit's adversary scratch directory (suite.log)
needs-coordinator: yes; target/ grew from 3.1G to 5.4G (llm-cli and llm-models test builds), over the 4G limit

Pass 1 of `aep:adversary` (Opus) against `63f0d3b44`, wave 2026-10-30-w24. Report as returned,
with the scratch path shortened.

## Cases added (each red alone)

- `crates/llm-chat/tests/adversary_w24_refusal.rs::a_refusal_or_reasoning_that_is_not_a_string_is_refused_not_dropped`:
  `{"refusal":7}: a field the decoder reads, in a shape the wire does not write, was dropped: events [], outcome Ok(TurnOutcome { stop_reason: EndTurn, items: [], ... })`
- `crates/llm-chat/tests/adversary_w24_refusal.rs::a_refused_answer_the_gateway_wrote_is_read_back_as_history`:
  `the gateway refuses the assistant message it wrote itself: Unsupported chat request field is outside the published subset: messages[].refusal`
- `crates/llm-models/tests/adversary_w24_refusal_route.rs::a_target_selected_for_a_conversation_carrying_a_refusal_can_send_it`:
  `selection admitted target coding-secondary (Responses) with rejections [(Id("coding-primary"), [Temperature]), (Id("coding-secondary"), [])], and its wire refuses the conversation before dispatch: Unsupported the Responses projection carries no assistant refusal text`
- `crates/llm-cli/tests/adversary_w24_refusal.rs::infer_on_a_turn_the_model_refused_shows_the_refusal`:
  `the refusal was dropped: exit Some(0), stdout "", stderr "route=small protocol=chat-completions input_tokens=unknown output_tokens=unknown\n"`

Suite: `cargo test --locked -p llm-chat -p llm-models -p llm-cli --no-fail-fast` on Rust 1.98.0:
276 executed, 272 passed, 4 failed, `EXIT=101`; only the three new targets failed.

## Attacked and could not break

Interleavings (refusal then content, content then refusal, refusal split across chunks, refusal
beside tool calls); empty, null and absent `refusal`, `reasoning`, `reasoning_content`,
`tool_calls` and a usage-only chunk all stay retriable; whitespace-only text counts as output; the
equal/different reasoning rule within one delta; `[DONE]` followed by more data (client returns
at `[DONE]`); usage after `[DONE]` stays unknown, never zero; `[DONE]` with no finish reason is
refused and final once output was shown; Messages and Responses encoders refuse at `close` every
outcome carrying a refusal; routing fallback counts `RefusalDelta` as visible; serde keeps
`kind`-tagged kebab-case with `deny_unknown_fields`.

```findings
- file: crates/llm-routing/src/selection.rs
  line: 223
  category: property
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: selection admits a Responses target for a conversation carrying AssistantRefusal, whose projection then refuses it unsupported before dispatch instead of the target being rejected as opaque state is
- file: crates/llm-chat/src/ingress.rs
  line: 561
  category: contract-drift
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: the Chat ingress encoder writes message.refusal that the same crate's ingress decoder refuses as messages[].refusal, so a client echoing the gateway's refused answer cannot continue the conversation
- file: crates/llm-cli/src/lib.rs
  line: 784
  category: acceptance
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: llm infer prints only TextDelta, so a turn the model refused shows no refusal text and exits 0 with an ordinary usage line
- file: crates/llm-chat/src/incoming.rs
  line: 209
  category: contract-drift
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: a non-string refusal or reasoning delta or message.refusal is silently dropped and not counted, contrary to docs/chat.md's rule that every field read is refused when it contradicts the wire; no server shown to send it
- file: crates/llm-chat/src/outgoing.rs
  line: 123
  category: judgement
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: a refusal-only assistant turn is sent with neither content nor tool_calls, which OpenAI's schema and llama.cpp's parser document as required; not verified against a server
- file: docs/chat.md
  line: 94
  category: judgement
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: the claim that every other wire has no place for refusal text is likely wrong for Responses, which has a refusal content part on assistant output messages; inferred, not verified here
- file: AGENTS.md
  line: 58
  category: contract-drift
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the retry-only-before-output invariant row does not name the unit's new cut tests in every_output_delta.rs, so they can be weakened without touching the table
```
