# Working on LLM

What llm is and how a person uses it is in [README.md](README.md) and on the site,
<https://beyond10x.github.io/llm/>. This file is what an agent changing the repository needs.

## Serves

Organisation objectives O1 (explicit model reach, credentials and spending decisions), O3 (one
neutral inference interface for several agent harnesses) and O6 (attributable usage, costs and
failures), as `.engineering/planning/vision/portable-model-inference.md` records.

## Owns, and does not own

llm owns the client side of model inference: the neutral turn (`llm-core`), the transport, the
three protocol crates, credentials, provider bindings, routing, pricing, `call_tool` and the
blocking adapter. The accepted design is `docs/design.md`; per-protocol detail is in
`docs/responses.md`, `docs/messages.md` and `docs/chat.md`.

It does not own:

- **Serving.** The gateway, hosting and provisioning crates (`b10x-llm-gateway`,
  `b10x-llm-provision`, `b10x-llm-runpod`, `b10x-llm-modal`) and the `llm.gateway`, `llm.hosting`
  and `llm.runpod` ESS domains live in [beyond10x/llm-gateway](https://github.com/beyond10x/llm-gateway)
  since 0.2.0. Change them there; their archived stories stay in this store as history.
- **Agent loops, tool execution and approval.** A tool here is a name, a description and a JSON
  Schema. The caller runs it.
- **Credential custody and login flows.** Secrets come from a resolver the caller injects.
- **Its consumers.** No crate here depends on a consumer repository.

Protocol, provider/account, credential source, model and hosting provider stay distinct concepts.
A provider adapter never changes an endpoint or a billing account on its own.

## Invariants and the tests that hold them

Each claim fails the named test when broken. Weakening a test weakens the claim, so it needs a
story that says so.

| Invariant | Held by |
| --- | --- |
| No workspace crate depends on a consumer; `b10x-llm-core` has no transport dependency | `crates/llm-core/tests/dependency_boundary.rs::inference_workspace_does_not_depend_on_its_consumers_and_core_has_no_transport` |
| The `secrets` library is reached only through `b10x-llm-credentials` feature `secrets`: optional, off by default, tag `v0.5.0` at the commit the lockfile records, forwarded by no core crate | `crates/llm-credentials/tests/secrets_boundary.rs::the_secrets_library_is_reached_only_through_the_credentials_feature` (moving the tag means moving `TAG` and `REVISION` there) |
| Unknown usage stays unknown, never zero | `crates/llm-core/tests/embedding.rs::unknown_usage_stays_distinct_from_zero_and_invalid_subsets_refuse`, and per protocol `crates/llm-responses/tests/adversary.rs::an_unreported_input_count_stays_unknown_and_is_never_zero`, `crates/llm-messages/tests/streaming.rs::a_counter_the_route_never_reported_stays_unknown`, `crates/llm-chat/tests/incoming.rs::a_counter_a_present_usage_report_omits_stays_absent_and_never_becomes_zero` |
| Secret material is never a configuration value | `crates/llm-credentials/tests/injected.rs::secret_material_is_not_a_config_value_and_binary_custody_is_supported`, `crates/llm-providers/tests/bindings.rs::binding_documents_have_strict_versions_and_no_literal_secret_fields` |
| Opaque provider state crosses no binding coordinate, even under fallback | `crates/llm-core/tests/embedding.rs::opaque_state_cannot_cross_any_binding_coordinate`, `crates/llm-routing/tests/catalog.rs::opaque_state_never_crosses_bindings_even_with_explicit_fallback` |
| An `unauthorized` failure never falls back | `crates/llm-routing/tests/codex_login_fallback.rs::a_misconfigured_codex_login_is_unauthorized_and_never_falls_back`, `crates/llm-routing/tests/credential_fallback.rs::a_malformed_pointer_document_is_unauthorized_and_never_falls_back` |
| Fallback tries only the declared compatible order | `crates/llm-routing/tests/fallback.rs::eligible_failure_before_output_attempts_only_the_declared_compatible_order` |
| Credentials never reach a redirect target or an ambient proxy | `crates/llm-http/tests/transport.rs::redirect_never_sends_credentials_to_a_second_endpoint`, `crates/llm-http/tests/ambient_proxy.rs::ambient_proxy_environment_does_not_receive_caller_credentials` |
| No `unsafe` code | `unsafe_code = "forbid"` in the root `Cargo.toml`; Clippy `all` and `pedantic` deny |
| The conformance suite answers at least 674 scenarios and skips none | `contracts/baseline.json`, enforced by `b10x-llm-conformance check` |

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
| Workspace tests; `b10x-llm-credentials` and `b10x-llm-cost` with all features and checked with none; `cargo fmt --all --check`; Clippy on all targets and features with `-D warnings`, on the default toolchain and on 1.98.0 | `task rust` |
| Generated pages and quoted guide programs are current | `task docs` (`cargo run --locked -p llm-docs -- generate --check`) |
| The ESS specification validates | `ess specify validate --path spec` |
| Suite and schema projections match `contracts/`, then the suite runs three times against `contracts/baseline.json` | `task conformance` (`cargo run --locked -p b10x-llm-conformance -- check`, from the repository root) |
| The planning store validates | `aep plan artifact validate` |

CI installs `ess` 0.55.0 and `aep` 0.68.0 (pinned by revision in `gate.yml`). A second CI job
tests and lints `b10x-llm-credentials` and `b10x-llm-cost` with all features on macOS and Windows;
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
| `contracts/schema/` | `spec/` | `ess generate --path spec --kind schema --out contracts/schema` |

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
2. Bot pull request, green checks, merged through the App.
3. Tag `<version>` on the merge commit and publish the GitHub Release `LLM <version>`, both as the
   bot.
4. Verify the tag's commit, `Gate`, `Shared source gates` and `Documentation validation` on it, and
   the Release; only then report it released.

Consumers (Loom today) pin a release tag; a consumer moves only on its own change.

## Publishing

Every commit, push, pull request, tag and release is `b10x-bot[bot]`'s through `b10x-gates`, as
the workspace `AGENTS.md` says; `gh` is read-only here. Use a managed worktree for every change.
No private policy, credentials, customer names or `/home/<name>/` paths belong in this repository.

## Never

- Add a dependency from a core crate on a consumer, on the `secrets` library, or from
  `b10x-llm-core` on transport.
- Report an absent usage counter as zero, or drop opaque state or an unsupported field to make a
  translation succeed: carry it or refuse it.
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
