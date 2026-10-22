---
format: aep.planning-md/3
id: story:drop-package-prefix
kind: story
status: draft
title: Packages are named llm-*, without the b10x- prefix
summary: Rename every b10x-llm-* package to llm-*
relations:
- decomposes: epic:contracts
- serves: vision:portable-model-inference
revision: 2
---
## Context

The owner, 2026-10-22: "makes no sense, remove the b10x prefix". Every package of this workspace
was named `b10x-llm-*`, while its directory and library were `llm-*` and `llm_*`, so a consumer
wrote `llm-core = { package = "b10x-llm-core", … }`.

## Change

- Every package `b10x-llm-<x>` is `llm-<x>`; `llm-tool-call`'s library `b10x_llm_tool_call` is
  `llm_tool_call`. Directories and other library names do not change.
- Workspace and crate dependency lines drop the `package = "…"` key they no longer need.
- Live documentation, the generated crate reference, CI and Taskfile commands use the new names.
  Historical records keep the names they were written with.
- `CHANGELOG.md` records the rename as breaking under Unreleased.

## Acceptance

- `git grep -n 'b10x[-_]llm'` finds the old names only in historical records:
  `CHANGELOG.md` released sections, `docs/plan/`, `docs/verification/`, `changes/` and
  `.engineering/`.
- `RUSTUP_TOOLCHAIN=1.98.0 task check` passes, `llm-docs generate --check` included.
- `cargo metadata --no-deps` lists 15 packages, none starting with `b10x-`.
