---
format: aep.planning-md/3
id: review-result:adversary-w07-messages-ingress-encoder-pass-1
kind: review-result
status: active
title: Wave 2026-10-16-w07 adversary, story:messages-ingress-encoder, pass 1
relations:
- reviews: story:messages-ingress-encoder
revision: 1
---
unit: story:messages-ingress-encoder (tree llm-w07-messages-ingress-encoder, head 0f3317ee plus one untracked test file)
verdict: red
cases: executed 124→129, red 4
origin: introduced 3, pre-existing 0, undecided 0
wrote-outside-worktree: ~/.cache/llm-wave-w07/messages-ingress-encoder/scratch/adversary/case-run.log, ~/.cache/llm-wave-w07/messages-ingress-encoder/scratch/adversary/suite-run.log
needs-coordinator: yes. Finding 1 needs a design call: carry the model, refuse, or document the drop. Finding 3 is a limit of the neutral event vocabulary and needs a story outside this unit.

**1. Diff.** `git diff --stat` is empty because the only change is one untracked test file: `crates/llm-messages/tests/adversary_w07_ingress.rs`. I changed no implementation file.

**2. Cases added.** I wrote all five before running anything, then ran the file alone (`cargo test -p b10x-llm-messages --locked --test adversary_w07_ingress`, exit 101, 1 passed, 4 failed):

| test | asserts | now |
|---|---|---|
| `a_reported_model_is_not_dropped_when_none_was_announced` | `new(id, None)` plus an outcome with `upstream_model` set is either refused, or reads back equal | red: `left: None right: Some(Id("example-model-20260201"))` at :115 |
| `a_held_call_the_outcome_omits_is_refused` | a fragment for a call that was never announced, and an outcome without that call, makes `close` return Err | red: `a call the stream reported vanished at close: Ok([...])` at :145 |
| `a_call_held_for_its_name_is_refused_when_the_outcome_omits_it` | a call announced as `workspace.read` (held because the wire refuses that name), and an outcome without it, makes `close` return Err | red: `Ok([...])` at :170 |
| `a_relayed_answer_with_redacted_thinking_reads_back_whole` | an upstream Messages answer [thinking, redacted_thinking, text] is decoded by this crate, re-encoded from the same events and outcome, and decoded again as equal | red: `close` → `Protocol, "Messages answer contradicts what was already streamed"` at :217 |
| `control_a_held_call_the_outcome_carries_is_sent_complete` | control: when the outcome does carry the held call, it is sent and reads back | green |

**3. Suite run** (after the cases existed): `RUSTUP_TOOLCHAIN=1.98.0 cargo test -p b10x-llm-messages --locked --no-fail-fast`, EXIT=101, 125 passed and 4 failed across 129 cases. Every other binary reports `ok`, including `tests/ingress.rs: 21 passed`. The before count of 124 is that run's total minus my 5 cases.

**4. Findings** (they cover 0f3317ee):
- **`crates/llm-messages/src/ingress.rs:163`: the upstream model is silently dropped.** `close` refuses an announced model that disagrees with the outcome's model. But when no model was announced and the outcome reports one, `close` accepts and the reported model is lost. Read back, the outcome differs from the encoded one, which breaks Acceptance line 1. The story allows leaving the model out only "when unreported".
  - What reaches it: a `StreamEvent` carries no model, so a relaying gateway has to call `new(id, None)`. The only callers so far are tests and the conformance adapter. The single scenario covers only the case where neither side has a model.
  - Status: CONFIRMED, introduced.
- **`crates/llm-messages/src/ingress.rs:154`: `close` never reads `held`.** Two docs promise held calls are dealt with at close: the doc at :116-117 and the field doc at :87-88 say "`close` sends them complete", and :277-278 says a call is held "for `close` to refuse". In fact an outcome that omits a held call closes cleanly; the call is neither sent nor refused. Both cases above show it.
  - What reaches it: nothing I could find. No producer in this repo emits a fragment before its call starts, then an outcome without that call.
  - Status: INFEASIBLE. Fix it with a check or a doc line.
- **`crates/llm-messages/src/ingress.rs:384`: a real Messages answer cannot be relayed.** A redacted_thinking block produces no neutral event, so the following text opens at the redacted block's index, and `close` refuses after the caller has already been shown the text.
  - What reaches it: relaying a Messages upstream to a Messages caller. This crate's own decoder produces exactly this event and outcome pair.
  - Status: INFEASIBLE here, because `StreamEvent` has no block boundaries.

**5. Attacked and could not break:**
- Usage: no counter is ever sent as zero, and the fresh input count is derived exactly.
- Terminal reasons: the five named cases are refused or read back as themselves.
- Duplicate and re-announced calls are handled.
- The argument bound is consistent between encoder and decoder at MAX.
- `fail`: each error code maps to an error type the decoder reads back.
- Close-twice, fail-after-close, and caller-owned or unattributed items in the outcome are all handled.
- Opaque blocks with a foreign provenance or of an unknown type are refused, not dropped.
- An empty stream and an empty outcome round-trip.
- The unit's own tests: none of them would pass if the functions they call returned defaults.
- Not tested: whether real external SDK clients accept a `message_start` with no `usage`. No client was available offline, so I don't know.

**6. Paths outside the worktree:** the two scratch logs in the header. Lease `adversary-w07-messages-ingress-encoder` is acquired and released.

```findings
- file: crates/llm-messages/src/ingress.rs
  line: 163
  category: acceptance
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "close accepts an outcome with a reported upstream_model when the stream announced none, so the model is silently dropped and the decoded outcome differs from the encoded one (adversary_w07_ingress::a_reported_model_is_not_dropped_when_none_was_announced)"
- file: crates/llm-messages/src/ingress.rs
  line: 154
  category: contract-drift
  severity: note
  verdict: needs-revision
  origin: introduced
  message: "close never reads held although the docs promise held calls are sent complete or refused at close; an outcome omitting a held call closes cleanly (a_held_call_the_outcome_omits_is_refused, a_call_held_for_its_name_is_refused_when_the_outcome_omits_it); INFEASIBLE, no in-repo producer found"
- file: crates/llm-messages/src/ingress.rs
  line: 384
  category: boundary
  severity: note
  verdict: needs-revision
  origin: introduced
  message: "relaying a Messages answer of thinking, redacted_thinking and text through this crate's decoder and encoder is refused at close after the text was shown, because redacted_thinking emits no neutral event (a_relayed_answer_with_redacted_thinking_reads_back_whole); INFEASIBLE in this unit"
```
