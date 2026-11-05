---
format: aep.planning-md/3
id: approval-record:wave-2026-11-05-w26-proposal
kind: approval-record
status: approved
title: 'Wave 2026-11-05-w26: the breaking minor 0.22.0 and its four units'
relations:
- decides: story:image-input
- decides: story:output-and-image-capabilities
- decides: story:bedrock-web-identity-route
- decides: story:core-types-non-exhaustive
revision: 2
transitions:
- {from: "draft", to: "approved", at: "2026-11-05T01:52:02Z", actor: "human:timo", revision: 2}
---
## Decision

Session `b10x-chef-orchestrator`, acting as operator under the owner's grant, 2026-11-05:

> I accept story:output-and-image-capabilities (U3) and story:bedrock-web-identity-route (U4): move
> them to active.

> Go for wave 0.22.0, units U1 → (U2 ∥ U4) → U3, one tree per unit, as planned.

> The PR body and release notes name the consumer cost: harness adds a wildcard Item arm at its own
> pin move.

The breaking minor itself was decided in `approval-record:image-input-breaking-minor`, under the
owner's words of 2026-10-31: "nothing must be waiting on me - its your job to act on my behalf".

Build terms: units tell at 8G or 20 cargo minutes and stop at 10G or 30; every cargo call through the
shared build slot; `df -h /` once before each unit, none under 45G free; U2 and U4 never build in the
same tree. Gate S1 to S4 with two slots: start with `/` free at least the stage's peak plus 40G and
never under 45G; stop at the peak plus 2G or twice the cargo minutes, or when `/` falls under 40G;
the build cache discarded between stages.

## Units

1. U1: `Item`, `Capabilities`, `Rejection` and `AwsCredentialKind` become `#[non_exhaustive]`, or
   gain `Default`, so later additions are additive. This is the release's breaking change.
2. U2, after U1: `story:image-input`.
3. U4, after U1 and beside U2: `story:bedrock-web-identity-route`.
4. U3, after U2: `story:output-and-image-capabilities`.
