---
format: aep.planning-md/3
id: approval-record:seams-u3-correction
kind: approval-record
status: approved
title: Unit 4 refuses a request id that cannot name its batch records
relations:
- decides: story:embed-route-kind
revision: 2
transitions:
- {from: "draft", to: "approved", at: "2026-11-06T07:45:25Z", actor: "human:timo", revision: 2}
---
## Decision

Session `b10x-chef-orchestrator`, acting as operator under the owner's grant (orchestrator, 2026-11-06), on
`review-result:adversary-seams-u3-pass-1`:

> Decision: fix in a small U3 correction in this wave by refusing such an id before the first batch
> (invalid-request, not-sent) — no silent fallback. Record it in your store.
