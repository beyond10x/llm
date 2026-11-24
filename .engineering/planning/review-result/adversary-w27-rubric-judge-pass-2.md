---
format: aep.planning-md/3
id: review-result:adversary-w27-rubric-judge-pass-2
kind: review-result
status: active
title: 'Adversary, pass 2 (story:rubric-judge, wave 2026-11-24-rubric-judge): 4 introduced, 2 red cases'
relations:
- reviews: story:rubric-judge
revision: 1
---
unit: story:rubric-judge
verdict: red
cases: executed 48→52, red 2
origin: introduced 4, pre-existing 0, undecided 0
wrote-outside-worktree: the brief's scratch directory only, under `adv/` (pass-2 logs, one run script, a mutated workspace copy `adv/ws/` with its own `target/`; 365 MB in total)
needs-coordinator: no

Covers head bf58d42ab plus one untracked file. `git --no-pager diff --stat` is empty; `git status --short` shows only `?? crates/llm-decide/tests/adversary_w27_judge_pass2.rs` (test file; no implementation file touched, the committed pass-1 file is unchanged). Cargo ran with `RUSTUP_TOOLCHAIN=1.98.0`. `before` 48 is the implementor's `c1-gate-test.log` (4+8+11+25). I took no worktree lease. Conformance check not run: I added no scenario.

## Cases added: crates/llm-decide/tests/adversary_w27_judge_pass2.rs

4 cases: 2 red, 2 green property guards. First run was that file alone, before any suite run (`adv/p2-red-1.log`, exit 101, `2 passed; 2 failed`, the binary finished in 0.53 s):

```
:167 a_long_answer_of_numbers_is_read_in_time_proportional_to_its_length
  20000 numbers (40001 bytes) took 9.538588ms, 80000 numbers (160001 bytes) took 153.85855ms
:182 an_answer_nests_128_levels_and_no_deeper   (129 levels refused, then 128 levels:)
  left: None   right: Some("[[[[…128 levels…]]]]")
```

Green guards: `a_bound_and_a_literal_order_every_pair_of_numbers_by_exact_value` (:330), `a_number_that_is_read_and_written_comes_back_as_itself` (:386). The file is clean under clippy `-D warnings` and rustfmt on 1.98.0 (`adv/p2-clippy.log`).

## Suite run (after the cases existed)

`cargo test -p llm-decide --locked --no-fail-fast`, exit 101 (`adv/p2-suite.log`): adversary_u2_chain 4 ok, adversary_w27_judge 8 ok, adversary_w27_judge_pass2 `2 passed; 2 failed`, classify 11 ok, judge 25 ok. 52 executed, 2 red, both in the added file. The timing case printed 6.57 ms and 127.9 ms in this run.

## Findings (all origin introduced: the correction wrote the lines)

1. `crates/llm-decide/src/answer.rs:131`, CONFIRMED, warning. Measured: reading an answer costs the square of its length. Four times the numbers took 16.1 times as long in the first run and 19.5 times in the suite run. Cause: `rest.strip_suffix(after)` compares the whole remaining text once per number token. With `integers_are_exact` bypassed (mutant m10, `adv/p2-mutant-m10-integers-rounded.log`) the timing case passes, so the cost is in that scan. The loop is synchronous inside `judge` and checks no cancel. Reaches: every `judge` caller whose far side returns a long, number-heavy answer, such as a repetition loop up to the output limit, or an endpoint that fills the 32 MiB stream bound (`crates/llm-http/src/sse.rs:6`). Derived from the measurement, not measured: 160 KB costs 0.15 s, so megabytes cost minutes. Fix named, not applied: take the literal as `&rest[..rest.len() - after.len()]`, or check each integer inside the visitor.

2. `docs/design.md:814`, INFEASIBLE, note. Measured: the design says an answer nests "128 levels"; 128 nested arrays are refused, so the reader admits 127. The unit's case (`crates/llm-decide/tests/judge.rs:528`) repeats "128 levels" in its comment and tests 100 and 200, so it cannot tell. Reaches: nothing found; a schema nests 64 levels, so only an untyped subschema lets an answer get there.

3. `docs/design.md:817`, INFEASIBLE, note. Not a case of mine: the unit's own case pins it (`crates/llm-decide/tests/judge.rs:591`, `("-0", "-0.0")`). The design says every number written without a fraction or an exponent "is kept with every digit", and `CHANGELOG.md:14` says "every integer kept exactly"; `-0` comes back as `-0.0`. Reaches: nothing found that answers `-0`. One doc clause settles it.

4. `crates/llm-decide/src/schema.rs:209`, CONFIRMED, note. Mutant missed: with the subnormal exponent `-1073` instead of `-1074` (every subnormal doubled, so the greatest subnormal orders above the least normal) the committed suite stays green (judge `25 passed`, adversary_w27_judge `8 passed`) and the property guard at :330 goes red (`adv/p2-mutant-m02-subnormal-exponent.log`). Reaches: a bound or literal beside a subnormal double; no rubric found has one.

## Attacked, not broken

- `schema::compare` and `schema::same`: 34 x 34 edge pairs (zero and minus zero, least and greatest subnormal, least normal, greatest finite double, both sides of 2^53, 2^63 and 2^64 as integers and as doubles) and 3000 drawn pairs with a fixed seed, each through `minimum`, `maximum` and `const`, against a reference order written apart from the library's. Green.
- `canonical` against the reader: 4000 drawn integers keep every digit; 2000 drawn doubles, written by Rust's formatter, read back as the same bits, and the verdict's own text judged again gives the same bytes. Green.
- Mutants, one at a time on the scratch copy (`adv/p2-mutants.summary`): 10 of 12 are caught by the committed suite: negatives compared the wrong way round, the aligned-mantissa step dropped, array length and object length dropped in `same`, the repeated-key check lost for a nested object and for an array item, the stop-reason check dropped, trailing bytes accepted, integers rounded, the escape skip dropped. The twelfth, subnormal exponent `-1075`, halves every subnormal and changes no order; nothing can observe it.
- The digest and a number's spelling: `1` and `1.0` give two digests, which is what `docs/design.md:783` says ("as it was given"). Read, not run.
- "Exactly one text item" against the wires: the Responses decoder joins the parts of one message into one item (`crates/llm-responses/src/stream.rs:957-968`). Read; I found no decoder that splits a normal answer.
- `serde_json` feature `arbitrary_precision`, which would change what the strict visitor sees, is not enabled anywhere in the workspace (`cargo tree -e features`).
- The pass-1 claims: the 8 pass-1 cases pass on this head; none of the 12 mutants turned one green that should be red.
- Not attacked: `preserve_order` (the implementor named it), the conformance check itself.

```findings
- file: crates/llm-decide/src/answer.rs
  line: 131
  category: boundary
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: the exact-integer scan compares the whole remaining text per number token, so a long answer of numbers costs the square of its length inside judge
- file: docs/design.md
  line: 814
  category: contract-drift
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: the design says an answer nests 128 levels and the reader admits 127; nothing found that reaches the boundary
- file: docs/design.md
  line: 817
  category: judgement
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: the design says an integer is kept with every digit while the unit's own case pins -0 as -0.0; nothing found that answers -0
- file: crates/llm-decide/src/schema.rs
  line: 209
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: doubling every subnormal leaves the committed suite green; the added property guard catches it
```
