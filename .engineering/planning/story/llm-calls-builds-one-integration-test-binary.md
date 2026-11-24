---
format: aep.planning-md/3
id: story:llm-calls-builds-one-integration-test-binary
kind: story
status: draft
title: llm-calls builds one integration-test binary
tags:
- from-finding
relations:
- informed_by: review-result:adversary-w29-offline-replay-eval-pass-1
- decomposes: epic:routing
revision: 1
---
## Context

The rule for this organization's repositories is one integration-test binary per crate
(`tests/main.rs` with modules), because each binary is linked on its own.

`llm-calls` built six integration-test binaries before wave 2026-11-24-offline-replay-eval. That
wave added a seventh, `tests/replay.rs` (the story named that path, and the crate had no single
binary to add a module to). The wave's page records it as a deviation.

## Outcome

`llm-calls` builds one integration-test binary with the seven files as its modules. The same cases
run as before.

## Acceptance

- `cargo test -p llm-calls --no-run` links one integration-test executable.
- The number of executed cases of `cargo test -p llm-calls --all-features` is the same before and
  after, and no case is renamed beyond the module prefix.

## Verification

Targeted: `-p llm-calls` under nextest with the 60 s bound. No live call.
