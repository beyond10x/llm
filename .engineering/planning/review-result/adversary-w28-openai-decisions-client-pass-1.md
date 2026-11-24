---
format: aep.planning-md/3
id: review-result:adversary-w28-openai-decisions-client-pass-1
kind: review-result
status: active
title: 'Adversary, pass 1 (story:openai-decisions-client, wave 2026-11-24-openai-decisions-client): 12 introduced, 1 red case'
relations:
- reviews: story:openai-decisions-client
revision: 1
---
Recorded by the coordinator as the adversary returned it; nothing is changed. Covers head `21f8cc1bb`
of `impl/openai-decisions-client`.

unit: story:openai-decisions-client
verdict: red
cases: executed 43→57, red 1
origin: introduced 12, pre-existing 0, undecided 0
wrote-outside-worktree: the wave scratch directory `adv/` only (30 MB: logs, two throwaway sh scripts, `adv/mutants/ws` = the workspace copy with its `target/` removed)
needs-coordinator: no

```
 crates/llm-decide/tests/classify.rs         | 3 +++
 crates/llm-openai-decisions/tests/client.rs | 3 +++
 2 files changed, 6 insertions(+)
untracked: crates/llm-decide/tests/classify/adversary_w28_endpoint.rs (3 cases)
untracked: crates/llm-openai-decisions/tests/client/adversary_w28_wire.rs (11 cases)
```

Counts are the two test binaries I touched (`client` 23→34, `classify` 20→23). "Before" is a run after my cases existed with `--skip adversary_w28_`, which excludes exactly the two untracked files (`adv/run6-before.log`); "after" is `adv/run9-after-formatted.log`. No case of mine runs over 5 s (each binary finishes in 0.4 s). `cargo fmt --all --check` and clippy with `-D warnings` on both crates pass with my files in place. No scenario added or changed, so the conformance check was not run and no `.ess-0.87.0/` was created. I took no worktree lease (no tool for it in this agent).

**1. A body cut short of its declared length is `transport` and retriable, not `protocol` (red).**
`crates/llm-openai-decisions/tests/client/adversary_w28_wire.rs:193` (assertion at :208, `adv/run4-cut-body-case.log`). A 200 answer with half the example body under a full `content-length`, and the same half as one chunk of a chunked body with no terminator, both come back `(Transport, Accepted, retriable: true)`, message "HTTP response body failed". `docs/design.md:872`, the crate doc `crates/llm-openai-decisions/src/lib.rs:22` and `CHANGELOG.md:19` each say a body cut short is `protocol`. Only a cut where the connection's end is the body's end is `protocol` (control case :177, green). What reaches it: any connection dropped mid-body by a server that declares its length, the ordinary cut; the classifier then abstains `ProviderError(transport)`. The error comes from `crates/llm-http/src/transport.rs:463`, which predates the unit; the claim is the unit's. Fix either side: correct the three documents, or map it in `send`.

**2. The suite's escaped-name fixture escapes nothing.**
`crates/llm-openai-decisions/tests/client.rs:667`: `repeated_escaped` is the same expression as `repeated_root` at :657. "However its name is escaped" (`docs/design.md:873`) had no test. My guard `adversary_w28_a_member_repeated_under_an_escaped_name_is_refused` (wire file :225, response `model` and request `input`) is green: the code is right, the test could not fail for that class.

**3. Nine mutants the suite misses, each now with a guard that goes red on it.**
One mutant per build on the scratch copy, `cargo test --no-fail-fast --lib --tests` of the touched crate(s); `adv/mutants/logs/summary.txt`, and `adv/mutants/logs2/` for the price mutant. The conformance scenarios were not run against mutants. The code is correct today at every site; these are gaps in the tests.
- `lib.rs:157` size check removed: the transport still refuses `too-large`, but after the credential is resolved and a record written. Guard :306.
- `lib.rs:191` returned `model` taken from the catalog, and `lib.rs:280` recorded `upstream_model` taken from the catalog: every suite body names the model the catalog asks for. Guard :345.
- `lib.rs:334` price picked as "first declared" only: the suite's book has one price. Guard :345. My first version of this guard missed it too, because a price book sorts by id; the ids are now ordered so the reported model's price is second.
- `lib.rs:283` an unsent call priced anyway. Guard :260 (credential refused: one record, `not-sent`, no cost, no request).
- `lib.rs:250` `accept` header renamed. Guard :399.
- `request.rs:231` `text.len() <= max + 1`: every bound test uses two-byte characters. Guard :477 (one-byte texts at 1048576/1048577, 10485760/10485761, 128/129, 64/65).
- `crates/llm-decide/src/classify.rs:1033` any record counts as the call's own. Guard `adversary_w28_endpoint.rs:170`.
- `classify.rs:956` cancel check before the call removed. Guard `adversary_w28_endpoint.rs:192`.

**4. Usage counters are bounded differently from the reference.**
`crates/llm-openai-decisions/src/response.rs:161` takes any whole number in `u64`. The reference's `ResponseUsageResource` declares each counter `int64` with `minimum: -2147483648`, `maximum: 2147483647`. A counter above 2147483647 is taken; a negative one is unknown. No case written: read, not measured. Reach: nothing found; only an endpoint writing a counter outside its own schema.

**Attacked and not broken**
- Caught by the suite: 27 mutants, covering every one the brief names. Question count both sides, choices and levels both sides and each constant, every character bound constant, duplicate choice value, repeated-member check, depth 16→17, name check, answer of another type, answer count, usage contradiction, rounding at a half, range check, repeated and off-set labels, record kind, cancel and protocol checks before I/O.
- One equivalent mutant: `lib.rs:187` (missing `model`) is unreachable, `response.rs:140-144` refuses first.
- `basis_points` against an independent decimal-expansion oracle on more than 80,000 doubles: every k/20000 with both neighbours, exact ties, every binary exponent from the smallest subnormal. Green.
- Number forms through the wire (`-0.0`, `1E0`, `5e-324`, `1e-400`, `1.0000000000000002`, largest double, `"0.5"`, `1e400`): as documented.
- Strict reader cost at 1, 2 and 4 MiB of long numbers and escapes, fastest of three runs: doubles with size, no quadratic growth.
- One request and one record for 500, 503, 529, 408, 307, 302; no redirect followed. Another media type is refused. Cancel while the body arrives is recorded cancelled.
- Request field names, enum values and bounds against the reference; the conformance adapter runs the real client and classifier; the six scenarios against the code (`0.34875` → 3488 by hand).

```findings
- {file: crates/llm-openai-decisions/src/lib.rs, line: 22, category: contract-drift, severity: warning, verdict: CONFIRMED, origin: introduced, message: "A 200 body cut short of its content-length or mid-chunk is transport/accepted/retriable; design.md:872, the crate doc and CHANGELOG.md:19 say protocol. Red case adversary_w28_a_body_cut_short_of_its_declared_length_is_refused_protocol."}
- {file: crates/llm-openai-decisions/tests/client.rs, line: 667, category: test-cannot-fail, severity: warning, verdict: CONFIRMED, origin: introduced, message: "repeated_escaped is identical to repeated_root (:657); the escaped-name claim was untested. Guard added, green."}
- {file: crates/llm-openai-decisions/src/lib.rs, line: 157, category: mutant, severity: warning, verdict: CONFIRMED, origin: introduced, message: "Removing the size check survives the suite: too-large then comes after credential resolution and writes a record. Guard adversary_w28_a_body_over_the_bound_never_reaches_the_credential_or_the_record."}
- {file: crates/llm-openai-decisions/src/lib.rs, line: 191, category: mutant, severity: warning, verdict: CONFIRMED, origin: introduced, message: "Returning the catalog's model instead of the body's survives the suite. Guard adversary_w28_the_model_the_body_names_is_returned_recorded_and_priced."}
- {file: crates/llm-openai-decisions/src/lib.rs, line: 280, category: mutant, severity: warning, verdict: CONFIRMED, origin: introduced, message: "Recording the catalog's model as upstream_model survives the suite. Same guard."}
- {file: crates/llm-openai-decisions/src/lib.rs, line: 334, category: mutant, severity: warning, verdict: CONFIRMED, origin: introduced, message: "Picking the first declared price regardless of the reported model survives the suite (one price in its book). Same guard, two prices."}
- {file: crates/llm-openai-decisions/src/lib.rs, line: 283, category: mutant, severity: warning, verdict: CONFIRMED, origin: introduced, message: "Pricing a call that was not sent survives the suite. Guard adversary_w28_a_call_whose_credential_is_refused_writes_one_unsent_unpriced_record."}
- {file: crates/llm-openai-decisions/src/lib.rs, line: 250, category: mutant, severity: note, verdict: CONFIRMED, origin: introduced, message: "Renaming the accept header survives the suite. Guard adversary_w28_the_request_asks_for_json_and_another_media_type_is_refused."}
- {file: crates/llm-openai-decisions/src/request.rs, line: 231, category: mutant, severity: warning, verdict: CONFIRMED, origin: introduced, message: "text.len() <= max + 1 survives the suite: bounds are tested with two-byte characters only. Guard adversary_w28_a_text_of_one_byte_characters_is_held_to_the_same_bounds."}
- {file: crates/llm-decide/src/classify.rs, line: 1033, category: mutant, severity: warning, verdict: CONFIRMED, origin: introduced, message: "Counting any record as the call's own survives the suite. Guard adversary_w28_a_record_under_another_id_is_not_named_as_the_calls_own."}
- {file: crates/llm-decide/src/classify.rs, line: 956, category: mutant, severity: note, verdict: CONFIRMED, origin: introduced, message: "Removing the cancel check before the endpoint call survives the suite. Guard adversary_w28_a_chain_cancelled_before_the_call_asks_no_endpoint."}
- {file: crates/llm-openai-decisions/src/response.rs, line: 161, category: reference-drift, severity: note, verdict: CONFIRMED, origin: introduced, message: "Usage counters are taken over 0..u64::MAX; the reference bounds them to -2147483648..2147483647. No case; nothing found that reaches it."}
```
