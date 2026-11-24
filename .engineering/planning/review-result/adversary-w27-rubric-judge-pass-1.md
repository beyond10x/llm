---
format: aep.planning-md/3
id: review-result:adversary-w27-rubric-judge-pass-1
kind: review-result
status: active
title: 'Adversary, pass 1 (story:rubric-judge, wave 2026-11-24-rubric-judge): 8 introduced, 6 red cases'
relations:
- reviews: story:rubric-judge
revision: 1
---
Recorded by the coordinator as the adversary returned it. Two absolute paths of the workstation are
replaced by "the unit's scratch directory" and by the path inside the repository, because this
repository is public; nothing else is changed, and the findings block is byte for byte.

unit: story:rubric-judge
verdict: red
cases: executed 31→39, red 6
origin: introduced 8, pre-existing 0, undecided 0
wrote-outside-worktree: the unit's scratch directory, `adv/` (logs, 2 run scripts, mutated workspace copy `ws/` with its own `target/`, 321 MB)
needs-coordinator: no

Covers head 966bf1e1e plus one untracked file. `git --no-pager diff --stat` is empty; `git status --short` shows only `?? crates/llm-decide/tests/adversary_w27_judge.rs` (test file; no implementation file touched). rustc 1.99.1 (default toolchain). `before` 31 is the implementor's `gate-test.log` (4+11+16). I took no worktree lease. Conformance check not run: I added no scenario.

## Cases added: crates/llm-decide/tests/adversary_w27_judge.rs

8 cases: 6 red, 2 green guards. First run was that file alone, before any suite run (`adv/red-1.log`, exit 101, `2 passed; 6 failed`). Line numbers below are after rustfmt; the left/right values are unchanged from the first run.

```
:229 an_answer_that_repeats_a_key_is_refused_not_cut_to_its_last_value: {"pass":false,"pass":true,"score":5}
  left: (Some("{\"pass\":true,\"score\":5}"), None)   right: (None, Some(Protocol))
:266 a_number_of_the_answer_reaches_the_verdict_with_every_digit: {"score":51.248178375505404}
  left: (Some("{\"score\":51.24817837550541}"), None)   right: (Some("{\"score\":51.248178375505404}"), None)
:282 the_request_carries_the_schema_bytes_the_digest_covers
  left: "{\"maximum\":51.24817837550541,\"type\":\"number\"}"   right: "{\"maximum\":51.248178375505404,\"type\":\"number\"}"
:307 enum_and_const_compare_numbers_by_value_however_they_are_written: {"enum":[0,0.5,1],"type":"number"} admits 1.0
  left: Some(Protocol)   right: None
:317 a_bound_is_compared_exactly_beyond_two_to_the_53rd: 2^53 + 1 is over a maximum of 2^53
  left: (Some("9007199254740993"), None)   right: (None, Some(Protocol))
:345 an_answer_cut_off_at_the_output_limit_is_not_a_verdict
  left: (Some("12"), None)   right: (None, Some(Protocol))
```

Green guards: `a_retriable_failure_is_retried_on_the_one_target_with_the_same_bytes` (:355), `a_schema_and_a_literal_nest_to_the_bound_and_no_deeper` (:380). The added file is clean under `cargo clippy -p llm-decide --test adversary_w27_judge -- -D warnings` and rustfmt.

## Suite run (after the cases existed)

`cargo test -p llm-decide --locked --no-fail-fast`, exit 101 (`adv/suite-2.log`): adversary_u2_chain 4 ok, adversary_w27_judge `2 passed; 6 failed`, classify 11 ok, judge 16 ok. 39 executed, 6 red, all in the added file.

## Findings (all origin introduced: `judge.rs` and `schema.rs` do not exist at base ccec1f4a8)

1. `crates/llm-decide/src/judge.rs:330`, CONFIRMED, warning. Measured: an answer that gives a key twice is accepted with the last value; `{"pass":false,"pass":true,"score":5}` becomes the verdict `{"pass":true,"score":5}`. `docs/design.md:794` says "no key is dropped, no value coerced". Reaches: every `judge` caller, when the far side emits a repeated key; I found no recorded model answer that does. Fix named, not applied: refuse a repeated key while reading the answer, or drop the sentence.

2. `crates/llm-decide/Cargo.toml:22`, CONFIRMED, warning. Measured: `serde_json` is built without `float_roundtrip` for this crate, so the answer `51.248178375505404` comes back as `51.24817837550541` in `verdict_json`. With `llm-messages` in the same build (`cargo test -p llm-decide -p llm-messages --test adversary_w27_judge`, `adv/unified.log`) both float cases pass: `4 passed; 4 failed`. So verdict bytes depend on which crates are linked. Reaches: `cargo test -p llm-decide` (the story's verification target) and a consumer that brings its own `Models`; the conformance binary and `llm-models` link `llm-messages` and do not show it. Fix named: `features = ["float_roundtrip"]`, as `crates/llm-messages/Cargo.toml:20` has.

3. `crates/llm-decide/src/judge.rs:158`, INFEASIBLE, note. Measured: same cause on the schema. The digest covers `"maximum":51.248178375505404`, the stored schema and the request carry `51.24817837550541`, against the comment at :156. Reaches: nothing found; needs a 17-digit bound in a rubric schema.

4. `crates/llm-decide/src/schema.rs:118`, CONFIRMED, warning. Measured: `enum` (:118) and `const` (:121) compare with `serde_json` equality, where `1` and `1.0` differ. `{"type":"number","enum":[0,0.5,1]}` refuses `1.0`; the other five inputs in the case (`0.0`, `2.0` against integer enum, `5.0` against `const 5`, `1` against `[1.0,2.0]`, nested) sit behind that first assertion and were not individually run. `schema.rs:73` says "Every keyword is read as JSON Schema reads it" and :129 "however it is written"; `type: integer` does admit `2.0`. Reaches: any rubric with a numeric `enum` or `const`; the failure is a false refusal, not a wrong verdict.

5. `crates/llm-decide/src/schema.rs:151`, INFEASIBLE, note. Measured: bounds compare as `f64`, so `maximum` 2^53 admits 2^53+1 (the mirrored `minimum` assertion was not reached). Reaches: nothing found.

6. `crates/llm-decide/src/judge.rs:305`, INFEASIBLE, note. Measured: `stop_reason` is not read; an answer `12` with `MaxOutputTokens` is a verdict. Reaches: only a schema whose root admits a number has a valid cut-off prefix; no rubric in tests or scenarios has one.

7. `crates/llm-decide/src/judge.rs:277`, CONFIRMED, warning. Mutant missed: with `FallbackPolicy::disabled()` in a scratch copy, `tests/judge.rs` stays `16 passed` and the retry guard goes red at :365 (`adv/mutant-3.log`). Decision 5 ("the same-target retry stays") had no test and no scenario. Reaches: every retriable provider failure.

8. `crates/llm-decide/src/schema.rs:10`, CONFIRMED, note. Mutants missed: `MAX_DEPTH` 128 (`adv/mutant.log`) and the literal bound at :53-54 dropped (`adv/mutant-3.log`) both leave `tests/judge.rs` at `16 passed`; the depth guard goes red (65-level schema accepted; 100-level literal accepted at :415). The unit's 200-level case (`tests/judge.rs:647-653`) cannot tell: past 128 levels the re-read at `judge.rs:158` refuses by itself (`adv/mutant-2.log`: a 200-level literal still refused with the bound dropped). Reaches: only a schema built in memory, not parsed.

## Attacked, not broken

- Digest literal: recomputed with `sha256sum` outside the crate, equals `020bdd6b…840b`.
- Pinned route: `explain` lists every target of the route, rejected ones included (`crates/llm-routing/src/selection.rs:192-216`), so an incompatible second target cannot hide; an embed route is refused there. Read, not run.
- Request bytes: no request id, clock or map order enters; key order is sorted explicitly at every depth. Read, plus the unit's literal byte assertion.
- Call record: `Judging` relabels every record; the scenario's record JSON holds no turn text. Read.
- Conformance adapter: calls the real `Rubric::new` and `judge`; the only fact it computes is the request sha256 inside the scripted model. Read.
- Not attacked: cancellation, two concurrent calls with one request id, the conformance check itself.
- Aside, not a finding: clippy 1.99.1 fails `crates/llm-decide/tests/classify.rs:390` and `:603` (`assert_is_empty`), a file this unit did not change.

```findings
- file: crates/llm-decide/src/judge.rs
  line: 330
  category: acceptance
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: an answer that repeats a key is accepted with its last value, so a key is dropped although the design says none is
- file: crates/llm-decide/Cargo.toml
  line: 22
  category: contract-drift
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: serde_json without float_roundtrip changes the last digit of an answer's number, and the verdict bytes differ when llm-messages is in the build
- file: crates/llm-decide/src/judge.rs
  line: 158
  category: property
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: the schema read back from its canonical text can differ from the bytes the digest covers; no rubric with a 17-digit bound was found
- file: crates/llm-decide/src/schema.rs
  line: 118
  category: contract-drift
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: enum and const compare 1 and 1.0 as different, so an answer JSON Schema admits is refused
- file: crates/llm-decide/src/schema.rs
  line: 151
  category: boundary
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: minimum and maximum compare as f64 and admit an integer one past a bound of 2^53; nothing found that reaches it
- file: crates/llm-decide/src/judge.rs
  line: 305
  category: boundary
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: an answer cut off at the output limit is read as a verdict when its prefix is valid; only a number-rooted schema allows that and none was found
- file: crates/llm-decide/src/judge.rs
  line: 277
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: disabling the same-target retry leaves the unit's suite green; the added guard catches it
- file: crates/llm-decide/src/schema.rs
  line: 10
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: doubling the depth bound or dropping the bound on const and enum literals leaves the unit's suite green; the added guard catches both
```
