---
format: aep.planning-md/3
id: story:rust-toolchain-file-pins-the-gate
kind: story
status: implemented
title: rust-toolchain.toml pins the one Rust release the local gate and CI both read
relations:
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: .github/workflows/gate.yml
- confidence: cited
  path: .github/workflows/pages.yml
- confidence: cited
  path: AGENTS.md
- confidence: cited
  path: CHANGELOG.md
- confidence: cited
  path: Taskfile.yml
- confidence: cited
  path: checks/xtask/tests/gate/main.rs
- confidence: cited
  path: checks/xtask/tests/gate/toolchain.rs
- confidence: cited
  path: rust-toolchain.toml
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-11-25T15:21:43Z", actor: "human:operator", revision: 3}
- {from: "proposed", to: "active", at: "2026-11-25T15:21:43Z", actor: "human:operator", revision: 4}
- {from: "active", to: "implemented", at: "2026-11-25T19:02:33Z", actor: "human:operator", revision: 5, decided_on: {"recorded":{"test_result":1,"verification":1}}}
---
## Context

The workspace rule "The local gate equals the forge" asks for one pinned rustc that the local gate
and CI both read. Until this story the repository had no `rust-toolchain.toml`:

- `.github/workflows/gate.yml` named `toolchain: 1.98.0` twice and `.github/workflows/pages.yml`
  once, each as an input of the action `dtolnay/rust-toolchain`.
- A local `cargo` resolved the host's default toolchain. `Taskfile.yml` task `rust` therefore ran
  Clippy twice over the workspace: once on that default and once as `cargo +1.98.0 clippy`
  (`story:gate-pinned-clippy`).
- Every other local step (`cargo fmt`, `cargo check`, the test phase, the docs and conformance
  programs) compiled on the host's default only. On 2026-11-25 that default moved from 1.99.1 to
  1.100.0: a local gate then compiles and lints with a release CI never ran, and a lint new in it
  can fail `-D warnings` locally while CI stays green.

## Outcome

`rust-toolchain.toml` pins one exact Rust release with Clippy and rustfmt. A local `cargo` reads
it, and each workflow job that compiles installs what it names with `rustup toolchain install`. No
workflow and no task names a toolchain a second time, and a test of the gate crate finds one that
does.

## Acceptance

- `rust-toolchain.toml` at the repository root names one exact release (`major.minor.patch`), the
  one CI ran before this story (1.98.0), with the components `clippy` and `rustfmt`, and no
  target: every task and job builds for its own host.
- In `.github/workflows/gate.yml` (jobs `libraries-and-plan` and `native-adapter-builds`) and
  `.github/workflows/pages.yml` (job `build`) the step before the first `cargo` is
  `rustup toolchain install` with no argument. No workflow uses a toolchain action or a
  `toolchain:` input.
- `Taskfile.yml` task `rust` runs Clippy over the workspace once; no command there carries
  `+<toolchain>`.
- `checks/xtask/tests/gate/toolchain.rs` fails when: the file's channel is not an exact release
  or is older than the manifest's `rust-version`; a component is missing; a workflow or the
  Taskfile names a toolchain of its own (`+<toolchain>` after `cargo`, `rustc`, `rustfmt`,
  `rustdoc` or `cargo-clippy`, `RUSTUP_TOOLCHAIN`, a toolchain action, a `toolchain:` input, a
  `rustup` command other than the bare install); a job runs `cargo` in a step with no bare
  install step before it. Each of these drifts is planted in a text of the test's own and found.
- `rustc --version` in the repository prints 1.98.0 on the host, and the pull request's `Gate`
  and `Documentation validation` runs end green on Linux, macOS and Windows runners.
- `AGENTS.md` § Gate says where the release is pinned and how to move it.

## Verification

- `cargo nextest run -p llm-xtask --test gate` (the cases of `toolchain.rs` among them).
- `task rust` on the host, on the pinned release.
- The forge's `Gate`, `Documentation validation` and `Docs UX gate` runs on the pull request head.

## ESS first

None: a toolchain file, workflow steps, a task line and a test of the gate crate. No behaviour,
noun or contract of a library changes.

## Scope

- `rust-toolchain.toml` (new)
- `.github/workflows/gate.yml`, `.github/workflows/pages.yml`
- `Taskfile.yml`
- `checks/xtask/tests/gate/toolchain.rs` (new), `checks/xtask/tests/gate/main.rs`
- `AGENTS.md`, `CHANGELOG.md`

No file under `crates/`. No code is changed for a lint of a release newer than the pinned one.
