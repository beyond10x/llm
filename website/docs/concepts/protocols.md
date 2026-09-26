---
title: Protocol projections
description: Chat Completions, Messages and Responses, each projected onto the neutral turn for a declared subset, in both directions, refusing what the subset does not carry.
---

# Protocol projections

A **projection** maps the neutral turn onto one wire protocol and back. Each of the three is a
separate crate, and each serves both directions: an outgoing request to a model endpoint, and an
incoming request from a client that speaks that wire. One codec for both directions means a client
and a future gateway cannot disagree about the subset.

| Crate | Protocol | Request path | Outgoing client | Ingress decoder |
| --- | --- | --- | --- | --- |
| `b10x-llm-chat` | Chat Completions | `{base}chat/completions` | `ChatClient` | `decode_ingress_request` |
| `b10x-llm-messages` | Messages | `{base}messages` | `MessagesClient` | `decode_request` |
| `b10x-llm-responses` | Responses | `{base}responses` | none: a pure codec | `ingest_request` |

`ChatClient` and `MessagesClient` implement `Model`: they resolve the credential for the attempt,
send one request through the bounded transport and decode the stream. `llm-responses` performs no
I/O at all. It builds request bodies (`project_request`) and decodes streamed payloads
(`decode_stream`), and the caller supplies the HTTP exchange.

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

## Messages

- One route, `POST {base}messages`, in streaming mode, with a pinned `anthropic-version: 2023-06-01`
  header.
- The wire requires `max_tokens`. If the caller sets no limit, the binding's declared maximum is
  sent, because that is the operator's statement and not a guess.
- A temperature above `1.0` is refused, because this wire's range is narrower than the neutral one.
- Signed and redacted thinking cross as opaque state bound to all six coordinates. On ingress they
  arrive unattributed and are not sendable until the caller binds them.
- The stream has no `[DONE]` sentinel. `message_stop` ends it, and a stream that simply stops is
  refused with its last valid snapshot attached.

## Responses

- Requests are always streamed (`stream: true`) and never stored (`store: false`). The caller owns
  the conversation and replays it whole each turn.
- The standing instruction becomes a leading `developer` message.
- Tool results are always enveloped as `{"ok": <bool>, "output": <value>}`, successes included,
  so a round trip cannot flip the `failed` flag.
- A turn ends only at a `response.completed` or `response.incomplete` payload. The end of the stream
  is not a terminal answer.
- Every request asks for `reasoning.encrypted_content`. Reasoning entries a client sends in are
  carried unattributed, like Messages thinking.
- OpenAI streams reasoning as `response.reasoning_summary_*` events and vLLM as
  `response.reasoning_*`. Both spellings become reasoning deltas.
- An unrecognized stream event is kept as an opaque item and reported as a warning, not dropped.

## What this evidence covers

Each projection has its own conformance suite and a recorded set of deliberate mutations that the
suite catches. All of it runs against pinned response bytes, loopback sockets and in-process fakes.
None of it has been run against a live provider. The subset each crate implements is listed in
full in its repository document:
[chat](https://github.com/beyond10x/llm/blob/main/docs/chat.md),
[messages](https://github.com/beyond10x/llm/blob/main/docs/messages.md) and
[responses](https://github.com/beyond10x/llm/blob/main/docs/responses.md).

[Call a local endpoint](../guides/call-a-local-endpoint.md) runs `ChatClient` against a local
server.
