---
format: aep.planning-md/3
id: review-result:adversary-seams-u3-pass-1
kind: review-result
status: active
title: 'Wave 2026-11-05-llm-seams adversary, story:embed-route-kind, pass 1: one introduced'
relations:
- reviews: story:embed-route-kind
revision: 1
---
## Adversary pass 1, unit 4 (story:embed-route-kind)

Attacked `impl/embed-route-kind`; cases committed as `7b39823c6`
(`crates/llm-embed/tests/adversary_u3_embed.rs`, 10 cases, 1 red). Cases executed 14 → 24, red 1.
Origin: introduced 1, pre-existing 0, undecided 0.

- `crates/llm-embed/src/lib.rs:254`: a request id over 254 bytes (253 with ten or more batches) makes
  `<id>/<position>` pass the id bound and fall back to the bare id, so every batch record shares one id.

Routed by the orchestrator (2026-11-06): a small correction in this wave that refuses such an id before
the first batch (`invalid-request`, not sent), with no silent fallback.

```findings
[{"file": "crates/llm-embed/src/lib.rs", "line": 254, "category": "boundary", "severity": "warning", "verdict": "CONFIRMED", "origin": "introduced", "message": "a valid request id over 254 bytes makes the <id>/<position> record id overflow and fall back to the bare id, so every batch record of the request shares one id (adversary_u3_embed.rs a_request_id_at_its_bound_still_gives_each_batch_its_own_record_id)"}]
```
