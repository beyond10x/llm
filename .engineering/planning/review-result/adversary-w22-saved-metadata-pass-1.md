---
format: aep.planning-md/3
id: review-result:adversary-w22-saved-metadata-pass-1
kind: review-result
status: active
title: 'Adversary pass 1: story:saved-codex-metadata-item-on-resume'
relations:
- reviews: story:saved-codex-metadata-item-on-resume
revision: 1
---
confirmed, findings resolved on the wave branch

Adversary pass 1 on `story:saved-codex-metadata-item-on-resume` (wave head a98f0b09, wave
2026-10-29-w22, D208). Nine cases in `crates/llm-responses/tests/adversary_w22_saved_metadata.rs`:
look-alike types still sent, any payload beside the type left out, a conversation that empties or
ends with the item, every binding coordinate still refused, unattributed state still refused, 400
generated conversations through `encode_request`, the ingest round trip, both ingress encoders,
and `ResponsesClient` over loopback. All nine green on first run; no red case.

```findings
[
{"file":"crates/llm-responses/src/request.rs","line":618,"category":"mutant","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"a suffix match and a match that also requires a metadata object passed the unit tests; the adversary_w22 cases now fail both"},
{"file":"AGENTS.md","line":188,"category":"contract-drift","severity":"warning","verdict":"CONFIRMED","origin":"introduced","message":"AGENTS.md and docs/design.md said opaque state is carried or refused, never dropped; both now name the D208 exception (fixed in b37d5576)"},
{"file":".engineering/planning/story/saved-codex-metadata-item-on-resume.md","line":1,"category":"judgement","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"the acceptance named no conformance scenario; scenario egress-leaves-out-a-saved-metadata-item added, floor 954 (fixed in b37d5576)"}
]
```
