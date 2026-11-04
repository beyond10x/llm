---
format: aep.planning-md/3
id: approval-record:wave-2026-11-04-w25-proposal
kind: approval-record
status: approved
title: 'Wave 2026-11-04-w25: three adopter-report stories accepted, additive only'
relations:
- decides: story:structured-output
- decides: story:image-input
- decides: story:bedrock-web-identity
revision: 2
transitions:
- {from: "draft", to: "approved", at: "2026-11-04T07:35:38Z", actor: "human:timo", revision: 2}
---
## Decision

Session `b10x-chef-orchestrator`, acting as operator under the owner's grant, 2026-11-04:

> Accept story:structured-output, story:image-input and story:bedrock-web-identity. Unit 1 gets a
> security-reviewer pass. L4 and L5 stay drafts; anthropic-access stays out.

> One condition: a change that breaks a documented contract or a consumer is never under the grant.
> loom, harness, takt and others build on llm. Add the TurnRequest field and the Item variant without
> breaking: either the types are already #[non_exhaustive] (cite it), or the field is an Option with a
> Default or builder path. If a break cannot be avoided, stop before unit 2 and send me the affected
> consumers and the options. Release 0.21.0 only if additive.

> ESS pin: 0.85.0 now; 0.86.0 if I report it verified before your gate.

Wave go, same day, with the build terms of the machine's host monitor: units 0 and 1 tell at 8G or
20 min and stop at 10G or 30 min; units 2 and 3 tell at 8G or 30 min and stop at 10G or 45 min; each
gate stage tells at 8G or 20 min and stops at 10G or 30 min, with the build cache discarded between
stages.

## How the wave reads the condition

- No public type the wave touches is `#[non_exhaustive]`; the only one in `llm-core` is
  `RefusalCause` (`crates/llm-core/src/error.rs:40`).
- `story:structured-output` carries its format as an `Option` field of `Controls`, which derives
  `Default` (`crates/llm-core/src/controls.rs:260`); no known consumer builds `Controls` by literal.
  Its catalog capability and selection rejection would add a field to `Capabilities` (no `Default`,
  built by literal downstream) and a variant to `Rejection`, so they leave this wave.
- `story:image-input` needs a new `Item` variant; consumers match `Item` exhaustively, so it cannot
  be additive and does not run until the operator decides.
