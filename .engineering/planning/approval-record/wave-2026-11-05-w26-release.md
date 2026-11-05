---
format: aep.planning-md/3
id: approval-record:wave-2026-11-05-w26-release
kind: approval-record
status: approved
title: 'Wave 2026-11-05-w26: PR, bot merge, tag and release of 0.22.0 on green CI'
relations:
- decides: story:core-types-non-exhaustive
- decides: story:image-input
- decides: story:bedrock-web-identity-route
- decides: story:output-and-image-capabilities
revision: 2
transitions:
- {from: "draft", to: "approved", at: "2026-11-05T05:25:14Z", actor: "human:timo", revision: 2}
---
## Decision

Session `b10x-chef-orchestrator`, acting as operator under the owner's grant, 2026-11-05 (D525):

> go for llm 0.22.0 PR, merge through the bot, tag and release on green PR CI without asking again;
> check `gh pr checks <n>` yourself first.

> Drafts cli-web-identity and cli-route-explain stay drafts.

Earlier on the same wave (D517): no new unit build while the coordinator's agents' trees hold 20G or
more, checked with the shared trees budget before each unit and adversary build.

## Gate it rests on

Local gate at `fdf05bb9b` on Rust 1.98.0, four stages under the D497 terms, the build cache discarded
between stages: S1 1,004 and S2 862 tests passed, 0 failed (peaks 4G and 7G); fmt and Clippy
`-D warnings` (1G); docs drift, `ess specify validate`, conformance 1013/1013 three times and the store
valid (2G). `/` stayed at 75G free or more.
