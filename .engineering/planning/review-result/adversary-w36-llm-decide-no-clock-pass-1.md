---
format: aep.planning-md/3
id: review-result:adversary-w36-llm-decide-no-clock-pass-1
kind: review-result
status: active
title: 'Adversary, pass 1 (story:llm-decide-tests-read-no-clock-and-build-one-binary, wave 2026-11-25-tests-without-a-clock): read only, no run; 1 warning and 2 notes'
relations:
- reviews: story:llm-decide-tests-read-no-clock-and-build-one-binary
revision: 1
---
Adversary pass 1 on unit `story:llm-decide-tests-read-no-clock-and-build-one-binary`, head
`ca51d010e80907342bcd2df6d6e431814061352a`, base `45e7c1c13`.

Verdict: none by a run. The pass read the change and built nothing: its first job went to the build
node while the rule of that hour called that a fault, and the job was cancelled. Cases executed: 0.
Three findings by reading, all introduced by the unit.

1. `crates/llm-decide/src/answer.rs:143` and `:153`, warning. The `Strict` visitor is charged one
   unit for each element and each member it takes. Work inside one visit that grows with what was
   read before (the array so far read again, a key compared with every key so far) changes no count,
   so both cases would pass on such a reader. The two wall-time cases the unit removed could see it.
2. `crates/llm-decide/src/answer.rs:226`, note. `Scanner::skip_while` returned a slice with the
   lifetime of the answer's text. The scan could keep slices across turns of its loop and read them
   again without a charge; only a comment said it must not.
3. `crates/llm-decide/src/answer.rs:390`, note. The row "the literal of an enum" of the second case
   is the row "objects" with another key to the reader and gives the same two counts (400004 and
   1600004).

The mutants the pass prepared against this head were not built.
