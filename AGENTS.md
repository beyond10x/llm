# Working on LLM

What llm is and how a person uses it is in [README.md](README.md) and on the site,
<https://beyond10x.github.io/llm/>. This file is what an agent changing the repository needs.

## Serves

Organisation objectives O1 (explicit model reach, credentials and spending decisions), O3 (one
neutral inference interface for several agent harnesses) and O6 (attributable usage, costs and
failures), as `.engineering/planning/vision/portable-model-inference.md` records.

## Owns, and does not own

llm owns the client side of model inference: the neutral turn (`llm-core`), the transport, the
three protocol crates (each with its ingress decoder and encoder, and a usage reader that reads
an answer's model and counters from its bytes), the Amazon Bedrock Runtime
client (`llm-bedrock`: Converse, Anthropic `InvokeModel`, Signature Version 4, event-stream
frames, static, container and web identity AWS credentials), credentials, provider bindings,
routing, per-call records (`llm-calls`), closed-label classification and rubric verdicts
(`llm-decide`), the OpenAI `/v1/decisions` wire and the classifier's decision-endpoint path
(`llm-openai-decisions`), embeddings over
an embed route (`llm-embed`, the OpenAI `/v1/embeddings` wire), port
construction from a catalog or an `llm.routes/1` routes document in YAML or TOML, Bedrock
endpoints included (`llm-models`), pricing,
`call_tool`, the blocking adapter and the `llm` command (`llm-cli`: `model list`, `infer` and `replay`
over one `llm.toml`; it injects only the `environment` and `file` sources, no keychain). The
accepted design is `docs/design.md` (its "Client request fields" section is the rule for every
field an ingress decoder reads); per-protocol detail is in `docs/responses.md`,
`docs/messages.md`, `docs/chat.md` and `docs/bedrock.md`.

It does not own:

- **Serving.** The gateway, hosting and provisioning crates (`llm-gateway`,
  `llm-provision`, `llm-runpod`, `llm-modal`) and the `llm.gateway`, `llm.hosting`
  and `llm.runpod` ESS domains live in [beyond10x/llm-gateway](https://github.com/beyond10x/llm-gateway)
  since 0.2.0. Change them there; their archived stories stay in this store as history. This
  repository's site, README.md and status record describe none of it: they link that repository.
- **Agent loops, tool execution and approval.** A tool here is a name, a description and a JSON
  Schema. The caller runs it.
- **Credential custody and login flows.** Secrets come from a resolver the caller injects. The
  one exception is an `aws-process` Bedrock endpoint, whose command `llm-bedrock` runs to print
  AWS `credential_process` keys, and only for a caller that opts in with
  `RouteResolvers::allow_credential_process` on a build with feature `process`.
- **Its consumers.** No crate here depends on a consumer repository.

Protocol, provider/account, credential source, model and hosting provider stay distinct concepts.
A provider adapter never changes an endpoint or a billing account on its own.

## Invariants and the tests that hold them

Each claim fails the named test when broken. Weakening a test weakens the claim, so it needs a
story that says so.

| Invariant | Held by |
| --- | --- |
| No workspace crate depends on a consumer; `llm-core` has no transport dependency; `llm-routing` depends on no protocol crate | `crates/llm-core/tests/dependency_boundary.rs::inference_workspace_does_not_depend_on_its_consumers_and_core_has_no_transport` |
| The `secrets` library is reached only through `llm-credentials` feature `secrets`: optional, off by default, tag `v0.5.0` at the commit the lockfile records, forwarded by no core crate | `crates/llm-credentials/tests/secrets_boundary.rs::the_secrets_library_is_reached_only_through_the_credentials_feature` (moving the tag means moving `TAG` and `REVISION` there) |
| Unknown usage stays unknown, never zero | `crates/llm-core/tests/embedding.rs::unknown_usage_stays_distinct_from_zero_and_invalid_subsets_refuse`, and per protocol `crates/llm-responses/tests/adversary.rs::an_unreported_input_count_stays_unknown_and_is_never_zero`, `crates/llm-responses/tests/projection.rs::a_cache_write_count_is_kept_under_either_spelling_and_a_missing_one_stays_unknown`, `crates/llm-messages/tests/streaming.rs::a_counter_the_route_never_reported_stays_unknown`, `crates/llm-chat/tests/incoming.rs::a_counter_a_present_usage_report_omits_stays_absent_and_never_becomes_zero`, `crates/llm-chat/tests/incoming.rs::removing_any_one_timings_counter_leaves_unknown_exactly_what_it_counted`; ingress encoders `crates/llm-responses/tests/ingress.rs::an_unknown_input_count_is_omitted_and_decodes_back_unknown_never_zero`, `crates/llm-messages/tests/ingress.rs::an_unknown_output_count_is_left_out_and_decodes_back_unknown`; usage readers `crates/llm-responses/tests/usage_reader.rs::a_counter_the_answer_did_not_report_stays_unknown_and_is_never_zero`, `crates/llm-messages/tests/usage_reader.rs::a_counter_the_route_never_reported_stays_unknown_and_is_never_zero`, `crates/llm-chat/tests/usage_reader.rs::a_counter_a_usage_report_omits_stays_unknown_and_is_never_zero`; Bedrock Converse, whose body never carries a `cachePoint`, so an absent cache counter is a known 0 and only an absent `inputTokens` leaves the input unknown: `crates/llm-bedrock/tests/converse.rs::an_absent_cache_counter_is_a_known_zero_on_converse`, `crates/llm-bedrock/tests/converse.rs::the_converse_body_never_carries_a_cache_point`, `crates/llm-bedrock/tests/converse.rs::cache_counters_add_up_to_the_whole_input`; the command `crates/llm-cli/tests/command.rs::infer_reads_the_prompt_from_stdin_and_an_unreported_counter_prints_unknown` |
| A call record holds no text of the turn: no instructions, prompt, tool description, answer, reasoning, tool argument or error message | `crates/llm-calls/tests/record.rs::a_serialized_record_holds_none_of_the_turns_text`; scenario `contracts/calls/scenarios/a-routed-turn-writes-one-record-with-no-text.yaml` |
| No UTC day of one `ShadowLedger` spends past its shadow cap beyond the excess of the shadows in flight at its last admission; an unknown cost is held, never settled as zero; the cap is per process unless the ledger is durable (`ShadowLedger::durable`, `sqlite` feature) | `crates/llm-calls/tests/security_u4_cap.rs::no_sequence_spends_a_day_past_its_cap_beyond_the_excess_of_shadows_in_flight`, `crates/llm-calls/tests/security_u4_cap.rs::a_self_shadow_and_a_shadow_of_a_shadow_route_are_capped_and_never_chain`, `crates/llm-calls/tests/shadow.rs::two_shadows_in_flight_cannot_both_take_the_last_reservation`, `crates/llm-calls/tests/shadow.rs::an_incomplete_shadow_cost_is_held_at_its_reservation_and_never_settled_as_zero` |
| Secret material is never a configuration value | `crates/llm-credentials/tests/injected.rs::secret_material_is_not_a_config_value_and_binary_custody_is_supported`, `crates/llm-providers/tests/bindings.rs::binding_documents_have_strict_versions_and_no_literal_secret_fields`, `crates/llm-models/tests/routes.rs::no_secret_material_leaves_the_document`, `crates/llm-cli/tests/command.rs::a_literal_secret_in_the_file_is_refused_without_echoing_it` |
| Opaque provider state crosses no binding coordinate, even under fallback | `crates/llm-core/tests/embedding.rs::opaque_state_cannot_cross_any_binding_coordinate`, `crates/llm-routing/tests/catalog.rs::opaque_state_never_crosses_bindings_even_with_explicit_fallback` |
| A routes document alone never starts a program: an `aws-process` route runs its command only with feature `process` and the caller's `allow_credential_process`, and `llm` refuses such a routes file that others could have written | `crates/llm-models/tests/process_routes.rs::without_the_callers_permission_the_route_is_refused_and_nothing_runs`, `crates/llm-models/tests/routes_toml_bedrock.rs::without_feature_process_an_aws_process_route_is_refused_and_nothing_runs`, `crates/llm-cli/tests/credential_process.rs::a_routes_file_its_group_or_others_can_write_is_refused_and_runs_nothing`, `crates/llm-cli/tests/credential_process.rs::a_routes_file_in_a_directory_others_can_write_is_refused_and_runs_nothing` |
| An `unauthorized` failure never falls back | `crates/llm-routing/tests/codex_login_fallback.rs::a_misconfigured_codex_login_is_unauthorized_and_never_falls_back`, `crates/llm-routing/tests/credential_fallback.rs::a_malformed_pointer_document_is_unauthorized_and_never_falls_back` |
| A provider failure permits retry only before output, and only for the classes the far side reports as its own temporary state (Responses `server_is_overloaded`, `server_error`, `rate_limit_exceeded`; Messages `overloaded_error`, `api_error`); silent output also ends eligibility, for a stream error and for a connection cut on every wire; an exhausted quota or subscription usage limit stays final, reported inside a stream or as an HTTP 429 before it; an error status carrying `x-should-retry: false` is final whatever its status, and the header never makes a status retriable; a Messages turn that stops without content for a reason this projection does not model fails with its final usage and shows the caller nothing, a held `usage-extension` warning included: `unavailable` and, on a streamed answer, retried for a reason with no row of its own (`pause_turn`, `stop_sequence`, a name in no list), `too-large` for `model_context_window_exceeded` and `refused` for `refusal`, both final; a whole Bedrock `InvokeModel` answer is final on every row; and after a content block the turn is the incomplete turn it was | `crates/llm-messages/tests/retry_class.rs::a_stop_without_content_for_a_reason_in_no_row_is_retriable_before_output`, `crates/llm-messages/tests/retry_class.rs::a_stop_at_the_context_window_or_as_a_refusal_without_content_is_final`, `crates/llm-messages/tests/retry_class.rs::the_same_stops_after_silent_or_visible_output_are_an_incomplete_turn`, `crates/llm-messages/tests/retry_class.rs::a_stop_without_content_on_a_route_that_reports_a_cost_shows_nothing_and_keeps_its_class`, `crates/llm-messages/tests/streaming.rs::a_cost_reported_before_the_terminal_delta_is_not_shown_for_a_stop_without_content`, `crates/llm-messages/tests/usage_reader.rs::adversary_w30_a_stream_that_stops_without_content_fails_with_the_terminal_counters_the_reader_reports`, `crates/llm-messages/tests/streaming.rs::every_unmodelled_reason_without_content_fails_as_its_row_at_every_split`, `crates/llm-models/tests/relayed_failure.rs::a_messages_stop_without_content_for_a_reason_in_no_row_ends_after_the_policys_attempts`, `crates/llm-models/tests/relayed_failure.rs::a_messages_stop_at_the_context_window_or_as_a_refusal_without_content_is_asked_once`, `crates/llm-bedrock/tests/client.rs::an_invoke_stream_that_stops_without_content_for_an_unmodelled_reason_fails_as_its_row`, `crates/llm-bedrock/tests/client.rs::a_whole_invoke_answer_that_stops_without_content_for_an_unmodelled_reason_fails_as_its_row`, `crates/llm-models/tests/relayed_failure.rs::a_messages_stop_without_content_on_a_route_that_reports_a_cost_is_retried_the_same_way`, `crates/llm-responses/tests/retry_class.rs::overload_before_output_is_retryable_availability_without_provider_prose`, `crates/llm-responses/tests/retry_class.rs::overload_after_visible_or_silent_output_is_final`, `crates/llm-responses/tests/retry_class.rs::a_transient_failure_before_output_is_retriable_without_provider_prose`, `crates/llm-responses/tests/retry_class.rs::a_transient_failure_after_visible_or_silent_output_is_final`, `crates/llm-responses/tests/retry_class.rs::an_exhausted_quota_or_spend_limit_stays_final_before_output`, `crates/llm-messages/tests/retry_class.rs::overloaded_and_api_errors_before_output_are_retriable_without_provider_prose`, `crates/llm-messages/tests/retry_class.rs::overloaded_and_api_errors_after_visible_or_silent_output_are_final`, `crates/llm-messages/tests/retry_class.rs::every_other_stream_error_stays_final_before_output`, `crates/llm-responses/tests/retry_class.rs::an_exhausted_quota_answered_as_a_429_before_the_stream_is_final`, `crates/llm-responses/tests/retry_class.rs::a_quota_or_usage_limit_named_by_its_error_type_is_final_as_a_429`, `crates/llm-responses/tests/retry_class.rs::any_other_429_stays_a_retriable_rate_limit`, `crates/llm-models/tests/adversary_w10_usage_limit.rs::a_subscription_usage_limit_429_is_asked_once_and_ends_the_run`, `crates/llm-messages/tests/retry_class.rs::a_spend_cap_429_is_final_with_or_without_retry_after`, `crates/llm-messages/tests/retry_class.rs::any_other_429_stays_a_retriable_rate_limit`, `crates/llm-http/tests/retry_classes.rs::a_stalled_429_body_holds_the_caller_only_within_its_bounds_and_keeps_its_class`, `crates/llm-http/tests/adversary_w10_refusal_bounds.rs::a_429_body_that_trickles_is_cut_at_the_refusal_wait`, `crates/llm-http/tests/retry_classes.rs::a_do_not_retry_signal_makes_every_retriable_status_final_and_keeps_the_rest`, `crates/llm-http/tests/retry_classes.rs::the_signal_only_narrows_and_is_one_exact_false`, `crates/llm-models/tests/relayed_failure.rs::a_gateway_502_that_says_do_not_retry_is_attempted_once_on_every_wire`, `crates/llm-models/tests/relayed_failure.rs::the_same_502_without_the_signal_is_retried_on_its_target`, `crates/llm-responses/tests/retry_class.rs::a_cut_after_an_output_item_is_final`, `crates/llm-messages/tests/retry_class.rs::a_cut_before_any_block_stays_retriable`, `crates/llm-messages/tests/retry_class.rs::a_cut_after_silent_or_visible_output_is_final`, `crates/llm-chat/tests/retry_class.rs::a_cut_before_any_output_stays_retriable`, `crates/llm-chat/tests/retry_class.rs::a_cut_after_silent_or_visible_output_is_final`, `crates/llm-chat/tests/every_output_delta.rs::a_cut_after_a_refusal_or_reasoning_delta_is_final`, `crates/llm-chat/tests/every_output_delta.rs::a_cut_after_chunks_that_carry_no_text_stays_retriable` |
| A refusal names its cause (`Error::refusal`) only from an exact structured code, never from prose, copies no body text, and naming it changes no code, dispatch or retry class | `crates/llm-responses/tests/retry_class.rs::a_context_length_refusal_answered_as_a_400_is_told_apart_in_fixed_text`, `crates/llm-responses/tests/retry_class.rs::a_context_length_failure_inside_the_stream_names_its_cause`, `crates/llm-chat/tests/retry_class.rs::a_context_length_refusal_answered_as_a_400_is_told_apart_in_fixed_text`, `crates/llm-http/tests/retry_classes.rs::every_other_400_is_exactly_what_the_status_table_says`, `crates/llm-responses/tests/retry_class.rs::any_other_400_is_refused_as_before_without_a_cause_or_body_text`, `crates/llm-messages/tests/retry_class.rs::a_prompt_too_long_400_stays_a_plain_refusal_because_only_prose_names_it`, `crates/llm-routing/tests/fallback.rs::a_refusal_naming_its_cause_halts_as_every_refusal_does_and_keeps_the_cause` |
| A streamed delta is final: a successful turn's assistant text is the text deltas shown for it; a replaced server text is reported `shown-text-kept`, shown text that cannot be tied to one message overwrites none and is reported `shown-text-unpaired`, only for a returned turn | `crates/llm-responses/tests/live_stream.rs::the_turn_carries_the_text_the_caller_was_shown_and_says_when_the_server_wrote_other_text`, `crates/llm-responses/tests/live_stream.rs::shown_text_no_single_message_can_be_leaves_the_server_messages_as_sent_and_says_so`, `crates/llm-responses/tests/adversary_w09_shown_text.rs::a_refused_turn_does_not_tell_the_caller_its_shown_text_was_kept`, `crates/llm-messages/tests/streaming.rs::text_a_block_opens_with_is_shown_so_the_turn_carries_only_what_was_shown`, `crates/llm-chat/tests/incoming.rs::the_turn_carries_exactly_the_text_its_deltas_showed` |
| Fallback tries only the declared compatible order | `crates/llm-routing/tests/fallback.rs::eligible_failure_before_output_attempts_only_the_declared_compatible_order` |
| A turn never reaches an embed route and an embed request never a turn route: both are refused as `unsupported` before I/O, and a catalog without a route kind keeps its digest | `crates/llm-routing/tests/catalog.rs::a_turn_to_an_embed_route_is_refused_before_io`, `crates/llm-routing/tests/catalog.rs::an_embed_request_to_a_turn_route_is_refused_before_io`, `crates/llm-routing/tests/catalog.rs::a_catalog_without_route_kind_keeps_its_digest`, `crates/llm-embed/tests/embed.rs::an_embed_request_to_a_turn_route_is_refused_before_io`; scenarios `contracts/embeddings/scenarios/a-turn-to-an-embed-route-is-refused-before-io.yaml`, `contracts/embeddings/scenarios/an-embed-request-to-a-turn-route-is-refused-before-io.yaml` |
| A target declaring structured output or image input `false` is never selected or fallen back to for a turn needing it; an undeclared one is not rejected, and a document that declares neither keeps its bytes | `crates/llm-routing/tests/catalog.rs::a_target_declaring_no_structured_output_is_rejected_by_name_and_the_next_one_is_selected`, `crates/llm-routing/tests/catalog.rs::a_target_declaring_no_image_input_is_rejected_by_name_and_the_next_one_is_selected`, `crates/llm-routing/tests/catalog.rs::a_target_that_declares_a_feature_or_leaves_it_undeclared_is_not_rejected_for_it`, `crates/llm-routing/tests/fallback.rs::fallback_never_moves_a_turn_to_a_target_declaring_no_feature_the_turn_needs`, `crates/llm-models/tests/feature_declarations.rs::every_scenario_document_from_before_the_declarations_reads_them_undeclared_and_keeps_its_bytes` |
| A model call to a tool the turn did not publish refuses the turn unless the caller set `controls.return_unpublished_calls`; with it set, a repeated call id, a call other than a named tool choice and oversized arguments still refuse | `crates/llm-core/tests/unpublished_calls.rs::without_the_control_an_unpublished_call_still_refuses_the_turn`, `crates/llm-core/tests/unpublished_calls.rs::with_the_control_the_unpublished_calls_reach_the_caller_as_calls`, `crates/llm-core/tests/unpublished_calls.rs::with_the_control_a_duplicate_call_id_still_refuses`, `crates/llm-core/tests/unpublished_calls.rs::with_the_control_a_call_other_than_the_named_tool_still_refuses`; scenarios `contracts/inference/scenarios/model-output-calling-an-unpublished-tool-*.yaml` |
| A turn whose `controls.parallel_tool_calls` is `false` returns at most one tool call; a turn returning more is refused as `Protocol` under every tool choice | `crates/llm-core/tests/controls.rs::a_turn_allowed_one_tool_call_that_returns_two_is_refused` |
| Credentials never reach a redirect target or an ambient proxy | `crates/llm-http/tests/transport.rs::redirect_never_sends_credentials_to_a_second_endpoint`, `crates/llm-http/tests/ambient_proxy.rs::ambient_proxy_environment_does_not_receive_caller_credentials`, `crates/llm-bedrock/tests/web_identity.rs::a_redirect_is_unauthorized_and_its_target_never_receives_the_token`, `crates/llm-bedrock/tests/web_identity.rs::the_token_never_reaches_an_ambient_proxy` |
| A Bedrock request is signed exactly as the AWS Signature Version 4 test suite states | `crates/llm-bedrock/tests/sigv4_vectors.rs::every_case_matches_its_canonical_request_string_to_sign_and_signature`, `crates/llm-bedrock/tests/sigv4_vectors.rs::sign_writes_the_suites_headers_for_every_request_a_url_carries_as_written`; scenarios `contracts/bedrock/scenarios/sigv4-*.yaml` |
| The event-stream decoder refuses each invalid frame by the check it breaks, decodes a recorded body alike at every chunking, and reports a body cut inside a frame as a retriable cut | `crates/llm-bedrock/tests/eventstream.rs::each_invalid_smithy_frame_is_refused_by_the_check_it_breaks`, `crates/llm-bedrock/tests/eventstream.rs::every_recorded_body_decodes_alike_at_every_split_into_two_chunks`, `crates/llm-bedrock/tests/eventstream.rs::every_recorded_body_decodes_alike_fed_one_byte_at_a_time`, `crates/llm-bedrock/tests/eventstream.rs::a_stream_cut_inside_a_frame_fails_finish_as_a_retriable_cut`; scenarios `contracts/bedrock/scenarios/eventstream-*.yaml` |
| A Bedrock turn may be retried only before the model answered: a cut or a throttling exception after content is final | Scenarios `contracts/bedrock/scenarios/turn-a-cut-after-content-is-final.yaml`, `contracts/bedrock/scenarios/turn-a-throttling-exception-after-content-is-final.yaml` and their before-content counterparts. `crates/llm-bedrock/tests/client.rs` holds it on both wires: `throttling_inside_the_stream_is_retriable_only_before_the_turn_answered` (Converse, after text), `a_cut_is_retriable_before_the_turn_answered_and_final_after` (Converse, after text), `throttling_after_silent_output_is_final_and_before_any_content_retriable_on_both_wires` and `a_cut_after_silent_output_is_final_and_before_any_content_retriable_on_both_wires` (Converse and InvokeModel, after a silent `contentBlockStart` / `content_block_start`); `crates/llm-bedrock/tests/adversary_w2026_11_09_u2.rs::throttling_after_output_the_caller_was_not_shown_is_final_on_both_wires`, `crates/llm-bedrock/tests/adversary_w2026_11_09_u2.rs::a_cut_after_output_the_caller_was_not_shown_is_final_on_both_wires`, `crates/llm-bedrock/tests/adversary_w2026_11_09_u2.rs::every_transient_exception_is_retriable_before_content_and_final_after_on_both_wires` |
| No crate depends on an AWS SDK crate or on `aws-lc`; `llm-bedrock` signs with `ring` | `crates/llm-core/tests/dependency_boundary.rs::no_package_in_the_lockfile_is_an_aws_sdk_crate_or_aws_lc` |
| No `unsafe` code | `unsafe_code = "forbid"` in the root `Cargo.toml`; Clippy `all` and `pedantic` deny |
| No field a client sends is dropped: each ingress decoder carries, translates, reports or refuses it, and refuses an unknown key at every level by its path | `crates/llm-messages/tests/client_request_fields.rs::every_refusal_names_its_field_by_path`, `crates/llm-responses/tests/client_fields.rs::a_key_outside_the_subset_is_refused_by_its_path_at_every_level`, `crates/llm-chat/tests/ingress.rs::an_unknown_key_on_a_content_part_is_refused_by_its_path` and the other `an_unknown_key_on_*` cases there, `crates/llm-chat/tests/every_output_delta.rs::a_refusal_only_message_reads_back_and_a_refusal_that_is_not_a_string_is_refused_by_its_path`; advisory omissions `checks/conformance/tests/client_turns.rs` |
| The conformance suite answers at least 1080 scenarios and skips none | `contracts/baseline.json`, enforced by `llm-conformance check` |

The default gate makes no paid provider call and provisions nothing. Fixture evidence is not
provider or hosting qualification; `docs/implementation-status.md` keeps that distinction.

## Gate

CI (`.github/workflows/gate.yml`) runs Rust 1.98.0 and the repository has no
`rust-toolchain.toml`, so gate locally on the same toolchain; a newer Clippy adds lints CI lacks:

```bash
RUSTUP_TOOLCHAIN=1.98.0 task check
```

`task check` runs, in order:

| Step | Alone |
| --- | --- |
| `llm-credentials` and `llm-cost` checked with no features and `llm-bedrock` with its defaults; `cargo fmt --all --check`; Clippy on all targets and features with `-D warnings`, on the default toolchain and on 1.98.0, and on `llm-models` and `llm-bedrock` with no features, because workspace feature unification turns `process` on everywhere else | `task rust` |
| The test phase: the workspace tests, then `llm-credentials`, `llm-cost` and `llm-calls` with all features and `llm-models` with none, each one `cargo nextest run` under `.config/nextest.toml` (a test over 60 s is killed and fails; one run ends at 120 s). The binaries are built first, and every `harness = false` test target must answer nextest's `--list --format terse` (with and without `--ignored`) in libtest's form, or the phase fails naming it (`cargo xtask list-check`); the runs together may then take 120 s by their own JUnit reports, and the ten slowest tests are printed | `task test` (`cargo xtask test`, `checks/xtask`) |
| Generated pages and quoted guide programs are current | `task docs` (`cargo run --locked -p llm-docs -- generate --check`) |
| The ESS specification validates | `ess specify validate --path spec` |
| Suite and schema projections match `contracts/`, then the suite runs three times against `contracts/baseline.json` | `task conformance` (`cargo run --locked -p llm-conformance -- check`, from the repository root) |
| The planning store validates | `aep plan artifact validate` |

CI installs `ess` 0.87.0 and `aep` 0.68.0 (pinned by revision in `gate.yml`) and cargo-nextest
0.9.143 (pinned by version and SHA-256), and runs the test phase as its own step, "Rust tests"
(`cargo xtask test`, as `task test` does). Run the gate on this host as `slot task check` or
`slot task test`: inside a slot job the runs take no second slot. A second CI job
tests and lints `llm-credentials` and `llm-cost` with all features on macOS and Windows;
a native keychain change cannot be checked on Linux alone. `.github/workflows/shared-gates.yml`
runs the organisation's common Gates checks on pull requests, `main` and tags.

`.cargo/config.toml` builds the dev and test profile without debug information, so a test binary
carries no `.debug_*` section here or in CI (`checks/xtask/tests/gate/debug_info.rs`), and at
`opt-level = 1`, with debug assertions and overflow checks still on. Unoptimised, the tests that
encode a 16 MiB body or parse 2 MiB of YAML spent most of their time in serde_json's and
unsafe-libyaml's byte loops and their precondition checks (7 to 55 s each); at level 1 they take
0.1 to 2.7 s. A per-crate level does not reach far enough: under shared generics a test links the
serde_json instance an unoptimised upstream crate exported.

A pull request whose changed files are all planning or prose passes every Gate job under its
own name without building or testing: job `changes` (step "Planning or prose only") decides, and
every step of the other jobs runs only when the decision is not `skip`. Planning or prose is
everything under `.engineering/` and every `*.md` outside `website/` except
`docs/implementation-status.md`, which `llm-docs generate --check` reads. That `case` in
`gate.yml` is the one path list; `checks/xtask/tests/gate/paths.rs` runs it on both sides. A
file a gate step starts to read leaves the list in the same change. A push to `main` always runs
the whole gate. Documentation validation (`pages.yml`) still runs on every pull request.

An ESS pin move changes `ESS_VERSION` and `ESS_SHA256` in `gate.yml` together, the digest read from
the `ess-<version>-x86_64-unknown-linux-gnu.tar.gz` line of that release's `SHA256SUMS`, and moves
the `ess-conformance` and `ess-primitives` tags in `checks/conformance/Cargo.toml` with them. A
version moved without its digest fails the "Install ess" step before any test runs.

Build into the worktree's own `target/` and never set `CARGO_TARGET_DIR`. Let cargo create
`target/`: one made by hand before the first build has no `CACHEDIR.TAG`, and
`worktree discard-cache` then keeps the whole build as unrecognised. End every tree with
`worktree finish --discard-cache --archive <tree>`. The conformance runner writes its projections
and reports to `target/conformance/` under the repository root.

## Generated files

Never edit these by hand; change the source and regenerate.

| File | Source | Regenerate |
| --- | --- | --- |
| `website/docs/reference/crates.md` | `cargo metadata --no-deps` (each package's `description` is public text) | `task docs-generate` |
| `website/data/status.json`, `website/docs/status.mdx` | the `## Status at` section of `docs/implementation-status.md` | `task docs-generate` |
| `contracts/suite.json` | `spec/` and the scenarios `contracts/ess-inputs.yaml` lists | `ess verify conform synthesize --path spec --suite-format 5 --target ir --scenarios contracts --out contracts/suite.json` |
| `contracts/schema/` | `spec/` | `ess generate --path spec --kind schema --out contracts/schema` (ess writes only where `contracts/schema/.ess-output/` records it owns the bytes, and that record is ignored; ess 0.87.0 also refuses a copied `.ess-output/` as no ownership transfer, so generate into an empty directory under `target/` and copy its `schema/` files over `contracts/schema/schema/`) |

Authored scenarios live in `contracts/<domain>/scenarios/`; the conformance check refuses one that
`contracts/ess-inputs.yaml` does not list. `llm-docs generate --check` also fails when a guide
block titled `crates/llm-docs/examples/<name>.rs` differs from that file, and on an unbracketed
admonition title. Guide programs live in `crates/llm-docs/examples/` and are run before their
output is pasted into a page.

## Documentation

The site is `website/` (Docusaurus, `@beyond10x/docs-system` pinned in `website/package.json`),
served at `https://beyond10x.github.io/llm/`. Build it with
`npm --prefix website ci && npm --prefix website run build`. Two workflows publish it:
`pages.yml` (Documentation validation: build, `llm-docs` tests, `llm-docs provenance`, which writes
`.well-known/b10x-site.json` and the route inventory `.well-known/b10x-routes.json`) and
`b10x-docs-site.yml` (Documentation site: Website's `project-site.yml`, bot pushes to `main` only).
The unified-site files (`b10x.docs.yaml`, `b10x-docs-bundle.yml`, `b10x-docs-check.yml`,
`b10x-docs-pages.yml`) are retired and do not come back.

Keep page paths and heading IDs stable: the organisation Website redirects the former `/docs/llm/`
routes to them. A Markdown page links an `.mdx` page by route (`/docs/status`), never by file.
Writing rules and the end-to-end procedure are the workspace `docs` skill.

`ux/` is the site's UX loop (ui-iteration-loop): a Playwright and axe-core harness in TypeScript
over the built site served locally. `task ux-check` runs the findings rule, L1 core tests, the L2
layout contract (no horizontal scroll, always-visible navbar, search, theme toggle and sidebar, axe
0 serious or critical, keyboard reach) at 1440x900 and 390x844 in both themes, the L3 screen-only
user over `ux/scenarios/task-*`, then the ratchet against `ux/baseline.json` and main's.
`.github/workflows/docs-ux.yml` runs it on pull requests that touch `website/`, `docs/` or `ux/`.
A gate failure is fixed, or waived only by an open finding in `ux/findings/`; an improved value is
taken into the baseline in the same change (`task ux-ratchet -- --update`). `task ux-round` writes
a round to `ux/rounds/R-nn/`. Build the site first; a new page needs `task ux-scenarios`.

A release or a wave that changes a crate, a command, a default or a capability updates, in the same
change: `CHANGELOG.md` (under Unreleased), `docs/implementation-status.md` and then
`task docs-generate`, the hand-written pages it affects, README.md and this file.
`CHANGELOG.md` follows Keep a Changelog 1.1.0 with its six headings only; one bullet per change on
one line of at most about 120 characters; a breaking change starts with `**Breaking:**`.

## Planning store and waves

The plan is the AEP store in `.engineering/planning/` (`aep.project/5`); change it only through
the `aep` CLI and check it with `aep plan artifact validate`. Read the owning artifact and
`docs/design.md` before implementing. Validate the ESS domain before decomposing a new entity.
A story is `implemented` on its acceptance evidence, not because the workspace builds.

Waves run as the AEP implementing skill describes: `plan: open wave <id> (story:<id>)`, one
`impl/<story>` branch per story merged into `wave/<id>`, the wave merged into `main`, then
`plan: close wave <id>`. Commit titles are semantic (`feat:`, `fix:`, `docs:`, `spec:`, `test:`,
`plan:`, `release:`) with a body.

An adversary's mutant or base copy builds in a managed worktree of this repository
(`worktree create --repo <checkout> --base <commit>`), never in a copy under a cache or scratch
directory: the build pool keeps a job whose directory or argument is under a cache directory on the
workstation and does not hand it to a build node. Every adversary brief says so.

## Release

Versions are workspace-wide (`[workspace.package] version`) and tags are bare (`0.2.0`); the
workspace is `publish = false`, so a release is a source release with no assets. The 0.2.0 release
is the model:

1. Branch `release/<version>` from `main`; one commit `release: LLM <version>` that sets the
   workspace version (and `Cargo.lock`), dates the CHANGELOG section, moves README.md's status line,
   the `## Status at` heading of `docs/implementation-status.md` and the `source` line of
   `website/docs/index.md`, then runs `task docs-generate`.
2. Bot pull request; once its checks are green and `main` is green, the operator session merges
   the pull request through the bot (0.29.0: a merge commit on `main` whose tree is the release
   commit's). The session that cuts the release sends the operator session the pull request's
   number and full head sha and does not push to `main` itself. Up to 0.28.0 `main` was
   fast-forwarded to the release commit by a bot push. A wave's pull request lands on `main` by
   the same merge.
3. Tag `<version>` on the commit of `main` that carries the release (since 0.29.0 the merge
   commit, whose tree equals the release commit's), only after every run of `Gate`,
   `Shared source gates` and `Documentation validation` on that commit ended `success`:
   `task release-tag -- <version> <sha>` (`cargo xtask release-tag`, `checks/xtask`; `<sha>` is
   the full commit id) makes and pushes the tag as the bot and refuses it while one of those runs
   is absent, pending or red. No other command makes a release tag. `b10x-gates` reads its policy
   file from `B10X_GATES_POLICY`. Then publish the GitHub Release `LLM <version>` as the bot.
4. Verify the tag's commit, `Gate`, `Shared source gates` and `Documentation validation` on it, and
   the Release; only then report it released.
5. Delete the remote `release/<version>` with
   `b10x-gates bot --repository beyond10x/llm -- delete-branch release/<version>` once `main` holds
   its head.

Consumers pin a release tag and move only on their own change: Harness (on 0.8.0 since its
cutover, Atlas ADR 0097), Loom, inference and llm-gateway.

## Publishing

Every commit, push, pull request, tag and release is `b10x-bot[bot]`'s through `b10x-gates`, as
the workspace `AGENTS.md` says; `gh` is read-only here. Use a managed worktree for every change.
No private policy, credentials, customer names or `/home/<name>/` paths belong in this repository.

## Never

- Add a dependency from a core crate on a consumer, on the `secrets` library, or from
  `llm-core` on transport.
- Report an absent usage counter as zero (the one exception, a Bedrock Converse cache counter, is
  in the invariants table), or drop opaque state or an unsupported field to make a translation
  succeed: carry it or refuse it. The one exception: a Responses `Opaque` item holding a
  `response.metadata` stream event, which 0.16.0 and earlier saved and the endpoint refuses, is left
  out of the request (`crates/llm-responses/tests/saved_codex_metadata.rs`).
- Put a paid provider call or external provisioning into the default gate.
- Hand-edit a generated file, or lower `contracts/baseline.json` without a recorded reason in the
  CHANGELOG.
- Bring a serving crate or a serving ESS domain back into this workspace; it belongs in
  llm-gateway.
- Write code that runs in anything but Rust.

<!-- b10x-release-operations:start -->
## Release completion

An ordinary release completes after this repository's exact tag, required source checks,
published release and required artifacts are verified. A pushed tag with unfinished checks or
uploads is queued; report it as released only after those requirements succeed.

Atlas reconciliation and public documentation publication run asynchronously. Do not wait for
Atlas or Website, update Website source locks or bootstrap snapshots, promote consumer pins,
release shared docs tooling, or redeploy documentation façades as part of an ordinary source
release. Report documentation as pending unless its publication was actually verified. A background
documentation failure does not invalidate a successful source release.

Keep this repository's provenance, correctness, security, compatibility and artifact verification
requirements. Shared rendering, routing or delivery-control changes still require their relevant
integration gates. A release request does not authorize deployment or downstream releases.
Repositories without a release unit retain their existing publication policy. This completion
boundary supersedes older instructions that attach synchronous documentation ceremony to each
source release.
<!-- b10x-release-operations:end -->
