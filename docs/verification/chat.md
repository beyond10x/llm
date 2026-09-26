# Chat Completions verification — 2026-09-21

Local measurements for the `llm-chat` projection at the revision this file lands in. They
qualify no live provider, no `vLLM` deployment, no gateway and no release. Nothing below
involved a paid call or a network endpoint outside loopback.

This record has been corrected twice under adversarial review. Both corrections are stated
below in their own words rather than left to read as if they had always been right.

## What runs

The projection is exercised twice over: by the crate's own Rust suite, and by authored ESS
scenarios observing the same public functions through the conformance adapter.

| Lane | Command | Result |
| --- | --- | --- |
| Rust | `cargo test -p b10x-llm-chat --locked` | 60 passed, 0 failed |
| ESS | `cargo run --locked -p b10x-llm-conformance -- target/chat-suite.json target/chat-baseline.json target/chat-run local` | 60 passed, 0 failed, 0 error, 0 unsupported, 0 skipped |

The Rust figure includes the eleven adversarial cases in `tests/adversary.rs` and
`tests/adversary_pass_2.rs`, which are kept in the suite and run by the ordinary gate.

The ESS suite is synthesized with the pinned ESS `0.26.0`
(`a5f1bea13294510819b266561c83be9509e6ba57`), the same revision CI installs and the same one
`checks/conformance` links. No production crate depends on ESS. Synthesis reports **0
refusals**; the suite holds **50 authored** behavioural scenarios and **10 generated** adapter
observation scenarios. Three consecutive local runs reported identical counts.

`checks/conformance/src/chat.rs` calls the real public functions — `project_request`,
`project_response_bytes`, `StreamProjection::accept`, `StreamProjection::finish`,
`decode_completion`, `decode_ingress_request`, `encode_ingress_completion` and
`IngressStream::close` — and exposes what they returned. It builds fixture bindings through
the real `BindingDocument::bind`, reads suite expectations never, and branches on a scenario
name never. Its entity, local notification and read-your-writes marker belong to this
verification adapter; they imply no production event bus, persisted projection record or
provider call.

## Fixtures

`crates/llm-chat/fixtures/*.sse` are pinned response bytes in the shapes this wire is served
in: two `OpenAI`-shaped streams, two `vLLM`-shaped ones, and one whose usage report names a
single counter. They are **authored to the documented shape, not captured from a live
server** — including the `vLLM` ones, which carry that server's `reasoning_content`,
`usage: null` and `stop_reason` fields. The ESS scenarios under `contracts/chat/scenarios/`
were generated from those same files and carry the same bytes inline, because a scenario
input is literal.

`tests/local_endpoint.rs`, `tests/adversary.rs` and `tests/adversary_pass_2.rs` serve pinned
bytes over a loopback socket and drive `ChatClient` through `llm_core::Model`, so the
outgoing request, the HTTP transport, the SSE framing, the projection, a bounded sink
refusing an event and the evidence a failure retains are all exercised end to end. They are
local fixtures. They establish no live `vLLM` qualification.

## Two claims this record got wrong

**The absent-counter class.** An earlier revision said the class had been closed rather than
its instances. That was false, and measured false: a mutation defaulting `input_tokens` to
zero survived all 43 Rust cases and all 46 scenarios then in the suite, because no `usage`
object anywhere in the decode direction omitted `prompt_tokens`. What closed it is a rule.
`removing_any_one_reported_counter_leaves_exactly_that_one_unknown` takes a report naming
every counter this wire carries, removes exactly one, and asserts only that one becomes
unknown — and asserts that the complete report fills every field of the neutral `Usage`, so
a counter added there forces the table to grow with it.
`encoding_places_every_counter_the_neutral_value_carries_and_only_those` is the same rule on
the encode side, which the first correction left as three exact-equality instances that
merely happened to span today's five counters.

**The count of entry points.** The dispatch correction is applied once per public entry point
of the decode module. This page, the module's own comment and the structural case all said
there were two. There are three: `StreamProjection::accept` is exported and documented as the
entry point for a caller that already has framed events, and it was the one without the
correction, for six refusals. Nothing failed, because the coverage was a sentence in three
places rather than a measurement anywhere.
`every_exported_decode_entry_point_applies_the_dispatch_correction` now derives the set from
`src/incoming.rs` itself — every exported function whose signature can carry a neutral
`Error` — and requires a refusal driven through each one. A fourth entry point fails that
case until it is driven too. The conformance adapter grew a `framed` observation mode so the
ESS lane reaches `accept` directly rather than only through the composing wrapper that
happened to hide the gap; `accept-refusal-says-nothing-was-sent` now kills four scenarios
where it would previously have killed none.

The same correction applies one layer up, where `retain` read a refusal's own `not-sent` as
proof nothing had been dispatched. Inside `turn` every error raised after the transport
returns is after dispatch; three callers reach that branch and none of them legitimately
claims `not-sent`.

## Does a green run detect incorrect behavior?

Thirty temporary edits to production sources. Every one failed a named Rust case or a named
scenario; **no mutation survived**. Every file was restored byte for byte and its SHA-256
re-checked against the pre-edit digest after each run; the digests are in
[chat-falsification.json](chat-falsification.json) beside each record, together with the full
list of cases and scenarios each mutation killed.

| Source | Deliberate defect | First named failure | Rust / scenarios failed |
| --- | --- | --- | --- |
| `client.rs` | `client-attaches-an-invalid-snapshot` | case `a_snapshot_that_cannot_validate_does_not_travel_with_its_failure` | 1 / 0 |
| `client.rs` | `client-discards-evidence-after-dispatch` | case `a_protocol_failure_after_dispatch_keeps_the_counters_the_endpoint_reported` | 1 / 0 |
| `client.rs` | `client-never-reaches-the-callers-sink` | case `a_refusal_raised_while_reading_the_stream_is_not_evidence_that_nothing_was_sent` | 2 / 0 |
| `client.rs` | `client-reads-its-own-not-sent-as-proof` | case `a_refusal_of_the_finished_outcome_is_not_evidence_that_nothing_was_sent` | 2 / 0 |
| `client.rs` | `client-sends-a-non-streamed-request` | case `a_local_vllm_compatible_endpoint_streams_through_the_neutral_port` | 1 / 0 |
| `incoming.rs` | `a-non-integer-choice-index-is-choice-zero` | case `no_refusal_in_the_decode_direction_reports_that_nothing_was_sent` | 1 / 1 |
| `incoming.rs` | `absent-counter-becomes-zero` | case `a_usage_report_that_omits_the_input_counter_leaves_it_unknown` | 1 / 8 |
| `incoming.rs` | `accept-refusal-says-nothing-was-sent` | case `the_third_public_entry_point_also_refuses_without_claiming_nothing_was_sent` | 1 / 4 |
| `incoming.rs` | `cache-write-counter-deleted-on-read-back` | case `the_cache_write_counter_this_wire_names_survives_being_read_back` | 1 / 5 |
| `incoming.rs` | `framing-refusal-says-nothing-was-sent` | case `every_exported_decode_entry_point_applies_the_dispatch_correction` | 2 / 0 |
| `incoming.rs` | `input-counter-defaulted-to-zero` | case `a_usage_report_that_omits_the_input_counter_leaves_it_unknown` | 1 / 1 |
| `incoming.rs` | `missing-finish-reason-invented` | case `a_stream_that_ends_without_a_finish_reason_is_refused_and_keeps_its_prefix` | 2 / 1 |
| `incoming.rs` | `non-streamed-refusal-says-nothing-was-sent` | case `every_exported_decode_entry_point_applies_the_dispatch_correction` | 2 / 1 |
| `incoming.rs` | `reasoning-counter-defaulted-to-zero` | case `a_protocol_failure_after_dispatch_keeps_the_counters_the_endpoint_reported` | 2 / 4 |
| `incoming.rs` | `streamed-refusal-says-nothing-was-sent` | case `a_contradictory_counter_refusal_is_not_evidence_that_nothing_was_sent` | 2 / 4 |
| `incoming.rs` | `tool-argument-fragments-dropped` | case `a_no_argument_tool_call_reaches_the_caller_as_an_empty_object` | 2 / 2 |
| `incoming.rs` | `truncated-stream-accepted` | case `a_stream_truncated_before_its_terminal_sentinel_is_refused` | 2 / 2 |
| `incoming.rs` | `two-choices-in-one-chunk-joined` | case `a_second_choice_is_refused_however_the_chunk_numbers_it` | 1 / 1 |
| `incoming.rs` | `unreported-model-replaced-by-the-binding` | case `an_unreported_upstream_model_is_never_replaced_by_the_configured_one` | 1 / 1 |
| `ingress.rs` | `absent-counter-reported-as-zero-on-egress` | case `a_reported_cache_write_counter_is_named_rather_than_dropped` | 3 / 2 |
| `ingress.rs` | `cache-write-counter-dropped-on-egress` | case `the_cache_write_counter_this_wire_names_survives_being_read_back` | 1 / 1 |
| `ingress.rs` | `configured-model-substituted-on-egress` | case `an_encoded_response_omits_a_model_the_upstream_never_reported` | 1 / 1 |
| `ingress.rs` | `encode-drops-the-reasoning-counter` | case `an_encoded_response_carries_reported_counters_and_the_reported_model` | 2 / 1 |
| `ingress.rs` | `ingress-accepts-what-the-neutral-subset-refuses` | case `a_request_the_neutral_subset_refuses_is_refused_at_ingress` | 1 / 3 |
| `ingress.rs` | `unknown-request-field-accepted` | case `an_unrecognized_field_is_refused_without_echoing_the_clients_bytes` | 1 / 1 |
| `ingress.rs` | `unsupported-request-field-accepted` | case `every_field_outside_the_published_subset_is_refused_by_its_own_name` | 1 / 2 |
| `outgoing.rs` | `deprecated-output-limit-field` | case `a_text_turn_projects_the_upstream_model_messages_and_usage_options` | 1 / 1 |
| `outgoing.rs` | `failed-tool-result-flattened` | case `a_failed_tool_result_is_never_projected_as_a_successful_one` | 1 / 1 |
| `outgoing.rs` | `opaque-state-dropped-on-the-way-out` | case `opaque_state_is_refused_rather_than_dropped` | 1 / 0 |
| `outgoing.rs` | `route-alias-sent-as-the-model` | case `a_local_vllm_compatible_endpoint_streams_through_the_neutral_port` | 1 / 1 |

Every mutation produced zero target errors and zero unsupported observations. No expected
assertion, timeout or baseline was changed to produce a pass.

## These scenarios do not yet run in the repository gate

`ess verify conform synthesize --scenarios contracts` selects what `contracts/ess-inputs.yaml`
names, not what the tree holds. That manifest does not list `contracts/chat/scenarios/`, so
the committed gate — `cargo run -p b10x-llm-conformance -- check` — synthesizes **183**
scenarios with these 50 absent, and exits 0 while doing it. The measurements above come from
synthesizing `--scenarios contracts/chat/scenarios` directly, which selects all 50.

With the manifest extended, the whole-repository suite synthesizes **237** scenarios (227
authored, 10 generated) and all 237 pass, which is what `contracts/baseline.json` then has to
floor at. The manifest, the committed suite, the generated schemas and the baseline are
regenerated centrally and are not this crate's to edit.

## What these measurements do not cover

- **`crates/llm-chat/src/client.rs` is observed by no scenario.** The conformance adapter
  opens no socket, so the ESS lane says nothing about the client. It is covered instead by
  eight loopback-socket cases and by six falsification entries, each of which dies on a named
  Rust case and on zero scenarios. A reader trusting the scenario counts alone would be
  trusting a lane that never ran this file.
- Cancellation and deadline handling on an outgoing call are `llm-http`'s, already covered
  there. No case here drives a cancelled or timed-out attempt through `ChatClient`.
- The refusal of opaque state whose provenance **matches** the selected binding is covered by
  the Rust suite only. An authored scenario cannot carry it: the binding revision is a hash of
  the whole validated declaration, so a matching one cannot be written by hand. The ESS
  scenario covers the cross-binding refusal, which `llm-core` owns.
- Streamed gateway egress names a call where the neutral stream announced it
  (`StreamEvent::ToolCallStarted`, added by `story:streamed-tool-call-name`) and streams its
  arguments under that index. Only a call the stream never announced is still emitted complete
  in the terminal chunks; that is the case of a model that returns calls only in its outcome.
- A failure whose earned snapshot cannot pass `Error::validate_for` — contradictory reported
  counters are how that happens — travels with no observation at all. The counters are lost
  to the caller in that one case; attaching them would make the failure itself unreportable.
- A chunk carrying exactly one choice that omits its index is read as the single choice this
  projection asked for. Two such choices in one chunk are refused, but a server that split
  two completions across separate unindexed chunks would be indistinguishable from one. No
  server is known to do that, and `n` is never sent as anything but its default.
