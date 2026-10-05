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

The route requires `max_tokens`. The neutral `max_output_tokens` stays optional, and an absent one
resolves to the binding's declared maximum — the only value in the system that is an operator's
statement rather than a guess. Temperature above `1.0` is refused: this route's range is not the
neutral one. A reasoning effort travels in `output_config.effort`.

**The projection places two prompt-cache breakpoints.** The caller replays the whole transcript on
every turn, so without them each turn pays the full input rate for everything it resends. A
non-empty instruction travels as one `system` text block marked `cache_control: {type: ephemeral}`,
which caches the constant head (tools, then system). A second, rolling marker goes on the last
`text` or `tool_result` block of the last message, so each turn writes the prefix the next one
reads back. Nothing else is marked: a replayed `thinking` or `redacted_thinking` block is sent byte
for byte, because its signature covers it as produced, and a tail with nothing markable carries no
rolling marker. An empty instruction sends no `system` at all; the rolling marker already covers
the tools.

Everything else is refused, not dropped: unknown request fields, image, document, search-result and
server-tool content, citations, cache-control on a block, a `none` tool choice, a system prompt that
is not one text block, and a role outside user/assistant. A translation that silently loses a field
is a translation that appeared to succeed. Ingress still refuses `cache_control` even though the
projection now sends it: a caller's own breakpoint has no neutral field to travel in, and dropping it
would hide a billing decision. So a body this projection sends is not itself valid ingress input.

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
accepted and changes nothing: its irrelevance is the route's own documentation. A `thinking` block
may open with `"signature": ""` or with no `signature` field at all, and its `signature_delta` signs
it either way; a finished thinking block without a non-empty signature is still refused.

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

One-hour cache creation and any nonzero server-tool use are outside this first pricing subset and
are refused rather than approximated. A five-minute breakdown that contradicts its own total is
refused.

**An accepted field this subset never reads still has to arrive in the shape the producer
documents.** Accepting a name is not the same as ignoring its value: a value in a shape the
producer does not send is not a field to pass over, it is evidence that the route is not the one
this projection was written against, and it is refused. That is a contract, not an accident of the
implementation, and the class it applies to is closed: exactly three names are accepted and never
read — `service_tier` and `inference_geo` on the usage object, and `stop_sequence` on a message and
on a message delta — each documented as a string, and each checked wherever it is accepted, because
the check travels with the accept list itself rather than sitting beside one use of it. Two other
accepted names, `citations` on a text block and `caller` on a tool call, are held to something
stricter still: any value at all is outside the declared subset and is refused outright.

**Fourteen of the eighty-five producer names this projection speaks are carried, not read.**
A name is not a behaviour — the behaviour around each is tested — but a wrong name is a field
silently never seen, or an accepted field refused, and no fixture written from the same wrong name
can notice. Sixty-seven were read from the extraction provenance the implementation
contract names, `harness-messages` at `709a2eb`; two of those — `anthropic-version` and the pinned
`2023-06-01` it carries — are sent on every single request and were invisible to this check until
its scan admitted a hyphen and a leading digit. Four more — `cache_control`, `ephemeral` and the
warning codes `unknown-stream-event` and `unknown-output-item` — were read from the same crate at
`2fd7235b` for the Harness parity rows. The remaining fourteen arrived with the carried
draft and nothing read in this session supports them: `auto`, `cache_creation`,
`ephemeral_1h_input_tokens`, `ephemeral_5m_input_tokens`, `caller`, `citations`, `inference_geo`,
`output_tokens_details`, `thinking_tokens`, `rate_limit_error`, `service_tier`, `timeout_error`,
`web_fetch_requests` and `web_search_requests`. They need one read of the producer's own types,
which belongs with the access story.

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

## Verification

[The verification record](verification/messages.md) states the counted behaviour and the mutation
evidence. The [ESS domain](../spec/domains/messages.yaml) declares only what the conformance adapter
observes through these public functions.
