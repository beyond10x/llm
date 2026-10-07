---
format: aep.planning-md/3
id: story:agents-own-target
kind: story
status: implemented
title: 'AGENTS.md: build into the tree''s own target/, never a shared CARGO_TARGET_DIR'
relations:
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: AGENTS.md
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-10-07T06:39:47Z", actor: "human:timo", revision: 3}
- {from: "proposed", to: "active", at: "2026-10-07T06:39:47Z", actor: "human:timo", revision: 4}
- {from: "active", to: "implemented", at: "2026-10-07T06:54:49Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"test_result":1,"verification":1},"asserted":{"test_result":1,"verification":1}}}
---
## Outcome

`AGENTS.md` § Gate tells an agent to build into the worktree's own `target/`, never to set
`CARGO_TARGET_DIR`, and to end every tree with `worktree finish --discard-cache --archive`; the
`task rust` row names the pinned-toolchain Clippy run that `Taskfile.yml` has.

## Why

`AGENTS.md` still says "Build with `CARGO_TARGET_DIR=~/.cache/b10x-target/<name>`". The operator
rule of 2026-10-06 replaced the shared target directories: they grew to tens of GiB that no
cleanup could see, and a shared target once ran another worktree's test binary. Builds now go
into the tree's own `target/`, which `worktree discard-cache` recognises and deletes at finish.

A `target/` made by hand before cargo first runs carries no `CACHEDIR.TAG`, and
`worktree discard-cache` then keeps the whole directory as unrecognised (release tree,
2026-10-07: "patch too large" at archive until the tag existed).

`story:gate-pinned-clippy` added `cargo +1.98.0 clippy` to `task rust`; the Gate table does not
mention it.

## Acceptance

- `AGENTS.md` has no instruction to set `CARGO_TARGET_DIR`; it says to build into the tree's own
  `target/`, to let cargo create it, and to finish with
  `worktree finish --discard-cache --archive <tree>`.
- The `task rust` row names Clippy on the default toolchain and on 1.98.0.
- The Gate workflow is green on the pull request head that carries this change into `main`.

## ESS first

None: agent documentation only.
