---
format: aep.planning-md/3
id: approval-record:image-input-breaking-minor
kind: approval-record
status: approved
title: Image input ships as one breaking minor after 0.21.0
relations:
- decides: story:image-input
- decides: story:output-and-image-capabilities
revision: 2
transitions:
- {from: "draft", to: "approved", at: "2026-11-04T08:46:41Z", actor: "human:timo", revision: 2}
---
## Decision

Image input ships as one breaking minor release, in its own wave after 0.21.0 (option B of four).
Decided on 2026-11-04 by the machine's host monitor session for the owner, relayed by session
`b10x-chef-orchestrator`, under the owner's standing words of 2026-10-31: "nothing must be waiting
on me - its your job to act on my behalf".

- One breaking minor: `Item::UserImage`, and `Item`, `Capabilities` and `Rejection` become
  `#[non_exhaustive]` (or gain `Default`), so later additions are additive.
  `story:output-and-image-capabilities` folds into that wave.
- harness and control-plane keep their llm pins (0.17.0 and 0.4.0) until their own pin moves; at that
  move harness adds a wildcard arm to its `Item` match.
- The release notes and the wave's pull request body state the break and that consumer cost.

## Options that were weighed

A. A breaking `Item::UserImage` alone. B. The same, with the three types made non-exhaustive in the
same release (chosen). C. An additive side channel carrying images in `Controls` by item index.
D. Keep image input a draft.

## Why it was needed

Wave 2026-11-04-w25 runs additive only (`approval-record:wave-2026-11-04-w25-proposal`). `Item`
(`crates/llm-core/src/item.rs:17`) is not `#[non_exhaustive]`, and consumers match it exhaustively.
