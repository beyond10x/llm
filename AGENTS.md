# Working on LLM

## Serves

- O1: explicit model reach, credentials and spending decisions.
- O3: one neutral inference interface for multiple agent harnesses.
- O6: attributable usage, costs and failures that can be compared.

## Boundaries

This repository owns the client side of model inference, not agent loops or tool execution, and
not serving: the gateway and hosting crates (`b10x-llm-gateway`, `b10x-llm-provision`,
`b10x-llm-runpod`, `b10x-llm-modal`) and the `llm.gateway`, `llm.hosting` and `llm.runpod` ESS
domains live in [beyond10x/llm-gateway](https://github.com/beyond10x/llm-gateway); change them
there. Core types
perform no I/O, carry no execution permission and depend on no consumer repository. Protocol,
provider/account, credential source, model and hosting provider remain distinct concepts.

Read README.md, docs/design.md and the owning AEP artifact before implementation. Validate the
ESS domain before decomposing new entities. All planning-store changes use the AEP CLI. Keep
unknown usage absent, secrets out of serializable configuration, and incompatible opaque state
explicitly refused. A provider adapter never silently changes endpoints or billing accounts.

Use managed worktrees. Commits and publication use the organization bot and existing Gates/Atlas
controls. No private policy or credentials belong in this repository. Run `task check` before
publication. Runtime code and checkers are Rust; no paid provider call belongs in the default gate.

## Gate

`task check` runs, in order: `task rust` (workspace tests, the credential and cost crates with all
and with no features, `cargo fmt --all --check`, Clippy with `-D warnings`), `task docs`
(`cargo run --locked -p llm-docs -- generate --check`), `ess specify validate --path spec`,
`task conformance` (`cargo run --locked -p b10x-llm-conformance -- check`) and
`aep plan artifact validate`. Each runs alone as written. CI (`.github/workflows/gate.yml`) runs the
same steps on Rust 1.98.0. The site builds with `npm --prefix website ci && npm --prefix website
run build`; `.github/workflows/pages.yml` runs that, the `llm-docs` tests and
`llm-docs provenance` on every push and pull request.

## Documentation

The public site is `website/` (Docusaurus, `@beyond10x/docs-system` pinned in
`website/package.json`), served at `https://beyond10x.github.io/llm/`; README.md is for people and
links the site. `crates/llm-docs` generates three files, never edited by hand:

| File | From |
| --- | --- |
| `website/docs/reference/crates.md` | `cargo metadata --no-deps` |
| `website/data/status.json`, `website/docs/status.mdx` | the `## Status at` section of `docs/implementation-status.md` |

Regenerate with `cargo run --locked -p llm-docs -- generate` (`task docs-generate`);
`generate --check` fails on drift, on a guide block titled `crates/llm-docs/examples/<name>.rs`
that differs from that file, and on an unbracketed admonition title. Guide programs live in
`crates/llm-docs/examples/` and are run before their output is pasted into a page.

A release or a wave that changes a crate, a command, a default or a capability updates
`docs/implementation-status.md` (then regenerates), the hand-written pages it affects, README.md and
this file in the same change. Pages link other components to their public docs and their GitHub
repository. Until the unified-site retirement for llm lands, `b10x.docs.yaml` and the
`b10x-docs-*` workflows stay beside `pages.yml` and `b10x-docs-site.yml`; the unified site collects
only `website/docs/**/*.md`, so a Markdown page links the status page by route (`/docs/status`),
never by file.

Read docs/implementation-status.md for the implemented libraries and remaining planned boundaries.
Do not mark implementation stories complete merely because the workspace builds. Published
contracts are versioned; later consumer adoption pins a released or explicitly qualified exact
revision. Local fixture evidence does not establish live provider or hosting qualification.

<!-- b10x-docs-operations:start -->
## Public documentation operations

This repository owns the public source and presentation allowlist in `b10x.docs.yaml`. The generated credential-free `.github/workflows/b10x-docs-bundle.yml` passively packages only those declared files for the exact successful `main` commit; it must never run repository code. The generated `.github/workflows/b10x-docs-check.yml` runs the publisher's per-source checks on every pull request and main push, with read-only contents and no credentials; it is deliberately separate from the shared gate, which runs on `pull_request_target` with a secret and never reads candidate source. Atlas selects the latest successful bundle with every other catalog source, and Website plus Docs System own rendering, shared components, search, and feeds. Do not add a standalone docs deployer or put App credentials in this public repository. If Atlas catalogs a former Pages workflow, that file remains repository-owned validation: preserve its bespoke checks while keeping exact read-only permissions, an unconditional pull-request trigger, and no deployment primitives. Project Pages at `/llm/` is only the generated stable redirect façade in `.github/workflows/b10x-docs-pages.yml`; content-only publication never rebuilds it.

From the complete organization workspace, verify the contract with a clean Atlas checkout at the current remote `main`. Set `B10X_ATLAS_CHECKOUT` to a managed Atlas worktree when the primary checkout is dirty or stale; never infer command availability from the primary alone.

```bash
atlas_checkout="${B10X_ATLAS_CHECKOUT:-atlas}"
atlas_head="$(git -C "$atlas_checkout" rev-parse HEAD)"
atlas_main="$(git -C "$atlas_checkout" ls-remote origin refs/heads/main | awk '{print $1}')"
test -z "$(git -C "$atlas_checkout" status --porcelain)"
test "$atlas_head" = "$atlas_main"
cargo run --manifest-path "$atlas_checkout/Cargo.toml" --locked -q -- \
  --store "$atlas_checkout/catalog/store" docs reconcile --workspace . --check
```

Keep internal plans, stories, ADRs, decisions, worklogs, security material, and research out of the public allowlist unless a repository authority explicitly declares them public.
<!-- b10x-docs-operations:end -->

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
