---
format: aep.planning-md/3
id: story:relayed-failures-keep-their-retry-decision
kind: story
status: active
title: A failure relayed through an ingress encoder reads back with the relaying side's retry decision
summary: A relayed unavailable, rate-limited, transport, protocol or cancelled failure reads back retriable in a downstream llm client
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: crates/llm-http/src/transport.rs
- confidence: inferred
  path: crates/llm-http/tests/retry_classes.rs
- confidence: cited
  path: crates/llm-messages/src/decode.rs
- confidence: cited
  path: crates/llm-messages/src/ingress.rs
- confidence: cited
  path: crates/llm-messages/tests/adversary_w09_retry.rs
- confidence: cited
  path: crates/llm-responses/src/ingress.rs
- confidence: cited
  path: crates/llm-responses/src/stream.rs
- confidence: cited
  path: crates/llm-responses/tests/adversary_w09_retry.rs
- confidence: cited
  path: docs/design.md
- confidence: cited
  path: docs/harness-parity.md
- confidence: cited
  path: docs/messages.md
- confidence: cited
  path: docs/responses.md
revision: 8
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T07:47:10Z", actor: "human:timo", revision: 7}
- {from: "proposed", to: "active", at: "2026-10-20T07:47:10Z", actor: "human:timo", revision: 8}
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

## Scope

Derived 2026-10-20 by `story-scoper` at tag 0.8.0 (`7873aa63`). Every line is **cited** (read from the story or the tree) or **inferred** (a reading that could be wrong).

- **Primary surface (as filed):** the failure path of the Responses and Messages ingress encoders: `crates/llm-responses/src/ingress.rs:390-430` (`IngressStream::fail`), `:689-707`, `:23-48`; `crates/llm-messages/src/ingress.rs:258-289`, `:658-669` — cited
- **Reading side:** `crates/llm-responses/src/stream.rs:745-775` (`provider_failure`, mark from `error.code` alone); `crates/llm-messages/src/decode.rs:313`, `:733-778` (`stream_error`, from `error.type` alone) — cited
- **Tests that flip:** `crates/llm-responses/tests/adversary_w09_retry.rs:66`, `crates/llm-messages/tests/adversary_w09_retry.rs:75`; the `adversary_w09_retry_pass2.rs` files assert every row of the encoders' table — cited
- **Routing:** no change; `exhausted` already clears the mark once retries are spent (`crates/llm-routing/src/fallback.rs:280-291`) — cited
- **Chat:** not affected; no `IngressStream::fail` and no retry mark in `crates/llm-chat/src` — cited
- **Reach through the gateway (changes the story):** llm-gateway calls `fail` only after the head is written (`crates/llm-gateway-cli/src/translate.rs:1008-1016` in that repository), and the head is written only with a payload the downstream decoder counts as output, so that failure already reads back final. A failure before output is answered HTTP 502 (`crates/llm-gateway/src/error.rs:157-162`, `src/relay.rs:1286-1297`), which every llm version reads as a retriable `transport` (`crates/llm-http/src/transport.rs:336-350`; only `Retry-After` is read, `:354`). The gateway makes one upstream attempt per request (`translate.rs:647-649`) — inferred, step 3, unproven
- **Also likely:** `crates/llm-http/src/transport.rs:336-350`, `crates/llm-http/tests/retry_classes.rs` if the HTTP channel is in the unit — inferred
- **Documents:** `docs/responses.md:400-418`, `:442-447`; `docs/messages.md:222-226`, `:290-296`; `docs/design.md:215-240`; `docs/harness-parity.md:344-349`; `CHANGELOG.md` — cited
- **Confidence:** medium — the wire option decides the files
- **Would collide with:** any unit on the encoders' failure path, the decoders' failure classification, `docs/design.md` "Provider failures retried before output", and, with the HTTP channel, `status_error` in `crates/llm-http/src/transport.rs` (also `story:quota-429-is-final`) — cited
- **Safety fact:** 0.7.0 and 0.8.0 readers parse a field added to a failure payload (Responses reads only `error.code`, `stream.rs:747-750`; Messages passes the `error` event to `stream_error` without the unknown-key check, `decode.rs:313` against `:306`) and still decide by code — unproven

## Open for the design

| Option | What it does | Reach |
| --- | --- | --- |
| A | a field on the failure payload the new decoders read | only failures relayed mid-stream, which already read final through the gateway |
| B | write a final failure under a code readers treat as final | changes the class read back; Codex retries unknown codes |
| C | a marker event before the failure | adds a warning routing counts as output; breaks the encoders' "only what our decoder interprets" rule |
| D | an HTTP signal on the gateway's 502 before output (a header the transport reads, or a 4xx) | the path where the re-retry happens; needs llm-gateway code as well as llm-http |

## Design decisions

Taken 2026-10-20 by session `adr-changes` under the owner's grant: option **D** of `## Open for
the design`. llm's side: the HTTP transport reads a do-not-retry signal on an error status, namely
`x-should-retry: false` if the official OpenAI and Anthropic SDKs' sources honour that header
(cite their lines), otherwise a named llm header; a test that a gateway-style 502 carrying it
causes no client retry. llm-gateway's sending side is its own story, in its own repository.
