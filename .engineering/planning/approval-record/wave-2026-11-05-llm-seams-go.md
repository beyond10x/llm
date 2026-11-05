---
format: aep.planning-md/3
id: approval-record:wave-2026-11-05-llm-seams-go
kind: approval-record
status: approved
title: 'Wave 2026-11-05-llm-seams: go for 0.23.0 with four units, one tree at a time'
relations:
- decides: story:call-records
- decides: story:closed-label-classifier
- decides: story:shadow-calls
- decides: story:embed-route-kind
revision: 2
transitions:
- {from: "draft", to: "approved", at: "2026-11-05T18:22:53Z", actor: "human:timo", revision: 2}
---
## Decision

Session `b10x-chef-orchestrator`, acting as operator under the owner's grant (orchestrator,
2026-11-05):

> Go for llm 0.23.0 with U1 call-records, U2 closed-label-classifier, U3 embed-route-kind and U4
> shadow-calls.

> One unit tree at a time, never side by side: U1, U2, U4, then U3. My waves share a 20G trees
> budget, and slots-max is 1. Finish each tree with worktree finish --discard-cache --archive when
> it merges.

> The ESS pin moves to 0.87.0, released and verified 2026-11-05, not 0.86.0. U1 carries it, and
> CI's ess moves with it.

> Terms: every cargo call through build-slot.sh with the wave-brief flags; pre-check per unit:
> df ≥ 25G and trees-budget.sh exit 0, else EXIT=precheck; your estimates as limits: tell at 8G or
> 20 cargo min, stop at 10G or 30, and for U3 stop at 9G; start and end lines per unit; gate S1-S4
> as proposed, BUILD_SLOTS=2, discard between stages, minutes recorded per stage.

> At most 3 sub-agents. An adversary on each unit; a security-reviewer on U4 (money cap).

> On green: one bot PR from wave-2026-11-05-llm-seams, merge on green, tag 0.23.0 after main is
> green, release, verify-release.sh into the verify file, the announcement file.

> The llm-gateway stories wait for its own wave after this release, with the pin first.

## Commits this go authorises

Unit commits and their fix commits, one merge per unit into `wave-2026-11-05-llm-seams`, the ESS
0.87.0 pin (in U1), the release-prepare commit, the closing store commit, one bot pull request to
`main`, the merge on green, tag `0.23.0` and its GitHub Release. Nothing else.
