---
format: aep.planning-md/3
id: story:relayed-failures-keep-their-retry-decision
kind: story
status: draft
title: A failure relayed through an ingress encoder reads back with the relaying side's retry decision
summary: A relayed unavailable, rate-limited, transport, protocol or cancelled failure reads back retriable in a downstream llm client
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
revision: 2
---
## Context

Found by the implementor of `story:retry-classes-harness-retried` in wave 2026-10-20-w09, with a
probe (not a committed test). When an llm ingress encoder relays a failure with nothing streamed
before it, an llm client reading that relayed stream marks it retriable, including failures the
relaying side had marked final:

- Responses: a relayed `unavailable` or `rate-limited` failure reads back retriable.
- Messages: a relayed `unavailable`, `transport`, `protocol` or `cancelled` failure reads back
  retriable.

The ingress encoders (`crates/llm-responses/src/ingress.rs`, `crates/llm-messages/src/ingress.rs`)
write a code the downstream decoder classes by its own rules, and no field carries the upstream's
retry decision.

## Outcome

A failure relayed through an ingress encoder reads back with the retry decision the relaying side
made, or the encoder documents per code what a downstream llm client will decide.

## Acceptance

- A test per protocol relays each failure class the encoder can write, decodes it with that
  protocol's client decoder, and asserts the retry mark equals the relaying side's.
- `docs/responses.md` and `docs/messages.md` state the mapping.

## Next wave

Decided 2026-10-20 by session `adr-changes` under the owner's grant: this story is the first unit
of the wave after 2026-10-20-w09. Since 0.8.0 the four transient classes (Responses `server_error`,
`rate_limit_exceeded`; Messages `overloaded_error`, `api_error`) are retriable before output, so a
failure an llm-gateway relays as final reads back retriable in an 0.8.0 client; the 0.8.0
CHANGELOG names this risk. The unit lets the gateway mark a failure whose retries are spent and the
client read that mark, and adds a test that a gateway-relayed final failure causes no client retry.
