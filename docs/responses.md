# The Responses projection

`b10x-llm-responses` projects the neutral turn contract onto the `OpenAI` Responses protocol and
reads that same protocol back. The projection is pure: no I/O, no credential, no endpoint, no
retry. One contract serves both directions, so a gateway ingress surface and an outgoing client
disagree about nothing.

| function | direction |
| --- | --- |
| `project_request(&Binding, &TurnRequest) -> Result<Value, Error>` | neutral turn to wire body |
| `encode_request(&Binding, &TurnRequest, Option<&Conversation>) -> Result<Vec<u8>, Error>` | neutral turn to the exact body bytes the client sends |
| `request_headers(Option<&Conversation>, u64) -> Vec<(&str, String)>` | every non-secret header the client sets |
| `ingest_request(&Binding, &Value) -> Result<TurnRequest, Error>` | wire body to neutral turn |
| `decode_stream(&Binding, &[Value]) -> StreamDecoding` | wire stream to neutral outcome |
| `encode_ingress_response(&TurnOutcome, &str, u64) -> Result<Value, Error>` | neutral outcome to the non-streamed response object |
| `IngressStream::{new, chunk, close, fail}` | neutral events and outcome to the Responses event payloads a caller reads |
| `ResponsesClient::new(llm_providers::Binding, HttpClient, Arc<dyn SecretResolver>)` | one bound endpoint as an `llm_core::Model` |
| `ResponsesClient::with_conversation(Conversation)` | opts that client into one conversation |

## The client

`ResponsesClient` is the only part of this crate that performs I/O. One turn:

1. checks the request against the binding (`TurnRequest::validate_for`), encodes it with
   `encode_request`, the only place the body is serialised, and bounds the encoded body by
   `MAX_REQUEST_BYTES`, all before any I/O;
2. resolves the account's credential through the injected resolver, once, before anything is
   sent. A resolver failure is returned as `Unauthorized` or `Unavailable` with dispatch
   `not-sent`, and no connection is opened;
3. sends one streaming `POST {base_url}responses` through `llm-http`, with the headers
   `request_headers` lists and the authentication header the binding declares, and no other
   header. Without a conversation that is `accept` and `content-type` alone: no originator,
   session or account header ([Conversation identity](#conversation-identity)). A success with
   no `content-type` is read as the event stream the `accept` header asked for, which is how the
   Codex backend answers; a success naming any other media type is `Protocol` with dispatch
   `accepted`;
4. reads the stream up to its first terminal or failure event, decoding each payload as it
   arrives with the decoder behind `decode_stream`;
5. hands each payload's events to the sink before it reads the next payload, in wire order. A
   sink refusal, a cancel or the deadline during hand-over ends the turn there: nothing more is
   read, and the failure has dispatch `accepted` and the evidence decoded so far. Before the
   terminal object that is the binding alone, and counters that were never read stay absent;
   for the events deciding the turn produces (a warning for an unknown output item), it is the
   outcome's observation, or the decoder refusal's, with the counters the provider reported;
6. returns a decoder refusal with the binding attached when the decoder attached nothing, so every
   refusal after dispatch names the serving binding;
7. checks the outcome against the request (`TurnOutcome::validate_for`): a call to a tool other
   than a forced one is `Protocol` with dispatch `accepted` and the outcome's observation.

It makes one attempt: no retry, no credential refresh, no fallback, the same contract as the
Messages client. A connection that drops before the terminal object is `Transport` with dispatch
`accepted`; a body that ends cleanly without one is `Protocol`, as [below](#terminal-truth-and-failure).
The turn is bounded by the transport's `total` limit, credential resolution included. A refusal
raised before anything was sent carries no observation, so `Error::validate_for` accepts it.

A turn's stream events reach the caller's sink as each payload arrives, as the Messages client's
do. The transport's bounds (bytes per event, bytes and events per stream, the `total` limit) still
hold: a payload over a bound ends the turn after the events before it were shown. A cut after any
payload other than `response.created`, `response.in_progress`, `response.queued` or `keepalive`
is final, not retriable; a cut after nothing but those keeps the transport's retry class, since a
`keepalive` advances no turn. Routing's rule that an attempt which showed an event is never
retried sees the same events, and the client's rule also covers output that produced no visible
event. `decode_stream` reads nothing after the first terminal object either, so it reports the
events and the turn the client does.

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

`project_request` writes these fields, and `ingest_request` reads them and two more it never
writes, `instructions` and `client_metadata`. `ACCEPTED_BODY_FIELDS` is the list. Every object
ingress reads is closed: a key no rule names, at any level, is refused with
`ErrorCode::Unsupported` and `request body carries a field outside the pinned subset: <path>`,
the path written by `llm_core::field_path` (`input[].content[].foo`, `tools[].container`,
`tool_choice.foo`, `reasoning.generate_summary`, `text.format`), rather than translated with the
excess dropped.

| field | value |
| --- | --- |
| `model` | the binding's upstream model name |
| `input` | the standing instruction, then the conversation |
| `tools` | `{"type":"function", "name", "description", "parameters", "strict":false}` |
| `stream` | always `true`; this projection serves streaming responses only |
| `store` | always `false`; the conversation is the caller's, replayed whole every turn |
| `include` | `["reasoning.encrypted_content"]` |
| `max_output_tokens`, `temperature`, `top_p`, `tool_choice` | present only when the caller set them |
| `reasoning` | `effort` (from `llm_core::wire_effort`: `max` is sent as `xhigh`, disabled thinking as `none`) and `summary` (`auto`, `concise`, `detailed`), each only when set |
| `parallel_tool_calls` | the turn's control, only when set |
| `prompt_cache_key` | the turn's hint, or the conversation's identifier ([Conversation identity](#conversation-identity)) |
| `text` | `{"verbosity": "low" \| "medium" \| "high"}`, only when the hint is set |
| `instructions` | ingress only: the head of the instructions, below |
| `client_metadata` | ingress only: any object, the unforwarded hint `client-metadata`; its value is not kept |

**Absent stays absent.** A sampling field nobody set is one the provider decides; writing its
default here would turn that decision into this crate's without anyone making it. `tool_choice` is
likewise omitted for `Auto`, because the model choosing is the route's own default.

The standing instruction goes out at the **head of `input`** as one `developer` message, not in a
top-level `instructions` field. An empty instruction produces no entry at all. On ingress the
instructions are the top-level `instructions` (a string) followed by the `input_text` parts of
every developer message that opens `input`, in order, joined by a line feed; an empty string is
still a part. That is what Codex sends: an `instructions` string and a developer message of two
parts. The join is one way: the turn round-trips, the body comes back as one developer message. A
developer part of another type, a developer message with no parts, and a developer message after
the first item of another kind are refused: the neutral turn has no item that places an
instruction between two turns.

A tool of another type than `function` is refused. When its type is one of `web_search`,
`custom`, `namespace`, `tool_search`, `local_shell`, `file_search`, `computer_use_preview`, `mcp`,
`image_generation` or `code_interpreter` (`REFUSED_TOOL_TYPES`) the diagnostic names it, copied
from that list; any other type is refused without being named.

### Advisory hints

`prompt_cache_key`, `text.verbosity` and `reasoning.summary` are hints this wire sends.
`omitted_advisories(&TurnRequest)` lists, in the design's order, the hints a turn carries that it
does not send: `prompt-cache`, `input-item-ids` and `client-metadata`. `ResponsesClient` emits one
`advisory-omitted` warning naming them after a successful turn, never before or during it (a
sink event is visible output and ends fallback eligibility), and none when the turn failed or
nothing was omitted. A sink that refuses that warning fails the turn with its own error.

### Items

| neutral item | wire entry |
| --- | --- |
| `UserText` | `message` / `user` / `input_text`, exactly one part |
| `AssistantText` | `message` / `assistant` / `output_text`, exactly one part; an empty `annotations` array is accepted on ingress |
| `ToolCall` | `function_call` with `arguments` as a JSON **string** |
| `ToolResult` | `function_call_output` with the envelope below |
| `Opaque` | outgoing: the payload, verbatim, after all six coordinates match |
| `UnattributedOpaque` | ingress: an entry of a type in `CARRIED_ENTRY_TYPES` (today `reasoning`), carried JSON-equal; refused outgoing until a caller binds it |

**Tool results are always enveloped**, as `{"ok": <bool>, "output": <value>}` serialized into the
single string this wire's `function_call_output` carries. The wire has no failure channel, so the
neutral `failed` flag has to travel inside the text. It is applied to successful results too: an
envelope used only for failures cannot be told apart from a successful result that happens to be
shaped like one, and a round trip that can flip `failed` corrupts the conversation rather than
translating it. On ingress a `function_call_output` string that parses to exactly this envelope
(an object of exactly a boolean `ok` and an `output`) is read as one; any other string is a
successful result whose output is that string, which is how Codex sends a command's output. It
goes back out inside the envelope. An `output` that is not a string is refused. Ingress parses the
envelope at most 127 levels deep (`serde_json`'s recursion limit of 128), so an output nested more
than 126 levels deep, failed or not, reads back as a successful string carrying the envelope's
text.

A call the caller rejected before running it is a failed result like any other: `failed: true`
with the reason as the caller's text in `output`, sent as `{"ok": false, "output": <reason>}` and
read back as failed. This wire has no rejected state, and llm adds none: a third key in the
envelope would make a 0.7.0 reader take the whole string for a successful result
([design](design.md#failed-and-rejected-tool-results)).

Each modelled item names its keys: a message `type`, `id`, `role` and `content`; a content part
`type` and `text`; a `function_call` `type`, `id`, `call_id`, `name` and `arguments`; a
`function_call_output` `type`, `id`, `call_id` and `output`. An `id` (a string) is the unforwarded
hint `input-item-ids`: its value is not kept and egress writes no id. A `reasoning` item is carried
whole, its `id` inside the payload.

### Opaque state goes out bound and comes back in unattributed

This is the one deliberate asymmetry in the contract, and it is not an oversight.

**Outgoing**, the caller holds an `Item::Opaque` that carries its six coordinates, they are checked
against the binding, and the payload is written verbatim. **On ingress there is nothing to check.**
The wire body carries no provenance, so reading an unmodelled `input` entry back as
`Item::Opaque` would mean this crate asserting an origin it never observed. It is worse than
merely unverified: it launders. A payload minted under one binding revision, replayed by a client
after the endpoint was repointed, would be stamped with the new revision and become sendable —
state `project_request` refuses turned by one pass through `ingest_request` into state
`project_request` sends. The invariant is that a mismatch is refused and never silently reused.

So ingress **carries** continuation state the model mints — an entry whose type is in
`CARRIED_ENTRY_TYPES`, today only `reasoning`, the one type the stream decoder models as
continuation state — as `Item::UnattributedOpaque`: the entry as a JSON value, protocol
`responses`, and no binding. It is JSON-equal to what arrived, not byte-equal: it is held as a
`serde_json::Value`, so its keys re-serialize sorted. `project_request` refuses it with
`Item::UNATTRIBUTED_REFUSAL` until the caller binds it with `TurnRequest::bind_unattributed`;
after that decision the entry goes out JSON-equal to how it came in, and a binding other than the
one the caller chose still refuses it. This crate never makes that decision.

Everything else outside the four modelled shapes is refused by name with `Unsupported`, as it was
before unattributed state existed: an `item_reference` (a pointer to provider-stored state, which
`store: false` never creates), an entry whose `type` is absent or empty, and any client-authored
entry this version does not model, such as a hosted-tool `*_call_output`. Carrying the last would
let a bound copy skip the tool pairing and size checks a modelled entry gets.

**The stream decoder still binds what it decodes, and that is a different act.** It watched this
binding produce the response it is reading; the attribution is observed, not minted. It is the
only place in this crate that constructs an `Item::Opaque`.

One further asymmetry, smaller: an opaque payload that is itself one of the four modelled shapes
is read back as that shape rather than as opaque.

### What is refused before anything is sent

| refusal | code |
| --- | --- |
| opaque state whose protocol, provider, account, endpoint, model or binding revision differs | `Unsupported` |
| a tool name outside `^[a-zA-Z0-9_-]+$`, **on either side** | `Unsupported` |
| a fixed field absent, or carrying any other value, on ingress | `Unsupported` |
| a message whose content is not exactly one part of the expected type | `Unsupported` |
| unattributed opaque state no caller has bound, outgoing (`Item::UNATTRIBUTED_REFUSAL`) | `Unsupported` |
| an `input` entry whose `type` is absent or empty, on ingress | `Unsupported` |
| an `item_reference` entry, on ingress | `Unsupported` |
| an `input` entry of a type neither modelled nor in `CARRIED_ENTRY_TYPES`, on ingress | `Unsupported` |
| a binding not declaring the Responses protocol | `Unsupported` |
| image or audio content, on either side | `Unsupported` |
| a key outside the subset at any level, on ingress, named by its path | `Unsupported` |
| a tool of another type than `function`, on ingress | `Unsupported` |
| a developer message after the first item of another kind, on ingress | `Unsupported` |
| a thinking budget, or disabled thinking beside an effort, outgoing (`llm_core::wire_effort`) | `Unsupported` |
| a turn's `prompt_cache_key` that differs from the client's conversation, outgoing | `Unsupported` |
| a field of the wrong type (`instructions`, `parallel_tool_calls`, `prompt_cache_key`, `client_metadata`, `text`, an item `id`, a non-string tool result), on ingress | `InvalidRequest` |
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
the opaque coordinates — and each has its counterpart in `ingest_request`. The sixth,
`llm_core::wire_effort`, refuses only a thinking control, which this wire's ingress never reads. A gateway that accepted
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

**The turn's items come from the terminal object's `output` when it carries any.** When it carries
none — `output` absent, or an empty array after items were streamed through
`response.output_item.done`, as the Codex backend sends it — the streamed items are the turn.
An empty `output` with nothing streamed stays an empty turn, and a forced tool that was never
called is then refused by `TurnOutcome::validate_for`.

**The turn's text is the text the caller was shown.** Text shown through
`response.output_text.delta` belongs to the output item its `item_id` names; for a delta that names
no item, to the item at its `output_index`; for a delta that names neither, to the message whose
`response.output_item.done` comes next. Every delta is final (`docs/design.md` "Streamed events are
final"), so each such item's text in the turn is its deltas in arrival order, whatever the server
finished with.

Shown text is paired with exactly one message the turn carries, from the terminal `output` or the
streamed items, by the first of these that finds one: the same `id`; the message whose
`response.output_item.done` ended deltas that named neither coordinate; the `output_index` that
message's `response.output_item.done` named; the message's position in the terminal `output`; a
message that already carries exactly the shown text. A terminal message takes the coordinates of
the streamed message it is (the same `id`, or, when as many terminal messages as streamed ones are
left, the next in order), and its position counts only when it is no streamed message, because a
terminal `output` can hold items the stream never showed. What none of these pairs is paired by
elimination, with the one message no coordinate both sides name tells apart from it: two different
ids, two different indexes or two different streamed messages are two items.

The decoder never carries a message's text twice and never overwrites a message it cannot tell is
the one shown. A message no shown text is paired with keeps the server's text, with no warning.
Shown text that more than one message could be is carried in none: the server's messages stay as
sent and the caller is told with one `shown-text-unpaired` warning.

This is the one protocol where the turn's text can differ from what was shown, by two routes: a
terminal `output` whose message carries other text than its deltas, or an empty terminal `output`
after a `response.output_item.done` message that does. Either way the message takes the shown text,
and the client tells the caller with one `shown-text-kept` warning once the turn has passed its
checks against the request, before any `advisory-omitted` warning and before the turn is returned.
A turn the client refuses, a forced tool never called for example, carries none.
`shown-text-unpaired` follows the same rule. Both messages are fixed and quote no text.
`decode_stream`, which checks no request, ends its events with them. Whether a real server ever sends final text that differs from
its deltas is not known: no recorded Responses stream was found to check it against.

Shown text no message could be is kept as an assistant message of its own (row R36), with no
warning. Kept messages are placed by the `output_index` of their first delta, ascending and stable
among equal indexes, whatever order they arrived in; one without an index, or with one past the end,
goes last. A server that streams a message only as deltas and completes with an empty `output` would
otherwise return a turn without the answer the caller just watched arrive.

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
successful decode that is a `Protocol` refusal in its own right, and `ResponsesClient` returns it
with the binding alone attached. On a **provider failure** it is
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
| `server_is_overloaded` | `Unavailable` |
| `rate_limit_exceeded` | `RateLimited` |
| anything else, or absent | `Refused` |

Only these codes are widened beyond refusal, because they describe the far side's
own state, and each of the three also marks the failure retriable. The decoder sets that mark only
when nothing but `response.created`, `response.in_progress`, `response.queued` and `keepalive`
came before the failure. An opening item that emitted no visible event counts as output, so a
caller may retry the identical request only before output. Every other code is
final, an exhausted quota (`insufficient_quota`) or spend limit included. That holds inside a
stream and before it: a 429 whose error body names one of the five billing, spend or quota codes
(`insufficient_quota`, `credit_balance_exhausted`, `organization_spend_limit_exceeded`,
`project_spend_limit_exceeded`, `organization_usage_limit_exceeded`) as its `error.code`, or
`insufficient_quota`, `usage_limit_reached` or `usage_not_included` (the Codex backend's
subscription limits) as its `error.type`, is a final `Refused` with dispatch `rejected`, as the
official Codex CLI treats it. The client hands the transport that rule, and the
transport reads the 429's body for it only, bounded in bytes and time; every other 429, and one
whose body cannot be read in that bound, stays a retriable `RateLimited`
(`story:quota-429-is-final`). The provider lines this rests on are in
[design.md](design.md#provider-failures-retried-before-output). The client itself still makes one
attempt.
**The provider's message is never relayed.** Diagnostics are fixed strings this
crate wrote; an upstream string relayed into a diagnostic is how a prompt or a credential
eventually reaches a log. `incomplete_details.reason` is treated the same way: it is kept when it
is shaped like a code (lowercase, digits, underscore, at most 64 bytes) and named `unrecognized`
otherwise.

## Answering a Responses caller

`encode_ingress_response(&TurnOutcome, id, created_at)` and `IngressStream` (`new`, `chunk`,
`close`, `fail`) are the answer half of gateway ingress: they turn the neutral events and outcome
that answer an ingested request back into the payloads a Responses caller reads. Each value is one
server-sent-event payload carrying its `type`; the `event:`/`data:` framing, the response
identifier and the clock are the server's.

The encoder emits only what `decode_stream` interprets, in the order it requires, and the tests read
every answer back through it: the decoded events and `TurnOutcome` equal the encoded ones for every
stop reason, streamed and non-streamed (the non-streamed object wrapped as the terminal
`response.completed`). The opening `response.created` precedes the first chunk's own payloads, or
comes with `close` or `fail` when nothing was streamed. `close` ends with exactly one
`response.completed` or `response.incomplete`; `fail` ends with `response.failed`, whose code the
decoder reads back as the same class of failure, and never carries the error's own message. A
refused `close` sends nothing and leaves the stream open for `fail`.

No wire field carries the gateway's retry decision, so a failure relayed inside a stream reads
back by the receiving decoder's rules. With nothing streamed first, `server_is_overloaded`,
`server_error` and `rate_limit_exceeded` read back retriable, whatever the gateway decided. That
covers an `unavailable` it marked final and a failure whose retries it had spent. A failure
before any output reaches the client as an HTTP status instead, and there the gateway can state
its decision: an error status carrying `x-should-retry: false` reads not retriable whatever its
status, the header the official OpenAI and Anthropic SDKs already obey; llm-gateway has to send
it. The header never makes a final status retriable
([design.md](design.md#provider-failures-retried-before-output), with the SDK lines;
`story:relayed-failures-keep-their-retry-decision`).

What the encoder refuses rather than approximates or drops:

| Refused | Why |
| --- | --- |
| a stop reason the decoder would read back as another, such as Chat's `content-filter` | the caller would see a different reason |
| `UnattributedOpaque` state, opaque state of another binding, `UserText` and `ToolResult` items | the wire cannot carry them as the model's answer |
| an outcome contradicting what was streamed: an announced call missing, renamed or with other arguments, or text the caller was shown missing | the caller already holds the streamed version |

Usage carries only reported counters: an unknown counter is left out, never written as zero, and a
total comes only from two known parts. A reported cache-write count goes out as
`input_tokens_details.cache_creation_input_tokens`, as the Chat encoder does; this crate's decoder
does not read that field back, so such an outcome does not round-trip exactly. Reasoning streamed
without opaque state in the outcome adds no output item. `ingest_request` still refuses
`stream: false`, so the non-streamed encoder serves callers that accept a request by other means.

One identifier per answer: every payload, the terminal one included, carries the caller-supplied
`id`; the upstream's response id belongs to the upstream exchange and is not relayed, so the decoded
`response_id` is the caller's. `fail` carries counters only when the error's observation reports
them as final; a partial snapshot is left out rather than read back as a final cost. A text delta
after any other item opens a new message item, and each message ends with exactly the text its
deltas showed. Because `StreamEvent` carries no item boundary, two consecutive messages with
nothing streamed between them, and one message whose text a reasoning delta or a call interrupts,
are refused at `close` (story `ingress-block-boundaries`). The `fail` code mapping (which error codes
read back as themselves and which as `refused`) is a table in the module documentation of
`crates/llm-responses/src/ingress.rs`.

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

Two further differences are choices, not defects. Harness sends a successful string tool result
as plain text and a failure as `{"ok":false,"error":…}`; this crate sends every result in the one
envelope [above](#items), for the round-trip reason given there. And Harness always
sends a conversation's cache key and identity headers; here a route opts in (next section).

## Conversation identity

A client sends no conversation identity unless the caller opts in with
`ResponsesClient::with_conversation(Conversation::new(id))`, optionally
`.with_originator(name)`. Without it the request carries no `prompt_cache_key` and only
`accept`, `content-type` and the authentication header. That is what the Codex backend accepted
on the 2026-10-04 probe, and vLLM documents `prompt_cache_key` as accepted and ignored.

A client given a conversation serves that conversation only. Every request it sends carries:

| where | what |
| --- | --- |
| body | `prompt_cache_key`: the conversation's identifier |
| header | `originator`: the name the caller gave, only when it gave one |
| header | `session-id`: the identifier |
| header | `x-client-request-id`: the identifier, `-`, and the number of requests that client sent before this one, so each attempt is a new request id |

These are Harness's (`harness-responses/src/project.rs:360`, `lib.rs:289`-`297`). Harness keys the
cache on the conversation because a prefix digest measured `cached_tokens: 0` where the
conversation's id measured 85% cached.

**Ingress reads `prompt_cache_key` into the turn's hint**, and `project_request` writes the hint
back. A turn carrying its own key, sent with a conversation, must carry that conversation's
identifier: `encode_request` refuses a different one with `Unsupported` (`the turn's
prompt_cache_key differs from the client's conversation`) rather than letting one of the two
silently win. A turn without a key, sent with a conversation, gains the conversation's, so its
projection reports `request_preserved: false`.

## What this does not establish

Fixture evidence for a projection and a client exercised against local sockets
(`crates/llm-responses/tests/client.rs`). Nothing here reaches a provider, and no paid call
runs in the ordinary gate. The vLLM measurements cited above were made against
`vllm/vllm-openai:v0.27.1` on different weights and a different host than any deployment; they are
claims about that server's HTTP surface and about nothing else.
