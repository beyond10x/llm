---
format: aep.planning-md/3
id: approval-record:rust-toolchain-file-2026-11-25
kind: approval-record
status: accepted
title: The toolchain pin is ordered as one pull request ahead of wave 9's build
tags:
- non-interactive
relations:
- decides: story:rust-toolchain-file-pins-the-gate
revision: 2
transitions:
- {from: "draft", to: "accepted", at: "2026-11-25T15:21:43Z", actor: "human:operator", revision: 2}
---
## Decision

`story:rust-toolchain-file-pins-the-gate` is accepted and is delivered as one pull request of its
own, ahead of the build of wave 9.

## Source

The operator session that acts with the owner's permissions ordered it on 2026-11-25: "pin the
Rust toolchain in the llm repository. [...] llm CI uses Rust 1.98.0, but the repository has no
rust-toolchain file, so local jobs resolve to the workstation default `stable`. [...] Change: add
`rust-toolchain.toml` with the exact CI version (check it in your workflow first; components
clippy and rustfmt; targets only what your tasks build) and make CI read the file instead of
naming the version a second time. One small PR, before wave 9's build [...] Do not change code
for a 1.100.0 lint; pin first."

## What it changes

The story moves `draft` to `proposed` to `active` with this record. It joins no wave: the order
names one pull request, written by the coordinator, and the wave plan after wave 8 keeps its
order. `story:gate-pinned-clippy`'s second Clippy run (`cargo +1.98.0 clippy` in `Taskfile.yml`)
ends with it, because the one run is on the pinned release.
