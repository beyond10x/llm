---
format: aep.planning-md/3
id: story:public-surface
kind: story
status: implemented
title: The repository carries its public documentation, changelog and gating surface
relations:
- decomposes: epic:contracts
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: .github/workflows
- confidence: cited
  path: AGENTS.md
- confidence: cited
  path: CHANGELOG.md
- confidence: cited
  path: LICENSE
- confidence: cited
  path: README.md
- confidence: cited
  path: b10x.docs.yaml
- confidence: cited
  path: changes
- confidence: cited
  path: website
revision: 13
transitions:
- {from: "draft", to: "proposed", at: "2026-09-19T23:42:39Z", actor: "human:timo", revision: 10, imported: true}
- {from: "proposed", to: "active", at: "2026-09-19T23:42:39Z", actor: "human:timo", revision: 11, imported: true}
- {from: "active", to: "implemented", at: "2026-09-21T08:45:40Z", actor: "human:timo", revision: 13, decided_on: {"recorded":{"test_result":1,"review_outcome":1,"ess_conformance_coverage_v1":1}}, imported: true}
---
## Context

The repository publishes source but carries none of the organization's public surface. Eventlog and
ESS each carry a documentation manifest, a passive bundle producer, a redirect facade, a changelog,
a licence and one shared Gates workflow; this repository carries none of them, and its Atlas catalog
subject exists only on an unpushed branch at `visibility: private` with no description, no
documentation surface and no delivery records.

Atlas refuses an active public repository that has no published documentation surface, and it
refuses a surface without both a `canonical` and a `source-alias` delivery record. No CLI verb
creates a delivery record, so the subjects are hand-authored. The independent-source roster that
lets a repository publish without Atlas admission is Rust source, not data, so adding this
repository to it is a source change in Atlas.

Operator direction, 2026-09-19: "prepare the wiring with ./atlas and create the public website docs
for this new llm repo, also make sure there is an AGENTS.md and a README.md - also setup
CHANGELOG.md, gating, etc", and open the first documentation draft in a browser.

## Acceptance

The repository carries a validated documentation manifest, a first public documentation tree that
builds, a changelog, a licence and the shared Gates workflow; and the Atlas catalog validates with
this repository declared public, carrying a documentation surface and both delivery records.

## Evidence

Reference surfaces read 2026-09-19: `eventlog/b10x.docs.yaml`, `eventlog/.github/workflows/`,
`eventlog/CHANGELOG.md`, `ess/website/`, `atlas/src/catalog.rs`, `atlas/src/workspace.rs`,
`atlas/src/docs.rs`, `gates/README.md`, `gates/docs/adoption.md`.

## Verification

The documentation site builds from a clean checkout. Atlas catalog validation and its rendered
projection check both pass. The manifest states the schema version Atlas accepts. Administrator
enrollment of this repository in the private Gates policy, and the website repository's own roster
and lock, are separate owners' steps and are named rather than claimed.

## Scope

Derived 2026-09-20 by `story-scoper`. Every line is **cited** (read from the artifact body or an
opened file in this tree or a reference sibling) or **inferred** (a reading that could be wrong).

- cited: `README.md` — the artifact scope names it; the human entry point and the home of the
  `<!-- b10x-docs:start -->` / `<!-- b10x-docs:end -->` block the siblings carry
  (`eventlog/README.md:209`, `ess/README.md:157`). The file exists and is edited, not created.
- cited: `AGENTS.md` — the artifact scope names it; receives the marked
  `b10x-docs-operations` and `b10x-release-operations` blocks (`ess/AGENTS.md:189`, `:209`).
  Exists and is edited.
- cited: `CHANGELOG.md` — named in the artifact scope and acceptance. Absent from this repository;
  created new, in the `ess/CHANGELOG.md` `## [Unreleased]` shape.
- cited: `LICENSE` — named in the artifact scope and acceptance. Absent; created new.
- cited: `b10x.docs.yaml` — named in the artifact scope, evidence and acceptance as the validated
  documentation manifest. Absent; created new against `b10x-docs/v4` (`ess/b10x.docs.yaml`).
- cited: `website` — named in the artifact scope and evidence (`ess/website/`) as the first public
  documentation tree that builds. Absent; a whole new Docusaurus tree, sole owner this story.
- cited: `changes` — named in the artifact scope as the published impact feed entry. Absent; one
  new `b10x-change/v1` file (`ess/changes/standalone-0.1.0.yaml`,
  `eventlog/changes/public-surface-2026-09-01.yaml`).
- cited: `.github/workflows` — named in the artifact scope for the shared Gates workflow, the
  passive bundle producer and the redirect facade. The directory exists and holds only `gate.yml`.
- inferred: `.github/workflows/shared-gates.yml` — the shared Gates workflow's filename in both
  siblings; a new file here, calling `beyond10x/gates/.github/workflows/common.yml`.
- inferred: `.github/workflows/b10x-docs-bundle.yml` — the passive bundle producer's filename in
  both siblings; Atlas-generated, new here.
- inferred: `.github/workflows/b10x-docs-pages.yml` — the redirect facade's filename in both
  siblings; Atlas-generated, new here.
- inferred: `.gitignore` — the website's install and build output must be ignored;
  `ess/.gitignore` carries `website/node_modules`, `website/build`, `website/.docusaurus`
  alongside a `website/.gitignore`. This repository's `.gitignore` has two lines and neither
  covers a Node tree.

The Atlas half of the acceptance — the catalog subject at `visibility: public`, the two delivery
records, the independent-source roster entry and the objective map — lands in the **Atlas
repository**, not here. It cannot be satisfied from this worktree and must not be reported as done
from it.

This story is the repository's public surface and nothing else. It touches no crate, no
`spec/domains/*.yaml`, no `contracts/*/scenarios/`, no `checks/conformance/src/`, and no file under
the repository-root `docs/` tree, which is where the five sibling wave-1 candidates land. No path
above appears in any of their declared scopes.
