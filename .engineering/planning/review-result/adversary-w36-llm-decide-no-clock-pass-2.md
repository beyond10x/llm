---
format: aep.planning-md/3
id: review-result:adversary-w36-llm-decide-no-clock-pass-2
kind: review-result
status: active
title: 'Adversary, pass 2 (story:llm-decide-tests-read-no-clock-and-build-one-binary, wave 2026-11-25-tests-without-a-clock): green, 3 notes, 0 red cases'
relations:
- reviews: story:llm-decide-tests-read-no-clock-and-build-one-binary
revision: 1
---
Adversary pass 2 (the last) on unit `story:llm-decide-tests-read-no-clock-and-build-one-binary`,
head `c2affd308c05d978a42a81791dbc159d5eb60d33` (correction 1), base `45e7c1c13`.

Verdict: green. Cases executed 99 → 99, 0 red. Origin: introduced 3, all notes. Four pool jobs, all
on the build node; none failed for a missing tool or a toolchain.

Pass 1's findings: the note at `:226` is fixed (eight tries to keep a slice past the next scanner
call, each refused by the compiler: E0499 six times, E0505 twice, E0621 once across the tries); the
note at `:390` is fixed (six list rows in the run); the warning at `:143` is the first note below.

Mutants, one build, each run on the unit's two cases by a case that chooses the mutant for its
thread and calls the unit's own case function (`23 passed; 7 failed`, 54.11 s; six of the seven
are kills, the seventh was a row of the adversary's own with a bare quote, corrected):

| Mutant | Case 1 (numbers) | Case 2 (any make) |
| --- | --- | --- |
| one zero-progress skip per 512 numbers so far | killed, 480683 / 6610208 | killed at "integers", 847940 / 10715879 |
| one zero-progress skip per 512 strings so far | survives, 100004 / 400004 (the case has no string) | killed at "strings with escapes", 897940 / 10915879 |
| per char of a string, one zero-progress skip per 16384 turns so far | survives (no string) | killed at "one string", 1422079 / 20332519 |
| nothing charged | killed, 0 / 0 | killed at "integers", 0 / 0 |
| every scanner char charged 3 times | green, 260012 / 1040012 | green, e.g. integers 700012 / 2800012 |
| the array so far read again per element | survives, counts unchanged; uncounted reads 107830 → 1602520 | survives; 165243 → 2491503 per list row |
| a key compared with the keys so far | survives (no object) | survives, one object 538894 / 2188894 unchanged; uncounted 1233134 → 19580889 |
| the rest compared inside `skip_while` | survives; uncounted bytes 12520000 → 199925000 | survives; e.g. integers 68446896 → 1093596875 |
| each literal copied to a `String`, the copies read again per number | survives; 205440 → 3165056 | survives; 317800 → 4932928 |
| one `skip_while(\|_\| true)`, then the base's scan on that slice with the `strip_suffix` defect | survives, 60002 / 240002; uncounted bytes 12480625 → 199922500 | survives, e.g. integers 200002 / 800002 |
| a string read again inside its own visit | survives (no string) | survives, one string 300007 / 1200007 unchanged; uncounted 393216 → 5111808 |

Every mutant whose extra work goes through a call on the scanner is killed. What survives is work
inside one visit, inside a scanner method, on a copy, or on the whole text taken as one slice.

Base against head (`4 passed; 0 failed`, 0.30 s): a copy of the base's `integers_are_exact` beside
the head's gives the same answer on 71 named texts and on 400000 generated texts with a fixed seed
(17103 of them read by `serde_json`, the scan refuses 3905 of those); `whole` on 37 named answers
gives the values written in the case; a count starts at 0 after another count and after a panic
inside `read`.

Suite on the clean head: unit `4 passed`, `tests/main/main.rs` `95 passed`, doc-tests 0, exit 0, one
integration executable. 50 runs of the two cases with the fetched executable on the workstation at
load average 84: 50 exit 0, one distinct set of counts.

Findings:

1. `crates/llm-decide/src/answer.rs:143` (and `:153`), note, CONFIRMED, introduced. Pass 1's
   warning, now run: the two mutants that read the array so far or the keys so far again leave
   every count unchanged while their own reads grow 15 to 16 times. The doc comment of `mod work`
   names the limit truthfully.
2. `crates/llm-decide/src/answer.rs:235`, note, CONFIRMED, introduced. `skip_while` takes the
   caller's predicate, so `skip_while(|_| true)` returns the whole rest of the text for one charge
   per char. The base's scan with the defect this story's cases were written after then passes both
   cases, with lower counts than the head. Named fix: the skipping method returns nothing, and the
   scanner reads a number's literal with a predicate of its own.
3. `crates/llm-decide/src/answer.rs:271`, note, CONFIRMED, introduced. The list of what the count
   does not see leaves out work inside the visit of one string that grows faster than that string.
