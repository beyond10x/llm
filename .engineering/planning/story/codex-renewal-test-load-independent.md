---
format: aep.planning-md/3
id: story:codex-renewal-test-load-independent
kind: story
status: draft
title: The Codex renewal resend test decides its claim under load
summary: adversary_codex_renewal's dropped-resolve case failed once at load average 31 on a 500 ms loopback bound
relations:
- decomposes: epic:access
- serves: vision:portable-model-inference
revision: 1
---
## Context

Seen once in wave 2026-10-20-w09 (the deltas unit's gate run): `b10x-llm-credentials`
`adversary_codex_renewal::a_dropped_resolve_does_not_let_the_next_resolve_resend_the_grant` failed
with "the first grant reached the endpoint, left: 0, right: 1" while the machine's load average was
31; it then passed 5 of 5 alone and 10 of 10 in a full rerun. The case gives a request 500 ms to
reach a loopback endpoint. The unit had no change under `crates/llm-credentials`.
`story:http-transport-load-sensitivity` covers the same kind of deadline in the HTTP transport.

## Outcome

The case decides its claim without depending on how fast a loaded machine schedules a request.

## Acceptance

- The case waits on an event (the endpoint seeing the request) rather than a fixed 500 ms, or its
  bound is justified in a comment with the measurement.
- It passes 20 of 20 under a parallel workspace build.
