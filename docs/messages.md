# The Anthropic Messages projection

`llm-messages` projects the neutral turn onto one documented route, `POST {base}/messages`, in
streaming mode, and decodes what that route answers. It is one codec used in both directions: the
outgoing projection a client sends, and the ingress a gateway accepts from a caller speaking
Messages. Authentication presentation and billing come from the binding, never from the protocol.

| Public item | What it does |
| --- | --- |
| `encode_request` | Projects a bound `TurnRequest` onto the wire body, refusing before any I/O |
| `decode_request` | Decodes an arriving Messages request into `IngressRequest` (neutral request plus the streaming choice) |
| `decode_message` | Decodes one complete response into a bound `TurnOutcome` |
| `StreamDecoder` / `decode_stream` | Decodes the event stream through the same response codec |
| `encode_ingress_message` | Encodes a neutral outcome as the non-streamed message a caller reads |
| `IngressStream` | Encodes neutral events and the outcome as the Messages event stream a caller reads |
| `MessagesClient` | One bound endpoint, one attempt, credential resolved per request |

## What the declared subset carries

Ordered user and assistant text, published tool names with object arguments, tool results with
their failure flag, signed thinking and redacted thinking. Thinking crosses as an opaque item bound
to all six binding coordinates: protocol, provider, account, endpoint, model and binding revision.
Opaque state from any other binding is refused rather than replayed.

**Ingress carries thinking unattributed, and never sends it on its own.** A request arriving at a
gateway carries no evidence of what served the reasoning inside it, so there is no binding to bind
it to. Stamping the *reading* binding onto it would launder: egress refuses that same payload when
it is bound to any other binding, so one `decode_request` / `encode_request` round trip would make
sendable state the projection otherwise rejects. So `decode_request` carries a signed or redacted
thinking block as `Item::UnattributedOpaque` — the block as a JSON value, protocol `messages`, no
binding — and `encode_request` refuses it with `Item::UNATTRIBUTED_REFUSAL` until the caller binds
it with `TurnRequest::bind_unattributed`. After that decision the block goes out JSON-equal to how it came
in — the same JSON value, not the same bytes: it is held as a `serde_json::Value`, so its keys
re-serialize sorted. The response half is unaffected: a stream or response this client read from its own bound
endpoint is attributable, and is attributed.

**A failed tool result is `is_error: true`.** The `tool_result` block carries the output as
`content`, a string as it is and any other value as compact JSON, and the neutral `failed` flag as
`is_error`. Ingress reads `is_error` back (absent is `false`) and the `content` string as the
output. A call the caller rejected before running it is sent the same way, `is_error: true` with
the reason as the caller's text: this wire has no rejected state, and ingress refuses any key
beside `type`, `tool_use_id`, `content` and `is_error`
([design](design.md#failed-and-rejected-tool-results)).

The route requires `max_tokens`. The neutral `max_output_tokens` stays optional, and an absent one
resolves to the binding's declared maximum — the only value in the system that is an operator's
statement rather than a guess. Temperature above `1.0` is refused: this route's range is not the
neutral one. A reasoning effort travels in `output_config.effort`, through `llm_core::wire_effort`,
which leaves every name unchanged on this wire, `max` included.

**What a coding-agent client sends is read under written rules** (`docs/design.md` "Client request
fields"). The first turn Claude Code sends, `tests/fixtures/claude-code-first-turn.json`, decodes
whole:

| Field | Ingress | Egress |
| --- | --- | --- |
| `thinking` | `{"type":"adaptive"}`, `{"type":"enabled","budget_tokens":n}` (n positive) or `{"type":"disabled"}` is `controls.thinking`; `budget_tokens` beside `adaptive` or `disabled`, and any other key (`display` included), refused by its path | re-emitted as read |
| `system` | a string, or text blocks joined by a line feed; an empty array is no instructions; a block of another type is refused (`Messages system block is not text`) | one block (one-way: the joined text comes back as one block) |
| `cache_control` | `{"type":"ephemeral"}` with an optional `ttl` of `5m` or `1h`, on a `system` block, a `text`, `tool_use` or `tool_result` block, or a tool, is the `prompt-cache` hint; `5m` is the default and reads as no lifetime; another key refused by its path, another type or lifetime refused; marks of two lifetimes in one request refused (`Messages cache marks carry more than one lifetime`); on a `thinking` or `redacted_thinking` block refused by its path | the breakpoints below, each with `ttl: 1h` when the hint says `1h`; the caller's mark positions are not reproduced |
| `output_config.effort` | any non-empty name, `max` included; `format` and any other key refused by its path | the same name |
| `tool_choice.disable_parallel_tool_use` | a boolean `v` on `auto`, `any` or `tool` is `controls.parallel_tool_calls = !v` | `disable_parallel_tool_use: !b`, writing `{"type":"auto"}` when the choice is automatic |
| `metadata` | an object holding only `user_id` (a string) is the `client-metadata` hint; another key refused by its path; the value is never kept | not sent (one-way), and reported omitted |

`omitted_advisories(&TurnRequest)` names the hints this projection does not send, in the design's
order: it carries only `prompt-cache`, so `prompt-cache-key`, `verbosity`, `reasoning-summary`,
`input-item-ids` and `client-metadata` are reported whenever present. `context_management`,
`safeguards`, `thinking.display`, `output_config.format` and every other key stay refused by their
path, and a `system` message inside `messages` stays a role outside the subset: the neutral turn
has no item that places an instruction between two turns.

**The projection places two prompt-cache breakpoints.** The caller replays the whole transcript on
every turn, so without them each turn pays the full input rate for everything it resends. A
non-empty instruction travels as one `system` text block marked `cache_control: {type: ephemeral}`,
which caches the constant head (tools, then system). A second, rolling marker goes on the last
`text` or `tool_result` block of the last message, so each turn writes the prefix the next one
reads back. Nothing else is marked: a replayed `thinking` or `redacted_thinking` block is sent byte
for byte, because its signature covers it as produced, and a tail with nothing markable carries no
rolling marker. An empty instruction sends no `system` at all; the rolling marker already covers
the tools. A turn whose `prompt-cache` hint has the lifetime `1h` marks both breakpoints
`{type: ephemeral, ttl: 1h}`; any other turn marks them as above.

**A subscription token changes `system`.** Under a `subscription-oauth` account, `system` opens
with the subscription client preamble ("You are Claude Code, Anthropic's official CLI for
Claude.") as its own block carrying only `type` and `text`, and the caller's instruction follows as
the marked block, so the breakpoint still covers the whole constant head. An empty instruction sends
the preamble alone, unmarked. The same account adds `anthropic-beta: oauth-2025-04-20` to the
headers and sends the token as `authorization: Bearer`. API-key and plain bearer accounts send
neither. Read back as ingress, such a body's two `system` blocks join into one instruction that
begins with the preamble.

Everything else is refused, not dropped: unknown request fields, image, document, search-result and
server-tool content, citations, a cache mark outside the shape above, a `none` tool choice, a
`system` block that is not text, and a role outside user/assistant. A translation that silently
loses a field is a translation that appeared to succeed. A body this projection sends is valid
ingress input: its breakpoints read back as the `prompt-cache` hint they were placed for.

A field outside the declared subset is refused by its path, in a request, a complete response or
a stream alike: `Messages field is outside the declared subset: <path>`, where the path starts at
the body or at the event type (`tools[].cache_control`, `message_start.message.usage.<name>`,
`content_block_delta.delta.<name>`). The field's name is copied only when it is at most 64 bytes of
`[A-Za-z0-9_-]`, and written `?` otherwise; its value never reaches the message.

An optional field spelled as an explicit `null` means absent, on both sides of the codec. The
producer writes `"stop_reason": null` for a message that has not stopped, and a caller may write
`"tool_choice": null` for a choice it is not making; one codec cannot read one spelling two ways.

## What the decoder requires of a stream

This route has **no `[DONE]` sentinel**: the terminal marker is a `message_stop` payload, so end of
input is the absence of an answer rather than the end of one, and a truncated stream is refused with
its last valid snapshot attached. A terminal message that names no stop reason is refused as well;
absence is not `end_turn`. A reason this projection does not model — `refusal`, `pause_turn`,
`stop_sequence` — is carried under its own name as an incomplete turn, never flattened into
completion.

The decoder refuses an event whose framed name contradicts its payload type, a `message_start`
whose message does not announce itself as an assistant message, a second `message_start`, content
before the message started, a delta for a block that never started, two blocks at one index, a
block still open at the terminal event, and a payload after it. Content is assembled in
content-block **index** order, so two blocks stopped in the opposite order to their starts do not
reverse the turn against the deltas the caller was already shown. A `ping` is
accepted and changes nothing: its irrelevance is the route's own documentation.

**The turn's text is the text the caller was shown.** A `text` block's text is its
`content_block_start` text followed by its `text_delta`s, and both are shown: text the block opens
with is a `StreamEvent::TextDelta` when the block opens, and is folded into the block, as a
`thinking_delta` is shown and folded. Nothing later replaces a block's text, so the turn's text
cannot differ from what was shown and `shown-text-kept` never appears on this route
(`docs/design.md` "Streamed events are final"). Every pinned fixture opens a text block with
`"text": ""`; the route's documentation does not say it always will.

A `thinking` block may open with `"signature": ""` or with no `signature` field at all, and its
`signature_delta` signs it either way; a finished thinking block without a non-empty signature is
still refused.

**An event, delta or content block this subset does not model is preserved, not refused.** A route
that adds an event type has not broken its stream, and ending the turn on it would refuse an answer
still arriving. The decoder keeps it whole as an `Item::Opaque` bound to the serving binding and
tells the caller with a `StreamEvent::Warning` whose text is fixed: `unknown-stream-event` for an
event or a delta (the whole event is kept; the block it names still assembles from the deltas
around it), `unknown-output-item` for a content block, warned about when it opens and kept at its
index. A kept event sits after every block that had started when it arrived, and kept events count
against the same in-flight content bound as blocks. A delta for a block that never started is still
refused: that contradicts the stream's own numbering rather than extending it. `decode_message` has
no stream to warn on, so a complete response still refuses an unknown content block, and egress
still refuses to send such an item back. Streamed tool arguments are bounded while they
accumulate and parsed once when the block closes, so half an argument object never reaches a caller.
A `tool_use` block is announced as `StreamEvent::ToolCallStarted` when it opens, with the `id` and
`name` of its `content_block_start` and before any of its argument fragments; a name the codec
would refuse at the end is refused there, before anything about the call is shown. A second block
under an `id` already announced is not announced again, and its fragments are not relayed.

`decode_stream` reads every byte it is given, which is why a payload after the terminal event is
refused there. `MessagesClient` stops reading at the terminal event, because the alternative is
holding a connection open to discover something it would refuse.

## Usage

Reported counters are **cumulative**, so a later report replaces the corresponding earlier one;
successive reports are never summed. A counter that goes backwards contradicts the earlier report
and is refused. An update is transactional: a report this subset cannot price leaves the previous
snapshot intact, and that snapshot is what a failure carries.

The route reports input tokens **disjointly** from its cache counters, while the neutral
`input_tokens` is inclusive. The two are reconciled only when every component is known; while any
component is unknown the inclusive total stays unknown rather than collapsing to the fresh count.
A counter the route never reported is absent. It is never zero, and a configured model name never
fills in an unreported serving model.

**The cache-write count is also known from its breakdown.** Anthropic's API reference
(`https://platform.claude.com/docs/en/api/messages.md`, read 2026-10-20) documents a message's
`usage` this way: "Total input tokens in a request is the summation of `input_tokens`,
`cache_creation_input_tokens`, and `cache_read_input_tokens`" (line 4119);
`cache_creation_input_tokens` is "number or null" (lines 4137-4141); and `cache_creation`,
"Breakdown of cached tokens by TTL", holds `ephemeral_1h_input_tokens` and
`ephemeral_5m_input_tokens`, each "default: 0" (lines 4121-4135). So with
`cache_creation_input_tokens` absent or `null` and a breakdown that reports both of its counts,
the write count is their sum, and the inclusive total is known when `input_tokens` and
`cache_read_input_tokens` are reported too. A breakdown that is absent, `null` or missing either
count leaves the write count and the total unknown: a documented default is not an observation. A
reported `cache_creation_input_tokens`, in the same event or a later one, outranks the breakdown;
a breakdown that contradicts a write count already reported is refused, and one that sums to less
than an earlier breakdown is refused as any counter that goes backwards is. In a stream the
documented place for the breakdown is `message_start`, because a `message_delta`'s usage
(`MessageDeltaUsage`, lines 4255-4300) has no `cache_creation`; one that arrives there anyway is
read the same way. No upstream is known to send the breakdown without the count. The one known to
omit the count, the Harness spike's Messages emulator, sends no breakdown either, so its total
stays unknown ([parity row M25](harness-parity.md#what-the-next-llm-release-changes-for-the-bridge)).

One-hour cache creation and any nonzero server-tool use are outside this first pricing subset and
are refused rather than approximated. A five-minute breakdown that contradicts its own total is
refused.

**An accepted field this subset never reads still has to arrive in the shape the producer
documents.** Accepting a name is not the same as ignoring its value: a value in a shape the
producer does not send is not a field to pass over, it is evidence that the route is not the one
this projection was written against, and it is refused. That is a contract, not an accident of the
implementation, and the class it applies to is closed: exactly three names are accepted as any
string and never read — `service_tier` and `inference_geo` on the usage object, and
`stop_sequence` on a message and on a message delta — each documented as a string, and each
checked wherever it is accepted, because the check travels with the accept list itself rather than
sitting beside one use of it. Two other
accepted names, `citations` on a text block and `caller` on a tool call, are held to something
stricter still: any value at all is outside the declared subset and is refused outright, except a
`caller` of `{"type":"direct"}` (below).

**A field the route leaves empty unless the request asked for a feature this projection never asks
for is accepted empty, and in no other value** (since 0.8.1). 0.8.0 refused a live streamed turn
with `Messages field is outside the declared subset: message_start.message.container`. Anthropic's
API reference (`https://platform.claude.com/docs/en/api/messages`, read 2026-10-20) documents each
of these on the response:

| Field | Where | Accepted | Why it is empty here |
| --- | --- | --- | --- |
| `container` | a message, `message_start.message`, `message_delta.delta` | `null`, or an object holding exactly `id` and `expires_at` (strings) and `skills` only as `null`; never consumed | "Information about the container used in this request. This will be non-null if a container tool (e.g. code execution) was used." This projection declares no container tool, so no later request of its own has a use for the id |
| `diagnostics` | a message | `null` | "`null` when the request did not supply `diagnostics`"; this projection never supplies it |
| `speed` | the usage object | `null` or `standard` | `fast` only when the request sets `speed`, which this projection never sends; fast output prices differently |
| `toolset_name` | a `tool_use` block | `null` | names the toolset of a toolset member; no tool this projection publishes is one |
| `caller` | a `tool_use` block | `null` or `{"type":"direct"}`, the documented default | the model called the tool itself, which is what a neutral tool call is; any other caller is a server tool and stays `unsupported` |
| `stop_details` | a message, `message_start.message`, `message_delta.delta` | `null` | "This is `null` when the `stop_reason` has no additional detail to report." The one field here the route fills without being asked, on a `refusal` stop; the neutral turn has no place for the detail, so such a turn is refused rather than carried without it |

Any other `container`, `diagnostics`, `speed`, `toolset_name` or `stop_details` value is refused as
`protocol` (`Messages accepted field has a type this route does not report`), never copied into the
message. The check travels with the accept lists like the one above, and the block reader is
shared with ingress, so a request's assistant `tool_use` block reads `caller` and `toolset_name` the
same way.

**Twenty-four of the hundred and six producer names this projection speaks are carried, not read.**
A name is not a behaviour — the behaviour around each is tested — but a wrong name is a field
silently never seen, or an accepted field refused, and no fixture written from the same wrong name
can notice. Sixty-seven were read from the extraction provenance the implementation
contract names, `harness-messages` at `709a2eb`; two of those — `anthropic-version` and the pinned
`2023-06-01` it carries — are sent on every single request and were invisible to this check until
its scan admitted a hyphen and a leading digit. Four more — `cache_control`, `ephemeral` and the
warning codes `unknown-stream-event` and `unknown-output-item` — were read from the same crate at
`2fd7235b` for the Harness parity rows. Nine — `container`, `diagnostics`, `direct`,
`expires_at`, `skills`, `speed`, `standard`, `stop_details` and `toolset_name` — were read from
Anthropic's API reference for 0.8.1 (the table above). The remaining fourteen arrived with the carried
draft and nothing read in this session supports them: `auto`, `cache_creation`,
`ephemeral_1h_input_tokens`, `ephemeral_5m_input_tokens`, `caller`, `citations`, `inference_geo`,
`output_tokens_details`, `thinking_tokens`, `rate_limit_error`, `service_tier`, `timeout_error`,
`web_fetch_requests` and `web_search_requests`. Ten more arrived with the client request fields
and are named by `docs/design.md` and the authored fixture rather than by the producer's types:
`1h`, `5m`, `adaptive`, `budget_tokens`, `disable_parallel_tool_use`, `disabled`, `enabled`,
`metadata`, `ttl` and `user_id`. They need one read of the producer's own types, which belongs
with the access story.

The split is not a paragraph to keep in step by hand: `tests/wire_names.rs` scans this crate's
source for every wire-shaped literal and fails on any name that is in neither list, and on any
list entry the source no longer speaks. Wire-shaped means `[a-z0-9_-]` with a lowercase or numeric
first character — a header name and a dated version string are both names this route speaks, and a
narrower scan would classify only the names it happened to select.

## The client

```rust
let client = MessagesClient::new(binding, HttpClient::new(Limits::default())?, resolver)?;
let outcome = client.turn(&request, &mut sink, &cancel).await?;
```

One attempt. The request is refused before a credential is resolved; the credential is resolved per
request through the injected resolver, so rotation needs no restart; the presentation is the
binding's (`x-api-key` or `Authorization`), never inferred from the protocol. Every request carries
`anthropic-version: 2023-06-01`, a constant rather than a setting. Diagnostics are fixed: a status
or a reported upstream failure contributes its category, never its response text.

A stream `error` of type `overloaded_error` (`unavailable`) or `api_error` (`transport`) is
retriable when no payload but `message_start` and `ping` came before it. After any other payload
it is final, a block that opened without showing anything included. Every other type is final,
`rate_limit_error` included, because on this route it may be a spend cap. A connection cut inside
the stream, inside an event or inside a chunk, counts output the same way: before any payload but
`message_start` and `ping` it keeps the transport's retry class, and after one it is final, as on
Responses (`story:messages-cut-after-silent-output-is-final`). Before the stream, a
429 whose error body says `rate_limit_error` with `error.details.error_code`
`enforced_spend_limit_reached` is that spend cap: a final `Refused` with dispatch `rejected`,
with or without a `retry-after` (`story:quota-429-is-final`). Every other 429 stays a retriable
`RateLimited`: a `rate_limit_error` without that code, with or without a `retry-after`, the 429 a
subscription token meets without the preamble, and a body that cannot be read within the
transport's bound. The provider lines this rests on are in
[design.md](design.md#provider-failures-retried-before-output).

A turn carrying hints this wire omits ends, once it has succeeded, with one `StreamEvent::Warning`
of code `advisory-omitted` naming them (`llm_core::advisory_warning`, wire name `Messages`). Never
before or during the turn, because a warning on the sink is visible output and ends fallback
eligibility; not at all after a failed turn or when nothing was omitted. A sink that refuses the
warning fails the turn with its own error.

**What bounds a turn, exactly.** A turn has three waits — credential resolution, the HTTP exchange
and a sink that is not accepting — and both things that end a turn from outside it reach all three.
Cancellation does, and so does one absolute instant taken when the turn starts.

Every client has that instant: `new` derives it from the transport's own `total` rather than asking
the caller to restate a value the two could then drift apart on, so the default bound is the one the
`HttpClient` was built with. `with_turn_limit` replaces it when a caller wants a shorter turn than
the transport's ceiling:

```rust
let client = MessagesClient::new(binding, HttpClient::new(limits)?, resolver)?
    .with_turn_limit(Duration::from_millis(300))?;
```

The instant is handed to each of the three waits, the HTTP exchange included: the transport takes it
as the caller's bound and keeps enforcing its own `response_headers`, `idle` and `total` alongside,
so the **shorter of the two** ends the exchange and a later instant can never lengthen it. A 300 ms
turn on a ten-second transport ends at 300 ms, whether it is waiting for a secret store, for
response headers, for the next event of a stream that stopped arriving, or for a caller that stopped
reading.

Retries, alternate accounts and fallback belong to routing. Gateway server composition and live
Anthropic API or subscription qualification are separate stories; nothing here establishes them.

## Answering a Messages caller

`encode_ingress_message(&TurnOutcome, id)` and `IngressStream` (`new`, `chunk`, `close`, `fail`)
answer a caller on this wire. Each value is one event payload carrying its `type`, which is also
the SSE `event:` name; framing is the server's. `IngressStream::new(id, model)` takes the response
identifier and, optionally, the model to announce in `message_start`, the only event that can carry
one; it refuses an identifier or model the decoder could not read before anything is sent. When a
model was announced, `close` refuses an outcome naming another; when none was, the outcome's model
stays off the wire and reads back as unknown.

A thinking block relayed live is signed at `close` from the outcome's opaque state, which only a
Messages upstream carries. A server whose upstream cannot sign reasoning (Chat Completions,
Responses) builds its stream with `IngressStream::without_reasoning_relay`: reasoning deltas then
send nothing, and signed thinking an outcome does carry is sent whole at `close`. A known input
total is left out when either cache count is unknown, because this wire reports fresh input apart
from both cache counters.

This crate's decoder is the contract: every block, terminal reason and usage report is checked
against the reader `decode_message` and `StreamDecoder` use before it is sent, and the tests decode
every answer and compare it with the encoded `TurnOutcome` for every stop reason, streamed and not.

- **Blocks.** Text becomes a `text` block with `text_delta`s, a call a `tool_use` block with
  `input_json_delta`s, reasoning a `thinking` block with `thinking_delta`s. A thinking block stays
  open until `close`, which sends the signature the outcome's opaque state holds and then stops it;
  every other block stops when the next opens. Reasoning with no signature, or signed for another
  binding, is refused. Content the stream never showed (redacted thinking, a call whose fragments
  arrived before it was announced) is sent whole at `close`, after the streamed blocks.
- **Reasons.** `end_turn`, `tool_use`, `max_tokens`, and an `Incomplete` reason under its own name;
  a reason the decoder would read as another one is refused.
- **Usage.** Only reported counters, in `message_delta`; an unknown counter is left out, never zero.
  Fresh input is sent only when the total and both cache counters are known, never derived from a
  part.
- **Failure.** `fail` emits the `error` event; eight error codes map to an Anthropic error type the
  decoder reads back as the same code, and `protocol`, `cancelled` and `unsupported` take the nearest
  (`api_error`, `api_error`, `invalid_request_error`). No wire field carries the gateway's retry
  decision, so a failure relayed inside a stream reads back by the receiving decoder's rules. With
  nothing streamed first, `overloaded_error` and `api_error` read back retriable, whatever the
  gateway decided. A failure before any output reaches the client as an HTTP status instead, and
  an error status carrying `x-should-retry: false` reads not retriable whatever its status, the
  header the official OpenAI and Anthropic SDKs already obey; llm-gateway has to send it. The
  header never makes a final status retriable
  ([design.md](design.md#provider-failures-retried-before-output), with the SDK lines;
  `story:relayed-failures-keep-their-retry-decision`).
- **Refusals.** `UserText`, `ToolResult` and `UnattributedOpaque` items, and an outcome that
  contradicts the streamed calls. Argument fragments for a call whose block already stopped make
  `close` refuse, and redacted thinking that would precede streamed text is refused because it
  would read back in another order.

## Verification

[The verification record](verification/messages.md) states the counted behaviour and the mutation
evidence. The [ESS domain](../spec/domains/messages.yaml) declares only what the conformance adapter
observes through these public functions.
