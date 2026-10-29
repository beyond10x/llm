---
format: aep.planning-md/3
id: review-result:adversary-w21-codex-stream-metadata-pass-1
kind: review-result
status: active
title: 'Adversary pass 1: story:codex-stream-metadata-is-not-replayed'
relations:
- reviews: story:codex-stream-metadata-is-not-replayed
revision: 1
---
needs-change, resolved on the wave branch

Adversary pass 1 on `story:codex-stream-metadata-is-not-replayed` (commit 1e4ceec4, wave
2026-10-29-w21, D202). Eight cases added in `crates/llm-responses/tests/adversary_w21_metadata.rs`:
the event at every position, repeated, with eleven odd payloads, after the terminal object, with a
terminal-shaped payload, an overload after only the event (still final, as before), ingress refusing
a replayed entry, and the recorded live Codex text turn. Seven passed on first run; the eighth is
the pre-existing finding below, pinned to today's behaviour with its story id.

```findings
[
{"file":"crates/llm-responses/tests/live_capture.rs","line":226,"category":"contract-drift","severity":"blocker","verdict":"NEEDS-CHANGE","origin":"introduced","message":"the live-capture replay still asserted a preserved response.metadata item and an unknown-stream-event warning; fixed on the wave branch, assertions cite story:codex-stream-metadata-is-not-replayed"},
{"file":"docs/verification/responses.md","line":213,"category":"contract-drift","severity":"warning","verdict":"NEEDS-CHANGE","origin":"introduced","message":"the live-capture record still described the metadata event as an Opaque item with a warning; fixed on the wave branch"},
{"file":"crates/llm-responses/src/request.rs","line":623,"category":"acceptance","severity":"warning","verdict":"CONFIRMED","origin":"pre-existing","message":"a response.metadata opaque item saved under 0.16.0 is still projected verbatim; doc row narrowed, case pinned and filed as story:saved-codex-metadata-item-on-resume"}
]
```
