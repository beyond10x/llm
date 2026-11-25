---
format: aep.planning-md/3
id: review-result:adversary-w35-converse-empty-turn-pass-2
kind: review-result
status: active
title: 'Adversary, pass 2 (story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason, wave 2026-11-25-converse-empty-turn): red, 4 notes, 2 red cases'
relations:
- reviews: story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason
revision: 1
---
Adversary pass 2 (the last) on unit `story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason`,
head `032c890b9ad6a2cf500903ef22adf7f1c9cf308e` (correction 1), base
`7ddbd1d6db98985229d9a2236f73402fbd5ed835`.

Verdict: red. Cases executed 201 → 203, 2 red. Origin: introduced 3, pre-existing 1. All four
findings are notes; neither red case is a wrong retry class, both are document sentences the decoder
does not hold on answers the adversary built.

The seven findings of pass 1: 1 to 5 and 7 closed with the line that shows it; 6 closed by the
coordinator's decision (the test adapter and its two tests are gone, no file names them, the kept
test holds both halves of its name).

The two cases, in `crates/llm-bedrock/tests/converse.rs`:

```
adversary_w35_a_text_block_opened_by_its_start_leaves_no_item_and_shows_nothing panicked at crates/llm-bedrock/tests/converse.rs:1963:5:
  left: ([Opaque { ... payload: Object {"contentBlockIndex": Number(0), "start": Object {"text": String("")}} }], [Warning { code: "unknown-output-item", ... }])
 right: ([], [])
adversary_w35_a_stop_marked_retriable_reports_no_output_the_model_produced panicked at crates/llm-bedrock/tests/converse.rs:1989:5:
marked for another attempt although the answer reports output the model produced and billed: output_tokens Some(25), Error { code: Unavailable, ... retriable: true, ... final_usage: true }
```

Suite afterwards: `cargo test -p llm-bedrock --no-fail-fast` exit 101, 201 passed, 2 failed (the two
above), 18 binaries; `cargo test -p llm-models --no-fail-fast` exit 0, 85 passed, 19 binaries,
`relayed_failure` 7; `cargo run --locked -p llm-conformance -- check` exit 0, 1088 passed three
times, skipped 0. No new test binary; the slowest binary 4.57 s.

1. `docs/bedrock.md:311`. The sentence says a text block that "opened by its start" leaves no item
   and shows nothing. A `contentBlockStart` with `start: {"text": ""}` is an unknown block: it leaves
   an opaque item and shows `unknown-output-item`; `start: {}` is a `protocol` refusal. The turn is
   still `Ok(Incomplete)`. The delta and the stop-only forms hold. Nothing reaches it: the two
   recorded `contentBlockStart` events are both `toolUse`.
2. `docs/bedrock.md:287`, `docs/design.md:509`, `crates/llm-bedrock/src/converse.rs:528`. The
   retriable row is justified by "the model produced nothing". The code never reads the counters for
   the decision: the failure carries whatever `metadata` reports as final usage, a non-zero
   `outputTokens` included. A caller that follows `may_retry()` can be billed output it never saw on
   each attempt, and no code in the repository bounds a Converse retry. The unit's own row test
   expects the retriable row with `output_tokens: Some(1)`. No recording holds a Converse answer
   without a content block. The Messages rule has the same shape.
3. `crates/llm-bedrock/src/converse.rs:1407` and `:480`, conformance alone. `!content.is_empty()` →
   `false` and `&& items.is_empty()` dropped both pass all 1088 scenarios: no scenario holds a whole
   answer with an empty text block, or a kept event before an unmodelled stop. The crate's tests
   catch both.
4. `docs/bedrock.md:272` and `website/docs/guides/call-a-model-on-bedrock.md:139`, pre-existing and
   untouched: "retry and fallback belong to routing" contradicts the new paragraph that routing
   refuses a Converse client and the caller bounds the retry.

Mutants, 9, each in the mutants worktree: `next_index > 0` → `false` (crate red 3 cases, conformance
red 1 scenario), → `true` (6, 4), → `self.answered` (6, 4); `!content.is_empty()` → `false` (crate
red 2, conformance green), → `true` (5, 2); `&& !content` dropped (3, 1); `&& items.is_empty()`
dropped (crate red 1, conformance green); the advisory shown for a failed turn (crate red in the new
test on all 7 rows, conformance green); the new scenario's `summary` edited (conformance red: "suite
drift: regenerate contracts/suite.json with ESS").

Attacked and not broken: every content block arm moves `next_index` or refuses first; a `toolUse`
delta for a block that never started and an out-of-order index are final `protocol`; a whole answer
with `content` absent or `null` is the final `protocol` refusal as at the base, `[]` fails as its
row, one unknown block or `{"text": ""}` is `Ok(Incomplete)`; stream and whole agree for every block
kind and `InvokeModel` agrees on the class for an empty text block; the 70 test names and every
scenario path on a changed document line exist; `CHANGELOG.md` lines are 78 to 119 characters; five
older scenarios changed one `summary` line each and one synthetic answer one line.
