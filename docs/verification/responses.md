# Responses projection verification — 2026-09-21

Local measurements of `crates/llm-responses` at the revision this file lands on. This record adds
executable behaviour for one protocol projection. It qualifies no live provider, no credential, no
endpoint and no release; nothing here made a network call.

**Corrected after two independent adversarial reviews.** The first version claimed eleven
mutations killed while four live guards were killed by nothing: its fixtures all reported complete
usage and all stayed inside the pinned subset, so no guard firing outside it was ever reached. The
second version fixed that and shipped a harness that could not enforce the rule it stated, plus a
retained-evidence correction reachable only from a payload list no server sends. Both are answered
below, and the parts of this page that make a claim about the harness are now checked by a
self-test rather than asserted.

## What runs

| lane | command | result |
| --- | --- | --- |
| Rust | `cargo test -p b10x-llm-responses --locked --no-fail-fast` | 55 passed, 0 failed |
| adapter | `cargo test -p b10x-llm-conformance --locked` | 0 passed, 0 failed (the crate is a binary target) |
| lints | `cargo clippy -p b10x-llm-responses --all-targets --all-features --locked -- -D warnings` | exit 0 |
| lints | `cargo clippy -p b10x-llm-conformance --all-targets --all-features --locked -- -D warnings` | exit 0 |
| format | `cargo fmt -p b10x-llm-responses --check`, `cargo fmt -p b10x-llm-conformance --check` | exit 0 |
| specification | `ess specify validate --path spec` | `llm v1 — 10 file(s), valid` |

The Rust lane is three files: `tests/projection.rs`, the pinned fixtures and the class checks
(39 cases), and `tests/adversary.rs` (11) and `tests/adversary_pass2.rs` (5), written by the two
independent reviews against the claims this page and [the contract](../responses.md) make. All
three run in the same command, and `--no-fail-fast` is used so that a failure in one target does
not hide the cases in the next.

## Executable specification

The header-only `llm.responses` domain generated **zero scenarios**. The domain now declares three
commands, three observation entities and three views, and the suite selected for it holds **66
scenarios: 57 authored behavioural scenarios and 9 generated observation-adapter scenarios**, of
which 3 are this domain's own command reachability checks. Synthesis reports **0 refusals**.

```
$ ess verify conform synthesize --path spec --suite-format 5 --target ir \
    --scenarios contracts/responses/scenarios --out target/responses-suite.json
66 selected scenario(s), 57 authored source(s), 0 refusal occurrence(s)
$ cargo run --locked -p b10x-llm-conformance -- \
    target/responses-suite.json target/responses-baseline.json target/conformance/run-N local
{"total":66,"passed":66,"failed":0,"error":0,"unsupported":0,"skipped":0}
```

Three consecutive runs printed identical counts. The retained [report](responses-report.json) is
paired with the suite digest it names; detailed runs went to `target/conformance/run-{1,2,3}`.

`checks/conformance/src/responses.rs` links the real `b10x-llm-responses` and `b10x-llm-core`
libraries and the ESS revision CI installs, `a5f1bea13294510819b266561c83be9509e6ba57` (0.26.0).
No production crate depends on ESS. The adapter parses the raw binding, turn, wire body and stream
fixtures with the production decoders, calls `project_request`, `ingest_request` and
`decode_stream`, and exposes what they returned. It reads no expected assertion and no scenario
name. The observation entities, the local notifications and the read-your-writes markers belong to
this verification adapter; they imply no production message bus and no request that was ever sent.

## Does a green run detect incorrect behaviour?

**Twenty-five** temporary edits to production implementations. Every source file was restored byte
for byte and its SHA-256 re-verified. Every mutation is killed by the **named** ESS scenario *and*
the **named** Rust case recorded beside it; the harness fails outright otherwise. The table below
is generated from [responses-falsification.json](responses-falsification.json) rather than written
beside it, because the first version of this page was hand-written and two of its rows named a
case the machine record did not list as failing.

| Deliberate defect | Named failing scenario | Scenarios failed / passed | Named failing Rust case |
| --- | --- | --- | --- |
| Substitute the configured model for a missing reported one | `an-unreported-model-is-not-the-configured-one` | 1 / 65 | `an_unreported_model_stays_unknown_rather_than_the_configured_one` |
| Turn an absent cached-input count into zero | `an-absent-cached-input-tokens-stays-unknown` | 3 / 63 | `every_reported_counter_is_independently_optional` |
| Turn an absent **input** count into zero | `an-absent-input-tokens-stays-unknown` | 1 / 65 | `an_unreported_input_count_stays_unknown_and_is_never_zero` |
| Turn an absent output count into zero | `an-absent-output-tokens-stays-unknown` | 2 / 64 | `every_reported_counter_is_independently_optional` |
| Turn an absent reasoning count into zero | `an-absent-reasoning-output-tokens-stays-unknown` | 2 / 64 | `every_reported_counter_is_independently_optional` |
| Treat end of stream as terminal truth | `a-stream-without-a-terminal-response-is-refused` | 1 / 65 | `a_stream_that_never_reaches_a_terminal_response_refuses_and_keeps_its_output` |
| Drop the counters a refused terminal object reported | `a-refused-terminal-object-still-reports-its-counters` | 2 / 64 | `a_terminal_object_the_decoder_refuses_still_reports_the_counters_it_carried` |
| Return a streamed-item refusal before the terminal object is read | `an-announced-item-refusal-still-reports-the-terminal-counters` | 1 / 65 | `a_refusal_reports_the_same_evidence_whichever_event_order_produced_it` |
| Read incompleteness from the payload, not the event name | `the-terminal-event-name-decides-incompleteness` | 1 / 65 | `the_terminal_event_that_says_incomplete_is_not_read_as_a_completed_turn` |
| Widen the 64-byte incomplete-reason bound | `an-over-long-incomplete-reason-is-named-rather-than-relayed` | 1 / 65 | `an_over_long_incomplete_reason_is_named_rather_than_relayed` |
| Drop an event outside the pinned subset | `an-event-outside-the-pinned-subset-is-preserved` | 1 / 65 | `an_event_outside_the_pinned_subset_is_preserved_and_reported` |
| Filter unknown content parts out of an output message | `message-content-outside-the-subset-is-refused-not-emptied` | 1 / 65 | `a_content_part_outside_the_pinned_subset_is_not_dropped_without_a_word` |
| Relay the provider's own text into a diagnostic | `a-failed-response-retains-its-terminal-usage` | 4 / 62 | `a_failed_response_keeps_its_reported_usage_and_carries_no_upstream_text` |
| Let a counter refusal overwrite the provider's own failure class | `a-rate-limited-failure-keeps-its-class-when-its-counters-disagree` | 1 / 65 | `a_provider_failure_keeps_its_class_when_its_counters_disagree` |
| Stop binding outgoing opaque state to its binding | `a-repointed-binding-revision-refuses-opaque-state` | 3 / 63 | `opaque_state_from_any_other_binding_coordinate_is_refused` |
| Mint provenance on ingress that was never observed | `ingress-refuses-an-unmodelled-entry-it-cannot-attribute` | 2 / 64 | `ingress_does_not_reinstate_opaque_state_a_repointed_binding_refuses` |
| Skip the tool-name class on ingress | `ingress-refuses-a-tool-name-egress-cannot-publish` | 1 / 65 | `ingress_refuses_a_tool_name_egress_cannot_publish` |
| Skip the tool-name class on egress | `an-unpublishable-tool-name-is-refused` | 1 / 65 | `a_tool_name_this_wire_cannot_publish_is_refused_before_it_is_sent` |
| Read an omitted fixed field as agreement | `ingress-refuses-a-body-that-omits-a-fixed-field` | 3 / 63 | `every_fixed_field_is_refused_unless_it_carries_exactly_the_pinned_value` |
| Test `include` for membership instead of equality | `ingress-refuses-an-empty-include` | 1 / 65 | `every_fixed_field_is_refused_unless_it_carries_exactly_the_pinned_value` |
| Join two content parts instead of refusing them | `ingress-refuses-a-message-of-two-content-parts` | 1 / 65 | `a_message_this_version_cannot_represent_part_for_part_is_refused` |
| Remove the published-tool field guard | `ingress-refuses-a-published-tool-field-outside-the-subset` | 1 / 65 | `every_ingress_content_refusal_the_contract_promises_is_reachable` |
| Accept an ingress field outside the subset | `ingress-refuses-a-field-outside-the-subset` | 1 / 65 | `a_body_field_outside_the_pinned_subset_is_refused_on_ingress` |
| Address the wire with the caller's alias | `text-turn-projects-the-pinned-body` | 8 / 58 | `the_wire_model_is_the_upstream_name_and_the_alias_never_substitutes_for_it` |
| Drop the tool-result failure flag | `a-failed-tool-result-keeps-its-failure` | 1 / 65 | `a_failed_tool_result_keeps_its_failure_through_the_round_trip` |

Every mutation produced zero target errors and zero unsupported observations. Each changed a
production fact the suite observes; no expected assertion, timeout or baseline was changed to
produce a pass.

### The harness itself was the finding, twice

Round 1 answered "a record can claim more than it checks" by making the harness fail when a
mutation was killed by neither lane. Reading that harness closely, it could not do what the
sentence said: a mutation that failed to compile was recorded `killed_by: "compilation"` and never
reached the check; any Rust failure counted, not the named one; the conformance lane ignored the
run's exit status and re-read a report a failed run may not have written; and without
`--no-fail-fast` an early failing target hid every case after it.

All four are fixed, and the fix is checked rather than asserted. `falsify-selftest.py` runs three
probes through the same harness:

```
selftest-no-op-comment: scenarios=0 rust=0 killed_by=NOTHING
selftest-does-not-compile: scenarios=0 rust=0 killed_by=NOTHING
selftest-wrong-case-named: scenarios=8 rust=9 killed_by=['scenario']
UNDEFENDED: selftest-no-op-comment: expected scenario 'text-turn-projects-the-pinned-body' in [] ...
UNDEFENDED: selftest-does-not-compile: the mutation does not compile; rewrite it so that it does
UNDEFENDED: selftest-wrong-case-named: expected ... case 'a_case_that_does_not_exist' in [...]
EXIT=1
```

A no-op edit, an edit that does not compile, and a real defect pointed at a case that does not
exist are all refused. The third also shows the `--no-fail-fast` fix: nine cases across more than
one target are listed where the old harness would have stopped at the first.

The real run caught one of its own rows this way before it shipped: `ingress-skips-the-tool-name-class`
named the adversarial case `a_tool_name_ingress_accepts_is_one_egress_can_publish`, and the case
that actually dies is this crate's own `ingress_refuses_a_tool_name_egress_cannot_publish`.

### Five adversarial cases are no longer discriminating, and their replacements are named

Requiring every fixed field to be present changed which guard fires first, so five cases in
`adversary.rs` and `adversary_pass2.rs` now take their acceptable `Err(Unsupported)` branch for a
reason other than the one they were written to drive. Their assertions still hold and none was
touched; the discriminating case for each intent lives in `projection.rs`, on a body that is
complete:

| intent | now non-discriminating | discriminating case |
| --- | --- | --- |
| a strict tool schema is not inverted | `a_strict_tool_schema_is_either_carried_or_refused_but_not_quietly_inverted` | `every_fixed_field_is_refused_unless_it_carries_exactly_the_pinned_value` |
| ingress refuses a tool name egress cannot publish | `a_tool_name_ingress_accepts_is_one_egress_can_publish` | `ingress_refuses_a_tool_name_egress_cannot_publish` |
| a published tool field outside the subset | `ingress_refuses_a_published_tool_field_outside_the_pinned_subset` | `every_ingress_content_refusal_the_contract_promises_is_reachable` |
| two content parts are not joined | `a_message_of_two_content_parts_reprojects_as_it_arrived` | `a_message_this_version_cannot_represent_part_for_part_is_refused` |
| `store` and `stream` guards | `ingress_refuses_provider_side_storage_and_a_non_streaming_body` | `every_fixed_field_is_refused_unless_it_carries_exactly_the_pinned_value` |

Every ingress table in `projection.rs` now begins by asserting that the **unedited** body is
accepted, so a refusal it records is caused by the one edit under test and not by a guard that
fires before it.

## Two classes the suite checks rather than a list

A hand-written list that only an adversary extends is the defect; a missing entry is its symptom.
Four Rust cases drive a whole rule instead of one instance of it, and each one has caught a real
defect:

| case | rule it drives | what it caught |
| --- | --- | --- |
| `every_event_the_pin_declares_accepted_is_actually_interpreted` | every name in `ACCEPTED_STREAM_EVENTS` is interpreted, not warned about | — |
| `every_body_field_the_pin_declares_accepted_survives_ingress` | every name in `ACCEPTED_BODY_FIELDS` is read on ingress | `top_p` was projected and absent from the list, so ingress refused a body this crate produces |
| `every_reported_counter_is_independently_optional` | each of the five counters absent in turn, the other four present | the class the three counter-default mutations belong to |
| `every_body_ingress_accepts_reprojects_unchanged` | anything ingress accepts, egress sends back **byte-identically** | the class `strict` and the ingress tool-name check belong to; strengthened from a field-wise comparison, which could not see a value rewritten inside a field the body did send |
| `every_fixed_field_is_refused_unless_it_carries_exactly_the_pinned_value` | each of the four fixed fields, driven absent, empty and different | omission read as agreement, and `include: []` passing because `all` over nothing is true |
| `a_refusal_reports_the_same_evidence_whichever_event_order_produced_it` | one refusal, both event orders a server may use, same evidence | a correction that was only reachable from an order no server sends |

The ESS domain carries the same two rules as observable facts. `RequestIngestion` gained
`reprojected` and `body_preserved` in this round: the first is false when a gateway accepts a
request its own outgoing side refuses to forward, the second when a field is accepted, never read
and silently rewritten on the way back out. Before they existed, no scenario could see an
ingress-to-egress disagreement at all, which is the structural reason two of the four blockers
were invisible to all 43 scenarios of the first suite.

## One gap that is not closed in code, and is not closed by pretending

`ingest_request` **refuses** an `input` entry outside the four modelled shapes. It cannot do better:
the wire body carries no provenance, `Item::Opaque` has exactly one representation — bound to a
binding — and stamping it with the binding doing the reading laundered state that
`project_request` refuses into state `project_request` sends. Refusing is sound and costs reasoning
continuity across a tool round trip through a gateway, which is a real cost.

Closing it properly needs a neutral state meaning *carried, attribution unverified*. That is a
change to `llm-core` and to the versioned `docs/contract-v1.md`, neither of which belongs to this
story; it is recorded as a request for the coordinator, and
[the contract page](../responses.md) states the asymmetry as a property rather than hiding it.

## What the second review verified and could not break

Recorded because it bounds what the rest of this page claims. Ingress mints no provenance
anywhere and the laundering finding is closed; the seven-site enumeration in `terminate` is
correct about that function and its one deliberate exception is the only one in it; the rewritten
opaque-state case asserts something real rather than merely true; and all eighteen ingress
fixtures were checked for a second guard that could fire first with the same code, and none was
found at the time. That last property is now a machine check rather than a review finding, since
this round's fixed-field rule is exactly the change that would have broken it.

## What this does not establish

- Nothing about a live `OpenAI` or vLLM endpoint. No request was sent; the fixtures are literals.
- Nothing about server-sent-event framing, which is `llm-http`'s and is verified there. This crate
  consumes decoded payloads.
- Nothing about an HTTP client, retry, fallback, credential presentation or billing, each of which
  is its own boundary and none of which is implemented here.
- The `MAX_ITEMS` bound on decoded output is not reached by any fixture; 4096 items is an
  expensive fixture for a bound `llm-core` already owns.
- The fixed-field rule makes this ingress surface strict: a body that does not carry `stream`,
  `store`, `include` and each tool's `strict` is refused. No measurement here says what fraction
  of real clients send all four.
- The vLLM behaviour cited in [the contract](../responses.md) is quoted from
  `verification-report:openai-responses-on-vllm` in `beyond10x/harness`, a run against
  `vllm/vllm-openai:v0.27.1` on different weights and a different host. It is evidence about that
  server's HTTP surface and about nothing else, and this record does not extend it.
