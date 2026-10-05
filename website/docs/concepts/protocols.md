---
title: Protocol projections
sidebar_position: 3
description: Responses, Messages and Chat Completions, each projected onto the neutral turn for a declared subset, in both directions, with a single-attempt client for each.
lede: Each wire is one crate with one codec for both directions and one client that makes one attempt and streams events to the caller as they arrive.
source: crates/llm-responses, crates/llm-messages, crates/llm-chat, docs/responses.md, docs/messages.md, docs/chat.md, CHANGELOG.md 0.1.5–0.1.7
---

# Protocol projections

A **projection** maps the neutral turn onto one wire protocol and back. Each of the three is a
separate crate, and each serves both directions: an outgoing request to a model endpoint, and an
incoming request from a client that speaks that wire. One codec for both directions means a client
and a gateway cannot disagree about the subset.

| Crate | Protocol | Request path | Client | Ingress decoder |
| --- | --- | --- | --- | --- |
| `b10x-llm-responses` | Responses | `{base}responses` | `ResponsesClient` | `ingest_request` |
| `b10x-llm-messages` | Messages | `{base}messages` | `MessagesClient` | `decode_request` |
| `b10x-llm-chat` | Chat Completions | `{base}chat/completions` | `ChatClient` | `decode_ingress_request` |

Every client implements `Model`. It checks the request against its binding, resolves the credential
for the attempt, sends one request through the bounded transport, and hands each decoded event to
the caller's sink as it arrives. `ResponsesClient` also bounds the projected body before it resolves
the credential.
It never retries; [routing](routing.md#same-target-retry) decides whether another attempt follows.

## Rules all three follow

- **Refuse, never drop.** A request field outside the subset is refused by name. A translation that
  silently loses a field only looks like it succeeded.
- **The binding decides authentication.** `anonymous` sends no credential header, `bearer` sends
  `Authorization: Bearer …`, and `api-key` sends the header the account names. The protocol
  decides none of this, and none of it changes billing.
- **The wire gets the upstream model name.** The request carries the binding's `upstream_name`,
  never the caller's route alias.
- **Absent stays absent.** A sampling setting the caller did not set is not written, so the
  provider's default stays the provider's decision.
- **Counters are normalized once.** Each projection maps its wire's usage fields onto the neutral
  counters. A counter the wire did not report stays unknown. A zero the wire reported is kept as a
  report.
- **Failures after dispatch say so.** A refusal raised while reading a response never claims the
  request was `not-sent`, so a caller is never invited to resend a turn the provider may already
  have billed.

## Responses

- Requests are always streamed (`stream: true`) and never stored (`store: false`). The caller owns
  the conversation and replays it whole each turn.
- `encode_request` returns the exact bytes the client sends, so a test or a log can pin them.
- The standing instruction becomes a leading `developer` message.
- Tool results are always enveloped as `{"ok": <bool>, "output": <value>}`, successes included,
  so a round trip cannot flip the `failed` flag.
- **Events arrive live.** The client hands each event to the caller as it is decoded. Text the
  caller was shown through deltas stays in the turn when the terminal output omits it, and a
  `keepalive` event is not an answer.
- A turn ends only at a `response.completed` or `response.incomplete` payload. The end of the stream
  is not a terminal answer. If a terminal `output` is empty after items were streamed, the streamed
  items stand.
- A failure after any output was decoded is final: the turn is never replayed.
- Every request asks for `reasoning.encrypted_content`. Reasoning entries a client sends in are
  carried unattributed, like Messages thinking.
- OpenAI streams reasoning as `response.reasoning_summary_*` events and vLLM as
  `response.reasoning_*`. Both spellings become reasoning deltas.
- An unrecognized stream event is kept as an opaque item and reported as a warning, not dropped.

### Conversation identity, opt-in

A client given a `Conversation` (`ResponsesClient::with_conversation`) sends, on every request,
`prompt_cache_key` equal to the conversation's identifier, a `session-id` header with the same
value and an `x-client-request-id` header that numbers the request within the conversation;
`Conversation::with_originator` adds an `originator` header. A client without one sends none of
these. `request_headers` lists exactly what is sent. llm never invents an identifier: the caller
mints it.

### The Codex backend

`b10x-llm-tool-call`'s `codex_model` binds a `ResponsesClient` to the Codex backend
(`https://chatgpt.com/backend-api/codex`) with the operator's Codex login as the credential; see
[credentials](credentials.md#a-codex-login). Two of its habits are handled: a successful answer
with no `content-type` is read as an event stream when the request asked for one (a success naming
another type is still refused), and the empty terminal output it sends after streamed items keeps
the streamed items.

## Messages

- One route, `POST {base}messages`, in streaming mode, with a pinned `anthropic-version: 2023-06-01`
  header.
- **Prompt caching.** Every request marks two cache breakpoints (`cache_control: ephemeral`): one
  on the standing instruction, sent as a block list so it can carry one, and one on the last
  markable block of the conversation's tail, so each turn reads the previous turn's prefix from
  the cache.
- The wire requires `max_tokens`. If the caller sets no limit, the binding's declared maximum is
  sent, because that is the operator's statement and not a guess.
- A temperature above `1.0` is refused, because this wire's range is narrower than the neutral one.
- Signed and redacted thinking cross as opaque state bound to all six coordinates. On ingress they
  arrive unattributed and are not sendable until the caller binds them.
- An unknown stream event, delta or content block is kept as an opaque item with a warning.
- The stream has no `[DONE]` sentinel. `message_stop` ends it, and a stream that simply stops is
  refused with its last valid snapshot attached.

## Chat Completions

The widest reach: any server that speaks this wire, including an anonymous local vLLM server.

- A streamed request always sends `stream_options: {"include_usage": true}`. Without it the wire
  reports no usage, which would look the same as a model that consumed nothing.
- `max_output_tokens` becomes `max_completion_tokens`.
- A failed tool result travels inside the text-only tool message as
  `{"error": <output>, "ok": false}`.
- `delta.reasoning_content`, which vLLM reasoning parsers emit, becomes a reasoning delta and is
  never joined to the answer text.
- Opaque continuation state is refused: this wire has nowhere to carry it.

## What this evidence covers

Each projection has its own conformance suite and a recorded set of deliberate mutations that the
suite catches. The suites run against pinned response bytes, loopback sockets and in-process fakes.
A live probe of the Codex backend found the two habits above, which 0.1.6 handles; no provider
route is qualified. The subset each crate implements is listed in full in its repository document:
[responses](https://github.com/beyond10x/llm/blob/main/docs/responses.md),
[messages](https://github.com/beyond10x/llm/blob/main/docs/messages.md) and
[chat](https://github.com/beyond10x/llm/blob/main/docs/chat.md).

[Call a local endpoint](../guides/call-a-local-endpoint.md) runs `ChatClient` against a local
server.
