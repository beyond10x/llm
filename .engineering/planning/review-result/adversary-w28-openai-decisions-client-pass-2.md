---
format: aep.planning-md/3
id: review-result:adversary-w28-openai-decisions-client-pass-2
kind: review-result
status: active
title: 'Adversary, pass 2 (story:openai-decisions-client, wave 2026-11-24-openai-decisions-client): 3 introduced, 1 pre-existing, 0 red cases'
relations:
- reviews: story:openai-decisions-client
revision: 1
---
Recorded by the coordinator as the adversary returned it; nothing is changed. Covers head `39a860042`
of `impl/openai-decisions-client` (correction round 1). The last pass.

unit: story:openai-decisions-client
verdict: green
cases: executed 106→109, red 0
origin: introduced 3, pre-existing 1, undecided 0
wrote-outside-worktree: the wave scratch directories `adv2/` (588 KB: logs and throwaway sh scripts) and `adv/` from pass 1 (624 KB); the workspace copy `adv2/ws` and its `target/` are removed
needs-coordinator: no

```
 crates/llm-decide/tests/classify/adversary_w28_endpoint.rs     | 58 +++++++++-
 crates/llm-openai-decisions/tests/client/adversary_w28_wire.rs | 51 ++++++++-
 2 files changed, 106 insertions(+), 3 deletions(-)
```

Last pass: what is left is a gap in the tests, not a defect a caller can reach. Three new green guards are uncommitted on `39a860042` (17 adversary cases now: 13 + 4).

Counts are `cargo test -p llm-openai-decisions -p llm-decide --locked --no-fail-fast`, every binary. "Before" skips my three new cases by name (`adv2/run4-before.log`); "after" is `adv2/run9-after.log`. Clippy with `-D warnings` and `cargo fmt --all --check` pass with my files in place. No scenario touched, so no conformance run and no `.ess-0.87.0/`. Mutants were not run against the conformance scenarios.

**What the brief asked, in order**
1. Re-pin: as strong as the case it replaces. `adversary_w28_wire.rs:214` keeps both framings and all three values `(Transport, Accepted, true)`; the control case is intact. I added `retriable == false` to the control (:179) and corrected its stale doc comment.
2. All 14 pass 1 cases are present, none ignored or weakened, all green. The 3 in `adversary_w28_endpoint.rs` have the same assertions line for line. I kept no pass 1 copy of the wire file, so that one was compared by reading every assertion.
3. Documents: no sentence naming an error code or dispatch in `docs/design.md`, the crate documentation or the CHANGELOG lines is left without a test that fails when it is false. Checked by mutant, not by the implementor's pairing: each of the seven `bounded` calls in `validate` deleted alone is caught, as are the transport and protocol sentences and the usage sentence. One sentence was held for one framing only ("whole by the transport's account" had no chunked fixture); new case :196 holds half a document in a chunked body with its last chunk to `(Protocol, Accepted, false)`.
4. Fixture pairs: none found bound to the same expression, and no table with two identical input rows, in `tests/client.rs` or `tests/classify/endpoint.rs`. One assertion cannot fail (finding 3).
5. Mutants: the seven at the implementor's named sites (stopwatch, URL, both token estimates, distinct values across questions, both member-name rules) are all caught by its new cases. Of 21 at sites it did not name, 17 are caught, 2 are equivalent and 2 survive (findings 1 and 2). Equivalent: `lib.rs:357` `final_usage` (usage is all unknown whenever the body was not whole) and the `failed` flag of the attempt (`llm-cost` pricing does not read it).
6. The correction changed no product code (`lib.rs` is documentation only). The usage rows sit on both sides of 2147483647 and hold a negative counter unknown beside a known one. Enforcing the reference bound, and reading a negative as its magnitude, both go red.

**1. The record id passes when built from a constant request id.**
`crates/llm-decide/src/classify.rs:947` with `format!("req-1/{}", self.id)` survives every binary of both crates (`adv2/logs2/summary.txt`): every fixture asks as `req-1`. The classifier-id half is caught. Guard `adversary_w28_the_record_id_is_this_requests_and_this_classifiers` (`adversary_w28_endpoint.rs:254`, two request ids through two classifier ids) goes red on it (`adv2/logs3/summary.txt`). The code is right.

**2. A boolean probability value read as the label that spells it passes.**
`crates/llm-openai-decisions/src/lib.rs:393` with `Flag(flag)` turned into the label `flag.to_string()` survives: the suite gives a boolean only against the labels `angry` and `calm`. Guard `adversary_w28_a_boolean_value_is_not_the_label_that_spells_it` (`adversary_w28_wire.rs:611`, labels `true` and `false`; boolean as probability value, as chosen value, as both) goes red on it. The code is right. What reaches the gap: a label set that has `true` or `false` as a label.

**3. One assertion of the finding 2 fix cannot fail.**
`crates/llm-openai-decisions/tests/client.rs:680`: `assert!(repeated_escaped.contains(r#""model""#))` is true of any body, since the unescaped member is always there. It does not assert that the escape is in the bytes, as the correction report says. The fixture itself at :679 is now right, and the companion assertion on `repeated_root` is real.

**4. A wall-clock test of wave 27 failed once in the gate's own command (pre-existing).**
`crates/llm-decide/tests/adversary_w27_judge_pass2.rs:169` asserts 80000 numbers take less than 8 times as long as 20000. In `adv2/run5-after.log` it measured 3.8 ms against 53.5 ms and failed while other builds ran on the host. Six repetitions alone measured 2.9 to 4.4 times (`adv2/run8-timing.log`), and the rerun of the whole command is green. Not touched by this unit; not chased. It can turn `cargo test -p llm-decide` red under load.

```findings
- {file: crates/llm-decide/src/classify.rs, line: 947, category: mutant, severity: note, verdict: CONFIRMED, origin: introduced, message: "A record id built from the constant request id req-1 survives the suite. Guard adversary_w28_the_record_id_is_this_requests_and_this_classifiers, uncommitted."}
- {file: crates/llm-openai-decisions/src/lib.rs, line: 393, category: mutant, severity: note, verdict: CONFIRMED, origin: introduced, message: "A boolean probability value read as the label that spells it survives the suite. Guard adversary_w28_a_boolean_value_is_not_the_label_that_spells_it, uncommitted."}
- {file: crates/llm-openai-decisions/tests/client.rs, line: 680, category: test-cannot-fail, severity: note, verdict: CONFIRMED, origin: introduced, message: "The assertion holds for any body; it does not check that the escape is in repeated_escaped."}
- {file: crates/llm-decide/tests/adversary_w27_judge_pass2.rs, line: 169, category: flaky-test, severity: warning, verdict: CONFIRMED, origin: pre-existing, message: "Wall-clock ratio assertion failed once under load (3.8 ms against 53.5 ms), six repetitions alone pass."}
```
