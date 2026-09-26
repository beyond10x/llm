# The Responses projection

`b10x-llm-responses` projects the neutral turn contract onto the `OpenAI` Responses protocol and
reads that same protocol back. It is pure: no I/O, no credential, no endpoint, no retry. One
contract serves both directions, so a gateway ingress surface and an outgoing client disagree
about nothing.

| function | direction |
| --- | --- |
| `project_request(&Binding, &TurnRequest) -> Result<Value, Error>` | neutral turn to wire body |
| `ingest_request(&Binding, &Value) -> Result<TurnRequest, Error>` | wire body to neutral turn |
| `decode_stream(&Binding, &[Value]) -> StreamDecoding` | wire stream to neutral outcome |

A `Binding` is a `Provenance` — protocol, provider, account, endpoint, model, binding revision —
plus the **upstream model name** that binding is configured to send. The two are separate because
the neutral request carries the operator's own model identifier and the wire carries the name the
endpoint answers to. Keeping them apart is what lets ingress refuse a body addressed to another
upstream model, and it is why no absent reported model is ever filled in from the caller's alias.

## What this crate does not own

Server-sent-event **framing** is `llm-http`'s `SseDecoder`. This crate consumes the payload values
that decoder yields and never sees a byte boundary. That is why a route which ends its stream
without the `data: [DONE]` sentinel costs nothing here: terminal truth is a `response.completed` or
`response.incomplete` object and nothing else — not end of stream, not a sentinel, not the last
delta seen.

Credential acquisition, endpoint selection, retry, back-off and fallback are each their own
boundary and are absent.

## The pinned request subset

`project_request` writes exactly these fields, and `ingest_request` reads exactly these fields.
`ACCEPTED_BODY_FIELDS` is the list, and a body carrying anything else is refused with
`ErrorCode::Unsupported` rather than translated with the excess dropped.

| field | value |
| --- | --- |
| `model` | the binding's upstream model name |
| `input` | the standing instruction, then the conversation |
| `tools` | `{"type":"function", "name", "description", "parameters", "strict":false}` |
| `stream` | always `true`; this projection serves streaming responses only |
| `store` | always `false`; the conversation is the caller's, replayed whole every turn |
| `include` | `["reasoning.encrypted_content"]` |
| `max_output_tokens`, `temperature`, `top_p`, `reasoning.effort`, `tool_choice` | present only when the caller set them |

**Absent stays absent.** A sampling field nobody set is one the provider decides; writing its
default here would turn that decision into this crate's without anyone making it. `tool_choice` is
likewise omitted for `Auto`, because the model choosing is the route's own default.

The standing instruction goes at the **head of `input`** as a `developer` message, not in a
top-level `instructions` field. An empty instruction produces no entry at all, and a body carrying
a top-level `instructions` is outside the subset and refused.

### Items

| neutral item | wire entry |
| --- | --- |
| `UserText` | `message` / `user` / `input_text`, exactly one part |
| `AssistantText` | `message` / `assistant` / `output_text`, exactly one part |
| `ToolCall` | `function_call` with `arguments` as a JSON **string** |
| `ToolResult` | `function_call_output` with the envelope below |
| `Opaque` | outgoing only: the payload, verbatim, after all six coordinates match |

**Tool results are always enveloped**, as `{"ok": <bool>, "output": <value>}` serialized into the
single string this wire's `function_call_output` carries. The wire has no failure channel, so the
neutral `failed` flag has to travel inside the text. It is applied to successful results too: an
envelope used only for failures cannot be told apart from a successful result that happens to be
shaped like one, and a round trip that can flip `failed` corrupts the conversation rather than
translating it. A body whose `function_call_output` is not in this envelope is refused on ingress.

### Opaque state goes out and does not come back in

This is the one deliberate asymmetry in the contract, and it is not an oversight.

**Outgoing**, the caller holds an `Item::Opaque` that carries its six coordinates, they are checked
against the binding, and the payload is written verbatim. **On ingress there is nothing to check.**
The wire body carries no provenance, so reading an unmodelled `input` entry back as
`Item::Opaque` would mean this crate asserting an origin it never observed. It is worse than
merely unverified: it launders. A payload minted under one binding revision, replayed by a client
after the endpoint was repointed, would be stamped with the new revision and become sendable —
state `project_request` refuses turned by one pass through `ingest_request` into state
`project_request` sends. The invariant is that a mismatch is refused and never silently reused.

So ingress **refuses** an entry outside the four modelled shapes, with `Unsupported`. That is the
answer `docs/design.md` sanctions — "preserved or refused, never dropped to make translation
appear successful" — and it costs reasoning continuity through a gateway, which is real.

**The stream decoder still binds what it decodes, and that is a different act.** It watched this
binding produce the response it is reading; the attribution is observed, not minted. Those are the
only two places in this crate that construct an `Item::Opaque`.

Closing the gap properly needs something the neutral contract does not have: a state meaning
*carried, attribution unverified*, which a gateway could hold and a client could not spend.
`Item::Opaque` has exactly two possibilities today — bound to a binding, or absent. Adding a third
is a change to `llm-core` and to `docs/contract-v1.md`, which is a versioned contract; it is
recorded as a request rather than made here.

One further asymmetry, smaller: an opaque payload that is itself one of the four modelled shapes
is read back as that shape rather than as opaque.

### What is refused before anything is sent

| refusal | code |
| --- | --- |
| opaque state whose protocol, provider, account, endpoint, model or binding revision differs | `Unsupported` |
| a tool name outside `^[a-zA-Z0-9_-]+$`, **on either side** | `Unsupported` |
| a fixed field absent, or carrying any other value, on ingress | `Unsupported` |
| a message whose content is not exactly one part of the expected type | `Unsupported` |
| an unmodelled `input` entry, on ingress | `Unsupported` |
| a binding not declaring the Responses protocol | `Unsupported` |
| image or audio content, on either side | `Unsupported` |
| a request model that is not the binding's | `InvalidRequest` |
| everything `TurnRequest::validate` already refuses | its own |

The tool-name class is this provider's, verified against this provider: on 2026-08-23 a toolset
named `workspace.read` was answered `Invalid 'tools[0].name': string does not match pattern.
Expected a string that matches the pattern '^[a-zA-Z0-9_-]+$'`. It is refused here and not in
`llm-core`, because a neutral identifier enforcing one vendor's pattern would forbid a name a
later wire accepts. The `+` is read as written: an empty name is refused rather than passing
vacuously.

**Every pre-flight check egress applies, ingress applies too.** `project_request` applies exactly
five — the binding's protocol, `TurnRequest::validate`, the request model, the tool-name class and
the opaque coordinates — and each has its counterpart in `ingest_request`. A gateway that accepted
a request its own outgoing side then refused to forward would have agreed to something it cannot
do, and the caller would find out one hop later.

**The four fields this projection writes with a fixed value must arrive carrying exactly that
value.** They are `stream` (`true`), `store` (`false`), `include`
(`["reasoning.encrypted_content"]`) and each published tool's `strict` (`false`).

**Absence is a different value, not agreement.** On this wire an omitted `stream` is a
non-streaming request, an omitted `store` is a conversation the provider keeps, and an omitted
`include` asks for no encrypted reasoning content. The neutral turn has nowhere to record any of
those choices, so accepting the omission and re-projecting the pinned value answers a request
nobody made — the same reasoning this projection applies on the other side when it omits
`tool_choice` rather than sending `"auto"`, so that the provider's default stays the provider's.

Equality is over the whole value. `include: []` is refused for the same reason
`include: ["something_else"]` is: a membership test reads the empty array as agreement, because
`all` over no elements is true.

This is a real requirement on a client, not a formality: a body that does not carry all four is
refused. It is the price of one contract serving both directions, and it is loud rather than
silent.

## The pinned stream subset

`ACCEPTED_STREAM_EVENTS` names every discriminator the decoder interprets. Anything else is
**preserved as an opaque item and reported** as a `Warning{code: "unknown-stream-event"}` — a
dropped event is a hole in the conversation the next turn cannot see.

| event | effect |
| --- | --- |
| `response.output_text.delta` | `StreamEvent::TextDelta` |
| `response.reasoning_summary_text.delta` | `StreamEvent::ReasoningDelta` |
| `response.reasoning_text.delta` | `StreamEvent::ReasoningDelta` |
| `response.output_item.added` of a `function_call` | `StreamEvent::ToolCallStarted` with its `call_id` and `name`; one without a usable name is not announced, and its argument deltas are not relayed. A repeated opening item, or a second item under a `call_id` already announced, announces nothing. An outcome that does not carry each announced call under the same `call_id` and `name` is refused, with the terminal counters |
| `response.function_call_arguments.delta` | `StreamEvent::ToolArgumentsDelta`, named from `response.output_item.added` |
| `response.output_item.done` | one decoded item |
| `response.completed`, `response.incomplete` | terminal truth, decided by the **event name** as well as the object's own `status` |
| `response.failed`, `error` | a typed refusal |
| the remaining progress and `.done` markers | ignored; the terminal object is authoritative |

**Two servers speaking this protocol spell reasoning differently, and both are pinned.** `OpenAI`
streams `response.reasoning_summary_*`. vLLM v0.27.1 streams `response.reasoning_*`: a two-turn run
recorded as `verification-report:openai-responses-on-vllm` in `beyond10x/harness` produced 307
warning lines to roughly 30 real events, 301 of them `response.reasoning_text.delta`, and no
reasoning delta ever fired. A pin that knows one family makes a visibly working run look silent on
the other, and makes the machine-readable record unreadable.

### Counters

Each is independently optional and an **absent one stays absent**.

| neutral | wire |
| --- | --- |
| `input_tokens` | `usage.input_tokens` |
| `output_tokens` | `usage.output_tokens` |
| `cached_input_tokens` | `usage.input_tokens_details.cached_tokens` |
| `reasoning_output_tokens` | `usage.output_tokens_details.reasoning_tokens` |
| `cache_creation_input_tokens` | **always absent** — this wire reports no cache-write figure |

A zero for `cache_creation_input_tokens` would be this crate claiming, on the provider's behalf,
that nothing was written to its cache. Contradictory known counters refuse with `Protocol` rather
than saturating.

`upstream_model` and `response_id` come from the terminal object's own `model` and `id` and are
absent when it does not report them.

**Exactly one content part, in both directions.** `Item::UserText` holds a single string, so two
`input_text` parts joined into it come back out as one part and the client is answered with a body
it did not send; zero parts come back out as one empty part. Egress writes exactly one part, so
requiring exactly one on ingress is the symmetric rule rather than a new restriction.

An output **message** whose content carries a part outside `output_text`/`text` — a model
`refusal`, an image — is refused with `Unsupported`, not filtered down to the parts this version
understands. Filtering was the earlier shape and it turned a `refusal` part into an assistant
message with no text beside an `EndTurn`: the caller is told the model answered and shown nothing.
The refusal table above says `Unsupported` for content this version does not carry *on either
side*, and this is the other side.

### Terminal truth and failure

`final_usage` is true only for a terminal or failed response object: it says the reported counters
are terminal, not that every count is known. A stream that ends without one refuses with `Protocol`
and `Dispatch::Unknown`, carrying a bound observation with `final_usage: false`.

**Every refusal carries the evidence it had, whichever event raised it.** A model that runs out of
output tokens mid tool call is the ordinary way to reach one, and the attempt was still paid for.

A server announces a finished item with `response.output_item.done` **before** the terminal object
arrives, so a refusal raised while decoding a streamed item is **deferred** rather than returned
where it is raised: the stream is drained, the terminal object is read, and the refusal is
returned carrying that object's counters, model and response id. Returning on the spot discarded
the terminal object sitting later in the same stream, which made the rule above true only of
payload lists no server sends. Deferring does not soften the refusal — it is still returned, and
the events the caller already saw are still reported.

The same evidence attaches to a refusal raised while decoding the terminal object's own output. A
refusal raised before anything terminal arrived carries the binding alone; if a deferred refusal
and a missing terminal object coincide, the deferred one is returned, because it is the cause and
the other is its consequence.

One refusal deliberately carries **no counters**: when the reported ones are contradictory there
is no valid evidence to attach, and attaching invalid evidence is worse than attaching none. On a
successful decode that is a `Protocol` refusal in its own right. On a **provider failure** it is
not: the failure is classified from the provider's own code first and the evidence attaches
second, so a rate limit stays `RateLimited` and only the invalid counters are dropped — the
binding, the upstream model and the response id are still what was observed. Reporting a rate
limit as a non-retriable protocol error because its usage block disagreed with itself ends a run
on somebody else's temporary state.

`StreamDecoding` separates `events` from `result` so that a failure after partial output does not
erase the output a caller already saw.

A provider failure is classified from its machine-readable code alone:

| upstream code | neutral code |
| --- | --- |
| `server_error` | `Unavailable` |
| `rate_limit_exceeded` | `RateLimited` |
| anything else, or absent | `Refused` |

Only these two are widened beyond refusal, because only these two are unambiguously the far side's
own state. **The provider's message is never relayed.** Diagnostics are three fixed strings this
crate wrote; an upstream string relayed into a diagnostic is how a prompt or a credential
eventually reaches a log. `incomplete_details.reason` is treated the same way: it is kept when it
is shaped like a code (lowercase, digits, underscore, at most 64 bytes) and named `unrecognized`
otherwise.

## Four behaviours of the Harness adapter this crate does not reproduce

The shapes above are adapted from `beyond10x/harness` commit
`9e401e40b12c2a47b54257587dbed8c16b3e93d8`, `crates/harness-responses/src/{lib,project}.rs`. These
four are deliberate departures, each one a defect this repository exists to avoid.

| Harness | here |
| --- | --- |
| an absent reported model falls back to the configured one (`usage_from_response`) | absent stays absent |
| an absent `cached_tokens` becomes `0` | absent stays absent |
| a whole `usage` object is discarded when `input_tokens` or `output_tokens` is missing | every counter is independently optional |
| the provider's error message is formatted into the diagnostic | three fixed diagnostics |

One further difference is not a defect: Harness sends `prompt_cache_key`, a per-conversation cache
hint minted by its client. There is no session concept in this contract, and the same measurement
run found vLLM documents the field as accepted and ignored, so it is not sent.

## What this does not establish

Fixture evidence for a projection, not a client. Nothing here reaches a provider, and no paid call
runs in the ordinary gate. The vLLM measurements cited above were made against
`vllm/vllm-openai:v0.27.1` on different weights and a different host than any deployment; they are
claims about that server's HTTP surface and about nothing else.
