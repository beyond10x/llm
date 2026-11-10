---
format: aep.planning-md/3
id: story:codex-renewal-write-back-fully-bounded
kind: story
status: draft
title: Every filesystem step of a Codex renewal write-back ends within a bound
summary: 'Pre-existing: write, sync, rename and directory open still block unbounded'
tags:
- from-finding
relations:
- informed_by: review-result:adversary-github-zero-u5-pass-1
- serves: vision:portable-model-inference
revision: 1
---
## Context

From `review-result:adversary-github-zero-u5-pass-1`, findings 2 and 3 (INFEASIBLE, pre-existing): after the re-read was made non-blocking, the Codex renewal write-back still has unbounded blocking points under `spawn_blocking` (`crates/llm-credentials/src/codex/local.rs:70`): the temp-file create, `write_all` and `sync_all`, the `lstat`, the rename and the blocking open of the parent directory (`crates/llm-credentials/src/codex/renewal.rs:846-888` at `77690c68`), and the re-read follows a symlink swapped in after the `lstat` (no `O_NOFOLLOW`, no device and inode match, `renewal.rs:828`). What reaches it: a hung filesystem or a swap race; no test reaches it reliably.

## Outcome

A Codex renewal write-back ends within a stated bound on every filesystem step, and dropping the runtime does not wait on it.

## Acceptance

A test with a parent path swapped to a FIFO after the rename, and a test with a symlink swapped in between `lstat` and re-read, each see the renewal end within the stated bound with `RefreshUncertain`, and both pass.
