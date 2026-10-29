---
format: aep.planning-md/3
id: review-result:adversary-w23-context-length-pass-1
kind: review-result
status: active
title: Adversary pass 1 on story:context-length-refusal-is-typed (wave 2026-10-29-w23)
relations:
- reviews: story:context-length-refusal-is-typed
revision: 1
---
```
unit: story:context-length-refusal-is-typed
verdict: red
cases: executed 274→281, red 1
origin: introduced 5, pre-existing 0, undecided 0
wrote-outside-worktree: scratch adv1/cases.log, scratch adv1/suite.log
needs-coordinator: yes
```

Adversary pass 1 of wave 2026-10-29-w23 on `a0a6ea6a` (merged at `707fcb93`), branch
`adv/w23-ctx-pass-1`. One new file, `crates/llm-responses/tests/adversary_w23_context_length.rs`;
no file under `crates/*/src/` changed.

Cases added:

- `a_context_length_refusal_relayed_through_the_ingress_encoder_reads_back_with_its_cause`: red. An
  `Error` with `refusal: ContextLengthExceeded` sent through `IngressStream::fail` and read back with
  `decode_stream` loses the cause (`left: None`, `right: Some(ContextLengthExceeded)`).
- `the_code_in_any_shape_but_the_exact_string_names_no_cause`: green, pinning (whitespace, newline,
  kebab-case, mixed case, prefix/suffix, array, object, number, bool, empty, nested, `param`,
  `errors[]`, top-level array, bare string).
- `the_byte_bound_is_inclusive_at_its_edge`: green, pinning.
- `a_400_whose_body_never_ends_is_the_plain_refusal_within_the_refusal_wait`: green, pinning.
- `a_cancellation_while_a_400_body_is_awaited_ends_the_wait_at_once`: green, pinning.
- `a_should_retry_header_leaves_a_context_length_400_final_and_named`: green, pinning.
- `a_context_length_failure_after_output_is_final_and_names_its_cause`: green, pinning.

Suite: `cargo test -p llm-responses --no-fail-fast`, exit 101: every target passed except
`tests/adversary_w23_context_length.rs` (6 passed, 1 failed); 281 cases executed. fmt applied,
clippy clean on llm-responses.

Tried and could not break: body text reaching message, tap or sink; the code read in any other form
than the exact string; the byte bound at its edge, stalled or cancelled bodies; the 429 quota rule
interfering; `x-should-retry` either way; routing eligibility; in-stream retry class before and after
output; Bedrock struct literals; Messages prose matching; Chat ingress (no failure encoder).

F3 was measured at `707fcb93`, before the coordinator's documentation commit.

```findings
[
  {"file": "crates/llm-responses/src/ingress.rs", "line": 701, "category": "contract-drift", "severity": "warning", "verdict": "CONFIRMED", "origin": "introduced", "message": "F1: IngressStream::fail writes wire code refused for a context-length refusal, so a relayed Error reads back with refusal None (red case crates/llm-responses/tests/adversary_w23_context_length.rs:418)"},
  {"file": "spec/domains/transport.yaml", "line": 39, "category": "contract-drift", "severity": "warning", "verdict": "CONFIRMED", "origin": "introduced", "message": "F2: the ESS spec does not describe the refusal rule or Error.refusal, and no conformance scenario holds the story's acceptance"},
  {"file": "CHANGELOG.md", "line": 7, "category": "contract-drift", "severity": "note", "verdict": "CONFIRMED", "origin": "introduced", "message": "F3: Unreleased lacks the change and the Breaking line for the new public Error field; docs/implementation-status.md is not updated"},
  {"file": "crates/llm-core/src/error.rs", "line": 35, "category": "judgement", "severity": "note", "verdict": "CONFIRMED", "origin": "introduced", "message": "F4: RefusalCause promises minor-release additions, but Error refuses unknown fields and the enum has no catch-all, so an Error from another version fails to decode as a whole"},
  {"file": "crates/llm-routing/src/fallback.rs", "line": 545, "category": "judgement", "severity": "note", "verdict": "CONFIRMED", "origin": "introduced", "message": "F5: AttemptResult::Failed drops refusal, so per-attempt records lose the cause that only the run's final error keeps"}
]
```
