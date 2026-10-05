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
repository. `pages.yml` (Documentation validation: build, provenance and route inventory) and
`b10x-docs-site.yml` (Documentation site: Website's `project-site.yml`) publish the site; the
unified-site files (`b10x.docs.yaml`, `b10x-docs-bundle.yml`, `b10x-docs-check.yml`,
`b10x-docs-pages.yml`) are retired and do not come back. Keep page paths and heading IDs stable:
the organization Website redirects the former `/docs/llm/` pages to them. A Markdown page links
the status page by route (`/docs/status`), never by file.

Read docs/implementation-status.md for the implemented libraries and remaining planned boundaries.
Do not mark implementation stories complete merely because the workspace builds. Published
contracts are versioned; later consumer adoption pins a released or explicitly qualified exact
revision. Local fixture evidence does not establish live provider or hosting qualification.

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
