---
format: aep.planning-md/3
id: review-result:adversary-github-zero-u1-pass-2
kind: review-result
status: active
title: Adversary, pass 2 (story:ingress-block-boundaries, wave 2026-11-09-github-zero)
relations:
- reviews: story:ingress-block-boundaries
revision: 1
---
unit: llm unit 1, story:ingress-block-boundaries, branch impl/ingress-block-boundaries at `95498057` plus 4 untracked adversary test files
verdict: red (CONFIRMED). The correction broke two of the unit's own tests, in packages its gate never ran.
cases: executed 961→968, red 2 (both are existing unit-written cases; all 7 added cases are green)
origin: introduced 2 / pre-existing 0 / undecided 0
wrote-outside-worktree: 5 paths, all in the unit's scratch-adv2 directory
needs-coordinator: decide which side changes. Either the two tests stop emitting a boundary as the first event into a `VecSink`, or `VecSink` takes a leading boundary.

**1. Diff stat.** Only test files, untracked, 461 lines:
```
?? crates/llm-chat/tests/adversary_w2026_11_09_block_boundaries_pass2.rs       69
?? crates/llm-core/tests/adversary_w2026_11_09_block_boundaries_pass2.rs      182
?? crates/llm-responses/tests/adversary_w2026_11_09_block_boundaries_pass2.rs  75
?? crates/llm-routing/tests/adversary_w2026_11_09_block_boundaries_pass2.rs   135
```

**2. Cases added** (each run alone first, all green on first run)
- llm-core, 4 cases: every boundary sequence a decoder can emit fits a sink whose event bound equals its counted events, and one more counted event is refused `too-large`; a sink at its event bound takes one boundary, then refuses text, opaque and warning events and a second boundary, keeping exactly the accepted prefix; once the byte bound is spent an opaque item is still taken and text is still refused; serde round trip of both variants (payload holding `"kind"` keys, `OpaqueItem` fields equal to `Item::Opaque`, `TurnOutcome` with `Item::Opaque`, extra or missing fields refused).
- llm-responses: Messages redacted thinking relayed to a Responses client: `chunk` sends nothing for the opaque item and the boundary; `close` refuses `unsupported` with the same message as `encode_ingress_response`.
- llm-chat: `chunk` returns `None` for both events; `close` and `encode_ingress_completion` refuse `unsupported`.
- llm-routing: an attempt whose only output is a 64 KiB opaque item into `VecSink::new(16, 64)`, then failing eligible: `Halt::VisibleOutput`, one attempt, `visible_events == 1`, the sink keeps the item.

**3. Suite run** (after the cases existed)
```
cargo test -p llm-core       exit: 0
cargo test -p llm-responses  exit: 0
cargo test -p llm-chat       exit: 0
cargo test -p llm-routing    exit: 101
  test an_eligible_failure_after_only_message_boundaries_still_falls_back ... FAILED
  panicked at crates/llm-routing/tests/fallback.rs:356:5:
    left: IneligibleFailure
   right: Succeeded
  test result: FAILED. 21 passed; 1 failed
cargo test -p llm-messages   exit: 0
cargo test -p llm-calls --test record a_message_boundary_is_not_the_first_event   exit: 101
  panicked at crates/llm-calls/tests/record.rs:556:9:
  assertion `left == right` failed: first event shown: None
    left: None
   right: Some(70)
```
968 cases ran; 961 is the same run without the 7 added cases. Pass-1 cases are green.

**4. Findings**

| file:line | verdict | origin | what was measured | what reaches it |
|---|---|---|---|---|
| `crates/llm-routing/tests/fallback.rs:356` | CONFIRMED | introduced | Halt is `IneligibleFailure`, expected `Succeeded`. Cause `crates/llm-core/src/port.rs:258-260`: a boundary with no counted event before it is refused `too-large`, and the run stops. | The unit's suite. No production decoder emits a leading boundary (nothing found). `95498057` ran only llm-core's tests. |
| `crates/llm-calls/tests/record.rs:556` | CONFIRMED | introduced | Same cause: the turn's first event is a boundary, `VecSink::new(64, …)` refuses it, `time_to_first_event_ms` is `None`, expected `Some(70)`. | The unit's suite. llm-calls was not run after the correction. |

**5. Attacked and held**
- Opaque payloads not charged to the byte bound: the event bound caps their number, `MAX_STREAM_BYTES` (32 MiB) and `MAX_ITEMS` cap their size.
- Boundary cap off by one: the existing port.rs unit tests catch both mutants (`>=` to `>`, dropped increment).
- A sink full before the change still refuses every counted event; only one more boundary is admitted.
- An opaque item past the byte bound still ends fallback eligibility.
- Responses and Chat refuse a Messages opaque item at `close`, streamed and whole alike; Bedrock Converse redacted reasoning emits no `OpaqueItem`.
- Both variants round-trip through serde; unknown fields are refused.

```findings
- file: crates/llm-routing/tests/fallback.rs
  line: 356
  category: contract-drift
  severity: blocker
  verdict: CONFIRMED
  origin: introduced
  message: "the correction's VecSink refuses a boundary with no counted event before it, so the unit's own fallback test for boundary-only attempts now halts IneligibleFailure instead of falling back; llm-routing was not re-run after 95498057"
- file: crates/llm-calls/tests/record.rs
  line: 556
  category: contract-drift
  severity: blocker
  verdict: CONFIRMED
  origin: introduced
  message: "the same VecSink refusal fails the unit's first-output-clock test, whose leading boundary now ends the turn with no first event recorded; llm-calls was not re-run after 95498057"
```
