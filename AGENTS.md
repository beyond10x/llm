# Working on LLM

What llm is and how a person uses it is in [README.md](README.md) and on the site,
<https://beyond10x.github.io/llm/>. This file is what an agent changing the repository needs.

## Serves

Organisation objectives O1 (explicit model reach, credentials and spending decisions), O3 (one
neutral inference interface for several agent harnesses) and O6 (attributable usage, costs and
failures), as `.engineering/planning/vision/portable-model-inference.md` records.

## Owns, and does not own

llm owns the client side of model inference: the neutral turn (`llm-core`), the transport, the
three protocol crates (each with its ingress decoder and encoder), the Amazon Bedrock Runtime
client (`llm-bedrock`: Converse, Anthropic `InvokeModel`, Signature Version 4, event-stream
frames, static and container AWS credentials), credentials, provider bindings,
routing, port construction from a catalog or an `llm.routes/1` routes document in YAML or TOML,
Bedrock endpoints included (`llm-models`), pricing, `call_tool`, the blocking adapter and the
`llm` command (`llm-cli`: `model list` and `infer` over one `llm.toml`; it injects only the
`environment` and `file` sources, no keychain). The accepted design is
`docs/design.md` (its "Client request fields" section is the rule for every field an ingress
decoder reads); per-protocol detail is in `docs/responses.md`, `docs/messages.md`,
`docs/chat.md` and `docs/bedrock.md`.

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
| Unknown usage stays unknown, never zero | `crates/llm-core/tests/embedding.rs::unknown_usage_stays_distinct_from_zero_and_invalid_subsets_refuse`, and per protocol `crates/llm-responses/tests/adversary.rs::an_unreported_input_count_stays_unknown_and_is_never_zero`, `crates/llm-messages/tests/streaming.rs::a_counter_the_route_never_reported_stays_unknown`, `crates/llm-chat/tests/incoming.rs::a_counter_a_present_usage_report_omits_stays_absent_and_never_becomes_zero`, `crates/llm-chat/tests/incoming.rs::removing_any_one_timings_counter_leaves_unknown_exactly_what_it_counted`; ingress encoders `crates/llm-responses/tests/ingress.rs::an_unknown_input_count_is_omitted_and_decodes_back_unknown_never_zero`, `crates/llm-messages/tests/ingress.rs::an_unknown_output_count_is_left_out_and_decodes_back_unknown`; Bedrock Converse, whose body never carries a `cachePoint`, so an absent cache counter is a known 0 and only an absent `inputTokens` leaves the input unknown: `crates/llm-bedrock/tests/converse.rs::an_absent_cache_counter_is_a_known_zero_on_converse`, `crates/llm-bedrock/tests/converse.rs::the_converse_body_never_carries_a_cache_point`, `crates/llm-bedrock/tests/converse.rs::cache_counters_add_up_to_the_whole_input`; the command `crates/llm-cli/tests/command.rs::infer_reads_the_prompt_from_stdin_and_an_unreported_counter_prints_unknown` |
| Secret material is never a configuration value | `crates/llm-credentials/tests/injected.rs::secret_material_is_not_a_config_value_and_binary_custody_is_supported`, `crates/llm-providers/tests/bindings.rs::binding_documents_have_strict_versions_and_no_literal_secret_fields`, `crates/llm-models/tests/routes.rs::no_secret_material_leaves_the_document`, `crates/llm-cli/tests/command.rs::a_literal_secret_in_the_file_is_refused_without_echoing_it` |
| Opaque provider state crosses no binding coordinate, even under fallback | `crates/llm-core/tests/embedding.rs::opaque_state_cannot_cross_any_binding_coordinate`, `crates/llm-routing/tests/catalog.rs::opaque_state_never_crosses_bindings_even_with_explicit_fallback` |
| A routes document alone never starts a program: an `aws-process` route runs its command only with feature `process` and the caller's `allow_credential_process`, and `llm` refuses such a routes file that others could have written | `crates/llm-models/tests/process_routes.rs::without_the_callers_permission_the_route_is_refused_and_nothing_runs`, `crates/llm-models/tests/routes_toml_bedrock.rs::without_feature_process_an_aws_process_route_is_refused_and_nothing_runs`, `crates/llm-cli/tests/credential_process.rs::a_routes_file_its_group_or_others_can_write_is_refused_and_runs_nothing`, `crates/llm-cli/tests/credential_process.rs::a_routes_file_in_a_directory_others_can_write_is_refused_and_runs_nothing` |
| An `unauthorized` failure never falls back | `crates/llm-routing/tests/codex_login_fallback.rs::a_misconfigured_codex_login_is_unauthorized_and_never_falls_back`, `crates/llm-routing/tests/credential_fallback.rs::a_malformed_pointer_document_is_unauthorized_and_never_falls_back` |
| A provider failure permits retry only before output, and only for the classes the far side reports as its own temporary state (Responses `server_is_overloaded`, `server_error`, `rate_limit_exceeded`; Messages `overloaded_error`, `api_error`); silent output also ends eligibility, for a stream error and for a connection cut on every wire; an exhausted quota or subscription usage limit stays final, reported inside a stream or as an HTTP 429 before it; an error status carrying `x-should-retry: false` is final whatever its status, and the header never makes a status retriable | `crates/llm-responses/tests/retry_class.rs::overload_before_output_is_retryable_availability_without_provider_prose`, `crates/llm-responses/tests/retry_class.rs::overload_after_visible_or_silent_output_is_final`, `crates/llm-responses/tests/retry_class.rs::a_transient_failure_before_output_is_retriable_without_provider_prose`, `crates/llm-responses/tests/retry_class.rs::a_transient_failure_after_visible_or_silent_output_is_final`, `crates/llm-responses/tests/retry_class.rs::an_exhausted_quota_or_spend_limit_stays_final_before_output`, `crates/llm-messages/tests/retry_class.rs::overloaded_and_api_errors_before_output_are_retriable_without_provider_prose`, `crates/llm-messages/tests/retry_class.rs::overloaded_and_api_errors_after_visible_or_silent_output_are_final`, `crates/llm-messages/tests/retry_class.rs::every_other_stream_error_stays_final_before_output`, `crates/llm-responses/tests/retry_class.rs::an_exhausted_quota_answered_as_a_429_before_the_stream_is_final`, `crates/llm-responses/tests/retry_class.rs::a_quota_or_usage_limit_named_by_its_error_type_is_final_as_a_429`, `crates/llm-responses/tests/retry_class.rs::any_other_429_stays_a_retriable_rate_limit`, `crates/llm-models/tests/adversary_w10_usage_limit.rs::a_subscription_usage_limit_429_is_asked_once_and_ends_the_run`, `crates/llm-messages/tests/retry_class.rs::a_spend_cap_429_is_final_with_or_without_retry_after`, `crates/llm-messages/tests/retry_class.rs::any_other_429_stays_a_retriable_rate_limit`, `crates/llm-http/tests/retry_classes.rs::a_stalled_429_body_holds_the_caller_only_within_its_bounds_and_keeps_its_class`, `crates/llm-http/tests/adversary_w10_refusal_bounds.rs::a_429_body_that_trickles_is_cut_at_the_refusal_wait`, `crates/llm-http/tests/retry_classes.rs::a_do_not_retry_signal_makes_every_retriable_status_final_and_keeps_the_rest`, `crates/llm-http/tests/retry_classes.rs::the_signal_only_narrows_and_is_one_exact_false`, `crates/llm-models/tests/relayed_failure.rs::a_gateway_502_that_says_do_not_retry_is_attempted_once_on_every_wire`, `crates/llm-models/tests/relayed_failure.rs::the_same_502_without_the_signal_is_retried_on_its_target`, `crates/llm-responses/tests/retry_class.rs::a_cut_after_an_output_item_is_final`, `crates/llm-messages/tests/retry_class.rs::a_cut_before_any_block_stays_retriable`, `crates/llm-messages/tests/retry_class.rs::a_cut_after_silent_or_visible_output_is_final`, `crates/llm-chat/tests/retry_class.rs::a_cut_before_any_output_stays_retriable`, `crates/llm-chat/tests/retry_class.rs::a_cut_after_silent_or_visible_output_is_final` |
| A streamed delta is final: a successful turn's assistant text is the text deltas shown for it; a replaced server text is reported `shown-text-kept`, shown text that cannot be tied to one message overwrites none and is reported `shown-text-unpaired`, only for a returned turn | `crates/llm-responses/tests/live_stream.rs::the_turn_carries_the_text_the_caller_was_shown_and_says_when_the_server_wrote_other_text`, `crates/llm-responses/tests/live_stream.rs::shown_text_no_single_message_can_be_leaves_the_server_messages_as_sent_and_says_so`, `crates/llm-responses/tests/adversary_w09_shown_text.rs::a_refused_turn_does_not_tell_the_caller_its_shown_text_was_kept`, `crates/llm-messages/tests/streaming.rs::text_a_block_opens_with_is_shown_so_the_turn_carries_only_what_was_shown`, `crates/llm-chat/tests/incoming.rs::the_turn_carries_exactly_the_text_its_deltas_showed` |
| Fallback tries only the declared compatible order | `crates/llm-routing/tests/fallback.rs::eligible_failure_before_output_attempts_only_the_declared_compatible_order` |
| Credentials never reach a redirect target or an ambient proxy | `crates/llm-http/tests/transport.rs::redirect_never_sends_credentials_to_a_second_endpoint`, `crates/llm-http/tests/ambient_proxy.rs::ambient_proxy_environment_does_not_receive_caller_credentials` |
| A Bedrock request is signed exactly as the AWS Signature Version 4 test suite states | `crates/llm-bedrock/tests/sigv4_vectors.rs::every_case_matches_its_canonical_request_string_to_sign_and_signature`, `crates/llm-bedrock/tests/sigv4_vectors.rs::sign_writes_the_suites_headers_for_every_request_a_url_carries_as_written`; scenarios `contracts/bedrock/scenarios/sigv4-*.yaml` |
| The event-stream decoder refuses each invalid frame by the check it breaks, decodes a recorded body alike at every chunking, and reports a body cut inside a frame as a retriable cut | `crates/llm-bedrock/tests/eventstream.rs::each_invalid_smithy_frame_is_refused_by_the_check_it_breaks`, `crates/llm-bedrock/tests/eventstream.rs::every_recorded_body_decodes_alike_at_every_split_into_two_chunks`, `crates/llm-bedrock/tests/eventstream.rs::every_recorded_body_decodes_alike_fed_one_byte_at_a_time`, `crates/llm-bedrock/tests/eventstream.rs::a_stream_cut_inside_a_frame_fails_finish_as_a_retriable_cut`; scenarios `contracts/bedrock/scenarios/eventstream-*.yaml` |
| A Bedrock turn may be retried only before the model answered: a cut or a throttling exception after content is final | Scenarios `contracts/bedrock/scenarios/turn-a-cut-after-content-is-final.yaml`, `contracts/bedrock/scenarios/turn-a-throttling-exception-after-content-is-final.yaml` and their before-content counterparts. UNMAPPED: no `crates/llm-bedrock` test holds it yet |
| No crate depends on an AWS SDK crate or on `aws-lc`; `llm-bedrock` signs with `ring` | `crates/llm-core/tests/dependency_boundary.rs::no_package_in_the_lockfile_is_an_aws_sdk_crate_or_aws_lc` |
| No `unsafe` code | `unsafe_code = "forbid"` in the root `Cargo.toml`; Clippy `all` and `pedantic` deny |
| No field a client sends is dropped: each ingress decoder carries, translates, reports or refuses it, and refuses an unknown key at every level by its path | `crates/llm-messages/tests/client_request_fields.rs::every_refusal_names_its_field_by_path`, `crates/llm-responses/tests/client_fields.rs::a_key_outside_the_subset_is_refused_by_its_path_at_every_level`, `crates/llm-chat/tests/ingress.rs::an_unknown_key_on_a_content_part_is_refused_by_its_path` and the other `an_unknown_key_on_*` cases there; advisory omissions `checks/conformance/tests/client_turns.rs` |
| The conformance suite answers at least 937 scenarios and skips none | `contracts/baseline.json`, enforced by `llm-conformance check` |

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
| Workspace tests; `llm-credentials` and `llm-cost` with all features and checked with none; `llm-models` tested and `llm-bedrock` checked with no features, both linted so, because workspace feature unification turns `process` on everywhere else; `cargo fmt --all --check`; Clippy on all targets and features with `-D warnings`, on the default toolchain and on 1.98.0 | `task rust` |
| Generated pages and quoted guide programs are current | `task docs` (`cargo run --locked -p llm-docs -- generate --check`) |
| The ESS specification validates | `ess specify validate --path spec` |
| Suite and schema projections match `contracts/`, then the suite runs three times against `contracts/baseline.json` | `task conformance` (`cargo run --locked -p llm-conformance -- check`, from the repository root) |
| The planning store validates | `aep plan artifact validate` |

CI installs `ess` 0.69.2 and `aep` 0.68.0 (pinned by revision in `gate.yml`). A second CI job
tests and lints `llm-credentials` and `llm-cost` with all features on macOS and Windows;
a native keychain change cannot be checked on Linux alone. `.github/workflows/shared-gates.yml`
runs the organisation's common Gates checks on pull requests, `main` and tags.

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
| `contracts/schema/` | `spec/` | `ess generate --path spec --kind schema --out contracts/schema` (ess 0.69 writes only where `contracts/schema/.ess-output/` records it owns the bytes; that record is ignored, so in a fresh tree generate once into an empty directory under `target/` and copy its `.ess-output/` beside `contracts/schema/schema/`) |

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

## Release

Versions are workspace-wide (`[workspace.package] version`) and tags are bare (`0.2.0`); the
workspace is `publish = false`, so a release is a source release with no assets. The 0.2.0 release
is the model:

1. Branch `release/<version>` from `main`; one commit `release: LLM <version>` that sets the
   workspace version (and `Cargo.lock`), dates the CHANGELOG section, moves README.md's status line,
   the `## Status at` heading of `docs/implementation-status.md` and the `source` line of
   `website/docs/index.md`, then runs `task docs-generate`.
2. Bot pull request; once its checks are green, `main` is fast-forwarded to the release commit by
   a bot push, not merged through the pull request. Waves land on `main` the same way.
3. Tag `<version>` on the release commit and publish the GitHub Release `LLM <version>`, both as the
   bot.
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
  succeed: carry it or refuse it.
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
