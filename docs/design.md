# LLM foundation design

This is the target accepted by the operator on 2026-09-19, not a claim that the scaffold implements it.

## Ownership and sequence

Build the full LLM foundation before consumer migrations. Extract model-facing behavior from
Harness and the archived Platform inference component, and hosting behavior from llmgw, preserving
provenance. Depend on none of those consumers. Tool execution, approval envelopes, sandboxing,
delegation and agent-loop policy stay with Harness. Metaharness retains vendor process launch and
confinement. Retire llmgw only after a reversible gateway cutover.

## Neutral boundary

Core describes a model turn, tools as model-facing schemas/results, streaming, cancellation,
capabilities, usage and typed errors. It does not grant a tool permission to execute. Protocol
adapters implement Responses, Messages and Chat Completions with one documented
text/image/tool/streaming subset. Unsupported fields and provider-specific opaque continuation
state must be preserved or refused, never dropped to make translation appear successful. The one
exception is a Responses `response.metadata` stream event that llm 0.16.0 and earlier saved as
opaque state: it describes a response, is no conversation state, and the endpoint refuses it, so
egress leaves it out.

### Types that grow

`Item`, `Capabilities`, `Rejection` (`llm-routing`) and `AwsCredentialKind` (`llm-models`) are
`#[non_exhaustive]`, so a new item kind, declaration, rejection or AWS credential source is an
addition, not a break. Inside its own crate a match still names every variant. A match outside it
ends in a wildcard arm. On every path in this workspace that carries an item, that arm refuses an
item kind it does not know by its `Item::kind` name: such an item is never dropped and never
sent. `Capabilities` is built with `Capabilities::text` and its `with_*` setters, and its
documents keep their bytes.

### Images in a user turn

Accepted for wave 2026-11-05-w26 (llm 0.22.0, story:image-input). `Item::UserImage` holds an
`ImageSource`: inline bytes of a media type (`image/png`, `image/jpeg`, `image/gif`,
`image/webp`), or an `https` URL the far side fetches. A URL carries no media type: Responses,
Chat Completions and Messages carry a URL alone, and the far side reads the type of what it
fetches. A document writes the item as
`{"kind":"user-image","source":{"kind":"bytes","media_type":"image/png","data":"<base64>"}}` or
`{"kind":"user-image","source":{"kind":"url","url":"https://…"}}`; a turn without one keeps its
`llm.turn/3` or `llm.turn/4` bytes, and an older reader refuses the unknown kind.

- **Bounds, before I/O.** Inline bytes are 1 to `MAX_IMAGE_BYTES` (5 MiB) before encoding; a URL
  is `https://`, a host (no userinfo; a port is not a host) and visible ASCII, at most
  `MAX_IMAGE_URL_BYTES` (8 KiB). The whole encoded turn stays within `MAX_REQUEST_BYTES`.
- **Redaction.** A URL may carry a signed query and the bytes are the caller's content, so neither
  reaches a diagnostic or a `Debug` form: `Debug` shows the media type and the length, every
  refusal is fixed text, and a document reader refuses a malformed image without quoting it.
- **Placement.** Each wire sends the image in the user message of the user content beside it, in
  item order. Messages and Converse already put consecutive user items in one message. Responses
  and Chat Completions send one message per user text, so a run of user texts and images holding
  an image is one message of parts there; a run without one is sent as before.
- **Wires.** Responses `input_image` (`image_url` a base64 `data:` URL or the URL, `detail`
  `auto`); Chat Completions `image_url` (`image_url.url` the same, no `detail`); Messages `image`
  with a `base64` or `url` source; Converse `image` `{format, source: {bytes}}`. Converse has no
  URL source and refuses an image by URL by name. `InvokeModel` sends the Messages block.
- **Ingress.** Each decoder reads the image parts of a user message into image items, in order,
  so a message of text and image parts is no longer refused; Responses and Chat Completions then
  read every part of that message as its own item, which is the message egress writes back.
  `detail` other than `auto`, a provider file and an image in any other message are refused by
  their path.
- **Output and usage.** A model's output carrying a user image is caller-owned content and is
  refused. llm counts no image tokens itself: an image is in whatever input count the far side
  reports, and an absent counter stays unknown.
- **Selection.** A model may declare `image_input`; one declaring `false` is rejected for a turn
  holding an image as `image-input` ([Structured output and image input
  declarations](#structured-output-and-image-input-declarations)).

### Structured output and image input declarations

Accepted for wave 2026-11-05-w26 (llm 0.22.0, story:output-and-image-capabilities).
`Capabilities` carries two optional declarations, `structured_output` and `image_input`
(`Option<bool>`, set with `with_structured_output` and `with_image_input`). A turn needs
structured output when `controls.output_format` is set, and image input when it holds an
`Item::UserImage`.

| Declaration | Selection (`Catalog::explain`, `resolve`, `run_turn`) | `TurnRequest::validate_for` |
| --- | --- | --- |
| `false` | the target is rejected for a turn that needs the feature, as `structured-output` or `image-input`; fallback never moves the turn to it | refused before I/O: `binding does not take structured output`, `binding does not take image input` |
| absent | not rejected | not refused: the wire carries the turn or refuses it before I/O, as in 0.21.0 |
| `true` | not rejected | not refused |

Absent is "not declared", not "no": no document before the fields declared anything, and reading
their absence as `false` would reject every existing target for every such turn. So an undeclared
target keeps the 0.21.0 behaviour exactly: its wire sends the format or the image, or refuses the
turn by name before any I/O (Converse refuses an image by URL; Responses and Chat refuse a format
without a name). In an `llm.binding/1`, `llm.catalog/1` or `llm.routes/1` document both are
optional booleans (`null` is refused); absent is not written, so a document from before them
keeps its bytes, its binding revisions and its configuration digest. The built-in declarations
(`Capabilities::text`, a string route's `default_capabilities`, a Bedrock route's) leave both
undeclared: no provider's support is recorded in this repository.

### Streaming declarations

A caller, such as a graph deciding how to schedule a model node, reads two declarations from
`Model::capabilities()` (`llm_core::Capabilities`), beside the settings a route admits:

| Declaration | Means | Every client in this workspace |
| --- | --- | --- |
| `output_streaming` | the model's text reaches the sink incrementally, as it is generated, not only at the end | true: Responses, Messages and Chat Completions all request a streamed answer |
| `input_during_output` | input can reach a turn that is still producing output | false: no client here can send it ([Input into a running turn](#input-into-a-running-turn)) |

`Capabilities::text` declares both as the clients do. In an `llm.binding/1`, `llm.catalog/1` or
`llm.routes/1` document both are optional: absent means `output_streaming` true and
`input_during_output` false, and neither is written at that value. A document written before the
two fields therefore reads unchanged and serializes to the same bytes, so its binding revision and
configuration digest do not move and opaque state stored against it stays valid.

The rule depends on the protocol, so one check, `Capabilities::validate_protocol`, applies it
where a declaration meets one: `BindingDocument::bind` (and with it every catalog), and
`Routes::parse`, whose refusal names the route. A declaration of input during output is refused
(`no protocol in this workspace carries input during output`), and so is one of output that does
not stream (`every protocol in this workspace streams its output`): a document claiming it about
one of these clients would be untrue, and refusing it keeps every accepted document at the
defaults, so no digest moves and a 0.7.0 reader reads everything llm writes. Lifting either
refusal later breaks no document. `Capabilities::validate` checks neither, so a `Model`
implemented elsewhere keeps its own claim.

### Streamed events are final

Every `StreamEvent` is final: no later event retracts, replaces or reorders one already emitted,
and the vocabulary has no event that could. A caller may act on each event as it arrives, and one
that speaks text deltas aloud cannot unsay them. So on a successful turn each assistant message in
the `TurnOutcome` carries exactly the concatenation of the `TextDelta`s shown for it, and the
history a caller stores is the text it showed. A message no delta was shown for carries the text
the server sent.

Where a server's own final text for a message differs from the deltas it streamed, the decoder
keeps the shown text, and the client emits `StreamEvent::Warning` with the fixed code
`shown-text-kept` (`llm_core::SHOWN_TEXT_KEPT`) once the turn has passed its checks against the
request and before the outcome is returned, as it emits `advisory-omitted`: a warning about a turn
is for a turn the caller is handed, so a refused turn carries none. Its message is fixed and quotes
neither text.

A decoder never carries a message's text twice and never overwrites a message it cannot tell is the
one shown. Where it cannot pair shown text with exactly one message (deltas and messages that name
no coordinate telling them apart, and no message already carrying that text), it keeps the server's
messages as sent, carries the shown text in none, and emits a second fixed code,
`shown-text-unpaired` (`llm_core::SHOWN_TEXT_UNPAIRED`), under the same rule. A second code rather
than `shown-text-kept`, because no shown text was kept: the turn's text differs from what was shown
in the other direction, and a caller storing history needs to tell the two apart. Overwriting the
first candidate was the earlier shape and, with a message no delta showed ahead of the shown one,
it carried the answer twice and lost the first message.

The other answers to the first case, and why they were not taken:

| Option | Why not |
| --- | --- |
| The server's text outranks the deltas, plus a warning | The caller's history differs from what it spoke, and an ingress encoder relaying the stream refuses at `close`, because the outcome contradicts what it already streamed |
| A field on `TurnOutcome` | Changes `llm.outcome/4` and every place that builds an outcome, for a fact one warning carries; the relaying encoder still refuses |
| Refuse a turn whose text diverged | A turn the caller already spoke becomes an error |

Only Responses can replace a server's text ([responses.md](responses.md)). Messages shows a text
block's opening text as a delta ([messages.md](messages.md)), and Chat Completions carries text
only as deltas ([chat.md](chat.md)).

### Where one streamed item ends

Text deltas alone cannot say which message they belong to: a Chat Completions answer of text,
reasoning and more text is one message, and a Responses answer of two message items, or a Messages
answer of a text block, a call and a text block, is two. An ingress encoder that relays the stream
has to put each delta in the item the outcome carries it in, before it knows the outcome. Two
events say where an item ends without a delta saying so:

- `StreamEvent::MessageBoundary`: the next text delta belongs to another assistant message than
  the text delta before it. A decoder emits it only immediately before a text delta when text of
  another message of the turn was shown just before (Responses: another output item; Messages and
  Bedrock: another content block), so messages streamed one after another are separated by exactly
  one boundary. Without it, text deltas continue one message, whatever is shown between them, so
  Chat Completions never emits it. A Responses wire that interleaved the deltas of two items would
  give one boundary per change, and the text after a boundary would then continue a message shown
  before it; no server is known to interleave.
- `StreamEvent::OpaqueItem { provenance, payload }`: an item that arrived whole, with no delta to
  show it, in its place among the streamed ones, exactly as the turn carries it in `Item::Opaque`.
  The Messages decoder emits it for a `redacted_thinking` block when the block opens.

The boundary carries nothing and is not model output. Fallback eligibility does not count it
(`crates/llm-routing/src/fallback.rs`, `Counting`), the first-output clock does not start on it
(`crates/llm-calls/src/lib.rs`, `Timed`) and `VecSink` charges it to neither of its bounds, holding
at most as many boundaries as its event bound; because a decoder emits it only after shown text, it
never precedes a turn's first output either way.
`VecSink` counts an opaque item as one event and does not charge its payload to its byte bound,
which bounds shown text. The opaque item is model output the caller is
shown no text of, and ends eligibility and starts the clock as any delta does: a relay has already
sent it on.

The opaque item carries the block rather than marking where one sits, because a bare marker is not
enough on Messages: the redacted block's data arrives in its `content_block_start`, and the
Messages decoder (`crates/llm-messages/src/decode.rs`, `start_block`) refuses a block that starts
at an index below one already started. An encoder holding a marker could only send the block at
`close`, after the text that follows it, or hold back every later delta until `close`. Carrying it lets the encoder send it in
place as it arrives; `close` still checks that the outcome carries exactly that item there.

Both are new variants of an exhaustive `StreamEvent`, a breaking change for every consumer that
matches it. `StreamEvent` stays exhaustive rather than `#[non_exhaustive]`: a relay that does not
decide what an event means drops it silently, and that is how this gap went unseen; the compiler
making each consumer decide is the check.

### Input into a running turn

llm does not carry input into a turn that is still producing output. A caller with new input
cancels the running turn and starts a new one carrying what the first turn left and the new input.

Why it is not offered:

- **The port takes one fixed request.** `Model::turn(&self, request: &TurnRequest, sink, cancel)`
  (`crates/llm-core/src/port.rs:75-80`) receives the whole request when the turn starts. The only
  signal a caller sends while it runs is `cancel`, a `CancellationToken`
  (`crates/llm-core/src/lib.rs:27`).
- **The exchange llm speaks has no way in.** Each client encodes the whole request into one body
  before it sends (`crates/llm-responses/src/client.rs:172`,
  `crates/llm-messages/src/client.rs:123`, `crates/llm-chat/src/client.rs:64`), posts that finished
  `Vec<u8>` once (`crates/llm-http/src/transport.rs:202-210`), and from then on only reads the
  answer's server-sent events. Over that exchange none of Responses, Messages or Chat Completions
  takes input into a response that is still streaming.
- **Steering needs a transport llm does not implement.** OpenAI documents mid-turn steering for
  Responses: a `response.steer` event sent on a Responses WebSocket connection while a response
  runs, for the GPT-6 model family only (<https://developers.openai.com/api/docs/guides/steering>,
  read 2026-10-20). llm's Responses client speaks HTTP with server-sent events and does not
  implement WebSocket mode, so llm does not offer steering; offering it is the draft
  story:responses-websocket-steering. Anthropic documents nothing like it for Messages, and
  OpenAI nothing like it for Chat Completions.
- **So the declaration says no.** `Capabilities::input_during_output` is `false` on every binding
  (`Capabilities::text`, `crates/llm-core/src/vocabulary.rs:124-138`), and since 0.8.0
  `Capabilities::validate_protocol` refuses `true` for all three protocols
  (`crates/llm-core/src/vocabulary.rs:187-215`) in every binding, catalog and routes document
  ([Streaming declarations](#streaming-declarations)). A caller that schedules from the
  declaration reads `false` and uses the pattern below.

The supported pattern, cancel then continue:

1. **Keep what was shown.** Every `StreamEvent` is final
   ([Streamed events are final](#streamed-events-are-final)), so the `TextDelta`s the caller's sink
   accepted are what the model said, whatever happens to the turn afterwards. A delta is accepted
   when its `emit` completes, and only then is it recorded as shown (step 2 says why).
2. **Cancel when the input arrives.** Every client checks the `Cancel` at each read of the stream
   (`crates/llm-http/src/transport.rs:341-342` and `:363`). The Responses and Messages clients also
   race it against each `emit`, the cancel first, and drop an `emit` still in progress when it
   fires (`crates/llm-responses/src/client.rs:343-349`,
   `crates/llm-messages/src/decode.rs:803-809`): a sink that shows a delta across an `.await`
   records it only after that `.await` returns. The Chat Completions client hands over the events
   of the payload it last read before it reads again (`crates/llm-chat/src/client.rs:87-96`). The
   turn ends `cancelled` with dispatch `accepted` and returns no `TurnOutcome`. It is never
   retried (`Error::may_retry`, `crates/llm-core/src/error.rs:84-93`), and a routed run starts no
   further attempt (`crates/llm-routing/src/fallback.rs:413-415`).
3. **Start the next turn.** Its items are the conversation so far, then what the first turn left,
   then the new input as `Item::UserText`. What the first turn left depends on how it ended:
   - **Cancelled:** the shown text as one `Item::AssistantText`; nothing shown, no assistant item.
   - **Finished before the cancel reached it:** its outcome's items. Each assistant message in them
     carries the text deltas shown for it, and a message no delta was shown for carries the
     server's text; on Responses, shown text the decoder cannot pair with exactly one message is
     carried in none, and `shown-text-unpaired` says so
     ([Streamed events are final](#streamed-events-are-final)). A caller that must store exactly
     what it showed checks for that warning.
   - **Finished in tool calls:** its outcome's items, then a result for every call it proposed,
     because a request with an unanswered call is refused before it is sent (`every prior tool
     call needs a result before the next turn`, `crates/llm-core/src/turn.rs:156-160`). Run the
     tool and carry its result, or carry the call as rejected: a failed `Item::ToolResult` whose
     output is the caller's reason
     ([Failed and rejected tool results](#failed-and-rejected-tool-results)). The new input comes
     after the results.

Each client sends the request's whole conversation on every turn (Responses with `store: false`,
`crates/llm-responses/src/request.rs:154-155`; Messages, `crates/llm-messages/src/codec.rs:84`;
Chat Completions, `crates/llm-chat/src/outgoing.rs:124`), so the next turn needs nothing from the
first beyond what the caller kept. From a cancelled turn only text crosses: it returned no outcome,
so no tool call, reasoning or opaque state of it reaches the next request.

`crates/llm-docs/examples/interrupt_turn.rs` runs the pattern in-process. Its tests check each
branch: the second request carries the shown text as assistant text followed by the new input, a
finished turn's outcome in its place, no assistant item when nothing was shown, and a rejected
result for each call a finished turn proposed. The guide
[Interrupt a turn with new input](../website/docs/guides/interrupt-a-turn-with-new-input.md) quotes
it with its output.

### Failed and rejected tool results

A tool result (`Item::ToolResult`) succeeded or failed (`failed: true`). A call the caller rejected
before running it, refused by a policy or an approval or given a bad argument, is sent as a failed
result: `failed: true`, with the reason as the caller's own text in `output`. llm adds no wording
of its own. No wire in scope carries a separate rejected state: Messages has only `is_error`, and a
Responses `function_call_output` and a Chat Completions `tool` message carry text only. A caller
that needs to tell a rejected call from a failed one keeps that fact beside the item, in its own
loop's record; the model is sent the same failed result either way.

| Wire | A failed result is sent as | How ingress reads a result back |
| --- | --- | --- |
| Messages | a `tool_result` block with `is_error: true`, the output as `content` (a string as it is, any other value as compact JSON) | `is_error: true` is a failure; absent or `false` is a success. The output is the `content` string |
| Responses | a `function_call_output` whose `output` is the string `{"ok":false,"output":<output>}`; a success is the same envelope with `ok` true | a string that parses to exactly that envelope, an object of a boolean `ok` and an `output`, is a failure when `ok` is false and a success when it is true, either carrying the `output` value. Any other string is a success carrying that string |
| Chat Completions | a `tool` message whose content is `{"error":<output>,"ok":false}`; a success is its output as text (a string as it is, any other value as compact JSON) | content that parses to an object of exactly the keys `error` and `ok`, each once, `ok` being `false`, is a failure carrying the `error` value. Any other content is a success carrying it as a string |

Each ingress reads back as failed what its own encoder sends for a failure, within the bounds it
keeps on client input. The output comes back unchanged on Responses, which envelopes every result,
and for a failed Chat Completions result; Messages, and a successful Chat Completions result, send
an output that is not a string as its JSON text, which reads back as that string. Two bounds make
exceptions:

- Ingress parses the Responses envelope and the Chat Completions failure object at most 127 levels
  deep (`serde_json`'s recursion limit of 128). On Responses, which envelopes every result, an
  output nested more than 126 levels deep, failed or not, reads back as a successful string
  carrying the envelope's text; on Chat Completions a failed output nested that deep reads back as
  a successful string carrying the failure object's text. Messages carries `is_error` beside the
  text, so the flag survives any depth.
- Chat Completions parses a tool message only up to the tool-result bound (256 KiB). The failure
  object adds 21 bytes around the output's JSON, so a failed output whose JSON is within 21 bytes
  of the bound is sent and then refused by ingress as too large.

A successful Chat Completions result whose text is exactly the failure object reads back as failed:
the wire cannot tell the two apart.

A typed rejected state was considered and not taken
(`docs/plan/wave-2026-10-20-w09-design-review.md`, Layer 3): a key added to the Responses envelope
makes a 0.7.0 reader take it for a successful plain string, Messages ingress refuses a key it does
not know, and changing `Item` would break every consumer for nothing a model is sent.

## Executor

Decided 2026-10-20 (`story:async-clients-without-a-tokio-runtime`, option B). The asynchronous
clients run on Tokio and do not own a runtime.

Measured before the rule, at `a4835807`: each call below was polled by a standard-library executor
on a thread in no Tokio runtime (`crates/llm-models/tests/no_tokio_runtime.rs`,
`crates/llm-http/tests/no_tokio_runtime.rs`). Every one panicked with "there is no reactor
running, must be called from the context of a Tokio 1.x runtime"; none returned an error, and none
reached the endpoint.

| Call | Panicked at | What it was |
| --- | --- | --- |
| Responses turn, client or catalog port | `crates/llm-responses/src/client.rs:352` | `tokio::time::timeout_at`, the turn deadline around credential resolution; the resolver was not reached |
| Messages turn, client or catalog port | `crates/llm-messages/src/client.rs:220` | the same, the Messages turn deadline |
| Chat Completions turn, client or catalog port | `crates/llm-http/src/transport.rs:246` | `timeout_at`, the response-header deadline; the credential had already been resolved |
| Chat Completions turn with a file credential | `crates/llm-credentials/src/local.rs:70` | `tokio::task::spawn_blocking`, the file read |
| `HttpClient::post_sse` and `post_sse_until`; `post_json` | `transport.rs:246`; `transport.rs:529` | the response-header deadline |
| `SseStream::next` on a stream opened inside a runtime | `transport.rs:347` | the deadline of a read that waits for the wire |
| `reqwest` alone, `Client::post(…).send()` (a throwaway test, not kept) | tokio 1.53.1 `src/net/tcp/stream.rs:164` | `TcpStream::connect`: taking the timers out would not have been enough |

A turn or exchange started inside a runtime and polled on outside it, measured at `b1c1ddd1`, which
checked only a call's first poll and a stream read before it waited for the wire
(`crates/llm-models/tests/adversary_w11_no_runtime.rs`, `crates/llm-models/tests/no_tokio_runtime.rs`,
`crates/llm-http/tests/no_tokio_runtime.rs`):

| Moved out while | What it did |
| --- | --- |
| the caller's sink held an event back, Responses and Messages | panicked creating the next turn-deadline timer: `crates/llm-responses/src/client.rs:356` (`bounded`), `crates/llm-messages/src/decode.rs:812` (`emit`) |
| its credential resolution waited, then the resolver needed Tokio, every wire | panicked in the resolver |
| the request awaited its answer (`post_sse`, `post_json`), a stream read awaited a chunk, a quota rule awaited a 429's body | no panic: each waited on the runtime nobody drove; the cases gave up after two seconds |

The rule: the asynchronous clients and `llm-http` refuse, rather than panic or wait, when
polled on a thread in no Tokio runtime: `unsupported`, not retriable, with the fixed message "an
llm client needs a Tokio runtime; run the call inside one, or use llm-blocking for a single
model". The first poll of a turn or an exchange is refused before anything that needs Tokio runs,
credential resolution included, dispatch `not-sent` (`llm_http::require_tokio_runtime`). After
that every wait that runs on Tokio asks again on each poll (`llm_http::within_tokio_runtime`), so
a call moved out of its runtime part-way is refused at its next such poll with the dispatch it had
reached: `not-sent` while the credential is resolved, `unknown` while the answer is awaited,
`accepted` for a read of the response body and for an event handed to the sink under the Responses
or Messages turn deadline, and the 429's own `rejected` while a quota rule waits for its body.
An event already queued on an `SseStream` is still returned outside a runtime, and Chat
Completions hands its events to the sink without a timer; Responses and Messages hand an event to
the sink under the turn deadline, so that hand-over is refused outside a runtime. Both functions serve every client; a catalog port inherits them from the
client it wraps, and a routing run or `call_tool` from the turn it runs. The code is `unsupported`
because no retry serves the call in the caller's configuration, which `Error::may_retry` already
says of that code, and `llm-blocking` refuses the mirror case, a blocking turn inside a
runtime, with it. `unavailable` is a temporary state of the far side and is retried;
`invalid-request` is about the request. No `ErrorCode` was added.

The message names two ways out. A caller without a runtime runs the call inside one it starts, or
runs a single model through `llm-blocking`: `BlockingModel::new` owns a one-worker runtime,
`BlockingModel::on_runtime` borrows a multi-thread one, and either blocks the calling thread for
the turn. A routing run and `call_tool` are not a `Model`, so `BlockingModel` cannot run them; their
caller takes the first way.

The check asks only whether a runtime is entered on the calling thread. A runtime built without
its time and IO drivers passes it and still panics: `post_sse` inside
`Builder::new_current_thread().build()` panicked at the response-header deadline with "A Tokio
1.x context was found, but timers are disabled. Call `enable_time` on the runtime builder to
enable timers." (the throwaway test above, run before the rule, after its first form and after
this one). Tokio 1.53.1's `Handle` has no public method that reports whether a driver is enabled.
A wait started in one runtime and polled on inside another after the first was dropped passes
the check too. The one case first measured (a second throwaway test), a request awaiting its
answer, failed as a retriable `transport` error without a panic; a Responses turn whose credential
wait spans the drop panics instead ("A Tokio 1.x context was found, but it is being shutdown."), as
it did before this rule, because the old runtime's timer is polled. No caller is known to move a
turn between runtimes.

## Credentials and configuration

Inference accepts an injected SecretResolver trait. A SecretReference is an opaque lookup name,
not a secret value or mandatory storage backend. Optional OS keychain and explicit file adapters
serve local applications; remote compositions inject their own implementation. Resolve at request
time to permit rotation. Return redacted short-lived material and safe errors. Caller-owned
credential acquisition and renewal remain outside inference; an injected refresh facility may be
used without owning a login flow or modifying another program's credential file.

TOML names providers/accounts, endpoints, served models/capabilities, routes and fallback order,
prices and provisioning references. IDs are operator-defined; no built-in-name-only resolver.
Authentication kind is separate from protocol. OpenAI API/subscription and Anthropic
API/subscription each have their own compatibility evidence; subscription access must not silently
fall back to billable API credentials. The future Connectors arbitrary-secret adapter must not
require changing route references. Core never depends on Connectors.

The `llm.routes/1` document is the short form, in YAML or TOML with one digest for both. A Bedrock
endpoint (`bedrock.region`, `aws-keys`, `aws-container`, `aws-web-identity` or `aws-process`) has
no binding: it stays outside the expanded catalog, and `Routes::port` builds `llm-bedrock`'s
client for its routes. An `aws-web-identity` endpoint names only its kind; the caller supplies the
role, session name, STS endpoint and token file, as it supplies an `aws-container` endpoint's
URI and token file. An `aws-process` endpoint names a command that prints AWS `credential_process` keys; it
is the one credential source that starts a program, so a routes document alone never runs it:
`llm-models` builds its resolver only with feature `process` and only after the caller calls
`RouteResolvers::allow_credential_process`, and refuses its routes as `unauthorized` otherwise.
Route aliases are unique names for one route; a route's `price` applies to the model it declares,
and a turn is attributed to the model its provider reported, as `docs/pricing.md` requires: a turn
that reports another model or none has an unknown cost, never one at the route's configured rate.
A turn's cost stays unknown wherever a count or rate is. The `llm` command (`llm-cli`) reads
one such file and is a caller like any other: it injects the environment and file sources and the
container credentials endpoint from its own environment, and allows credential processes, for a
routes file that on Unix passes the owner and mode check a `file:` source gets; nothing else.

## Routing and accounting

Fallback is opt-in and ordered. Only defined failures before output becomes visible are eligible;
request capabilities, state compatibility, deadlines and remaining spending limits constrain every
alternative. Do not replay an ambiguously accepted request by assuming it was free. Record every
attempt and preserve unknown spend. No semantic downgrade, cross-account switch outside the named
chain, or fallback after an exposed partial stream.

One deliberate relaxation, matching Harness: a failure whose retry class is retriable (408, 429
unless it reports an exhausted quota, 5xx including 529, each unless the answer carries
`x-should-retry: false`; a request or body that failed in transit, a stream cut inside an event)
and
that put nothing on the caller's sink is retried on the same target before any output is
visible, and then falls back, even though its dispatch is `unknown` or `accepted`. It is not
assumed free: each attempt keeps its own dispatch evidence, a possibly billed attempt stays
`unknown` with its spend recorded, and the caller's limit decision runs before every retry as
before every first attempt. Unauthorized and refused failures, and anything after visible
output, are never retried. The contract is in [contract-v1.md](contract-v1.md#validation-and-accounting)
and `spec/domains/routing.yaml`.

Account for tokens, cache reads/writes, reasoning usage where reported, and provisioned compute.
Version price inputs; distinguish measured usage, estimates, subscription charges and unknowns.
Unknown is never zero. Define budget scope, reservations/concurrency, restart policy and uncertain
charges before claiming enforcement. An admission bound is not a promise about a provider's final
invoice. Product-level cheapest-model optimization is outside the first milestone.

### Provider failures retried before output

Decided 2026-10-20 (`story:retry-classes-harness-retried`). A provider can fail a turn inside a
stream it already opened with status 200, so the HTTP status table above never sees it. Each class
below is the far side reporting its own temporary state. Each is retriable while the attempt has
produced no output and final once it has, whether or not the caller was shown that output. Responses
`server_is_overloaded` has followed this rule since 0.3.1. The other four are the classes
Harness's wire crates retried, and they now follow it too:

| Protocol | Stream error | Reads as | Provider documentation, read 2026-10-20 |
| --- | --- | --- | --- |
| Responses | `server_is_overloaded` | `unavailable` | [OpenAI error codes][openai-errors], "503 - Model temporarily overloaded": "Follow the Retry-After header when it’s present, then retry your request." |
| Responses | `server_error` | `unavailable` | [OpenAI error codes][openai-errors], "500 - The server had an error while processing your request": "Retry your request after a brief wait and contact us if the issue persists." |
| Responses | `rate_limit_exceeded` | `rate-limited` | [OpenAI error codes][openai-errors], "429 - Rate limit reached for requests": "If a Retry-After header is present, wait at least as long as it specifies before trying again. If it’s missing, use exponential backoff with jitter and limit the number of retries." |
| Messages | `overloaded_error` | `unavailable` | [Anthropic errors][anthropic-errors]: "529 - overloaded_error: The API is temporarily overloaded.", and "The official SDK automatically retries transient failures (such as connection errors, rate limits, and 5xx server errors) with exponential backoff" |
| Messages | `api_error` | `transport` | [Anthropic errors][anthropic-errors]: "500 - api_error: An unexpected error has occurred internal to Anthropic's systems. Retry the request with exponential backoff; if the error persists, contact support with the request ID." |

OpenAI's guide names HTTP statuses, not stream codes. The [Responses reference][openai-responses]
lists `server_error` and `rate_limit_exceeded` among a failed response's `error.code` values;
pairing them with the 500 and 429 entries above is a reading.

Output is counted as the overload rule counts it. On Responses it is any payload but
`response.created`, `response.in_progress`, `response.queued` and `keepalive`. On Messages it is
any payload but `message_start` and `ping`. A block that opens empty shows nothing, but the
attempt has started answering, and a second attempt would bill again. The decoders set the mark:
`provider_failure` in `crates/llm-responses/src/stream.rs` and `stream_error` in
`crates/llm-messages/src/decode.rs`. The client still makes one attempt; routing retries on the
same target and falls back in the declared compatible order, as above.

One Messages failure is counted by what the turn holds instead
(`story:messages-empty-turn-with-an-unmodelled-stop-reason`). A turn that stops without content for
a reason the projection does not model is `unavailable`, and its terminal `message_delta`, which
follows no content block, is not output for it: the route produced no content, so the failure is
marked retriable. `model_context_window_exceeded` and `refusal` without content are `too-large` and
`refused`, and stay final. After a content block, visible or silent, there is no failure to
retry: the turn is the incomplete turn it was, and a block left open stays the final `protocol`
refusal (`crates/llm-messages/tests/streaming.rs::a_block_left_open_before_an_unmodelled_reason_is_still_the_protocol_refusal`).
The turn that fails this way shows the caller nothing: a `usage-extension` warning the route's
header or terminal delta would have caused is dropped with the attempt, as the header's is before
`overloaded_error`, so a route that reports `usage.cost` is retried like any other
([messages.md](messages.md#what-the-decoder-requires-of-a-stream)). Any other failure after that
delta is final and shows the held warning first
(`crates/llm-messages/tests/retry_class.rs::adversary_w30b_an_overload_after_a_delta_without_content_is_final_only_with_its_costs_shown`). The failure carries the
reported usage as final, because the answer ended.
`crates/llm-messages/tests/retry_class.rs` holds the other four sentences
(`a_stop_without_content_for_a_reason_in_no_row_is_retriable_before_output`,
`a_stop_at_the_context_window_or_as_a_refusal_without_content_is_final`,
`the_same_stops_after_silent_or_visible_output_are_an_incomplete_turn`,
`a_stop_without_content_on_a_route_that_reports_a_cost_shows_nothing_and_keeps_its_class`). The
retry is bounded: a route that answers every attempt that way is asked
`RetryPolicy::DEFAULT.max_attempts` times and the run ends
(`crates/llm-models/tests/relayed_failure.rs::a_messages_stop_without_content_for_a_reason_in_no_row_ends_after_the_policys_attempts`).
This is the rule of a streamed answer. A whole (unstreamed) Bedrock `InvokeModel` answer that
stops this way keeps its code and its usage and is final, as every failure of a whole answer is
([bedrock.md](bedrock.md#retry-classes)).

Bedrock Converse counts the same way (`story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason`).
Its decoder models `end_turn`, `tool_use` and `max_tokens`. An answer that stops for another
reason and holds no content block and no kept event is a failure with its final usage:
`too-large` for `model_context_window_exceeded`, `refused` with no cause named for
`guardrail_intervened` and `content_filtered`, and `unavailable` for every other reason, marked
retriable on a streamed answer because only `messageStop` and `metadata` follow `messageStart`:
the answer held no content block and no kept event, so the caller was shown nothing. No row
reads a counter: the failure carries the usage the answer reported as final, and that may name
output tokens, so each attempt of a caller that retries may be billed, as on the Messages wire
after a header that reported a cost
(`crates/llm-messages/tests/retry_class.rs::adversary_w30_a_stop_without_content_after_a_costed_header_is_retried_as_an_overloaded_error_is`).
After a content block, visible
or silent, a text block that carried no text included, there is no failure to retry: the turn
is the incomplete turn it was
(`crates/llm-bedrock/tests/client.rs::an_unlisted_stop_after_a_recorded_silent_text_block_is_not_marked_retriable`).
The rows differ from the Messages ones in their reason names only; each decoder holds its own
(`stopped_without_content` in `crates/llm-bedrock/src/converse.rs` and in
`crates/llm-messages/src/decode.rs`). A text block without text and then a stop both tables
name alike is the incomplete turn on both wires, streamed and whole
(`crates/llm-bedrock/tests/client.rs::a_stop_after_an_empty_text_block_reads_alike_on_both_wires`);
what the turn holds differs in one place, as it did before the rule: a text block that carried
no text is an empty text item in a Messages turn and no item in a Converse turn. What counts as
a content block on that wire, the tests and who retries a Converse turn are in
[bedrock.md](bedrock.md#a-converse-answer-that-stops-without-content).

A connection cut mid-stream follows the same count (`story:messages-cut-after-silent-output-is-final`).
The transport marks a cut retriable: a body that ends inside an event, or fails in transit. Each
client keeps that mark until its decoder has answered and drops it after, in its stream loop:
`ResponsesClient`, `MessagesClient` and `ChatClient`. On Chat Completions output is non-empty
`content`, `reasoning_content`, `reasoning` or `refusal`, any `tool_calls` fragment, or a finish
reason; the chunk that opens the stream, `role` with an empty or null `content`, answers nothing
(`story:chat-counts-every-output-delta`).

These stay final:

- **An exhausted quota or spend limit** reported on Responses (`insufficient_quota`,
  `credit_balance_exhausted`, `organization_spend_limit_exceeded`, `project_spend_limit_exceeded`,
  `organization_usage_limit_exceeded`) reads as `refused`. It is a 429, but not a rate limit.
  [OpenAI][openai-errors]: "Retrying billing, spend, or quota errors won’t restore API access."
- **A Messages `rate_limit_error`.** [Anthropic][anthropic-errors]: "A tier spend-cap 429 has no
  retry-after header and keeps failing until access resumes". A stream error carries no header
  that tells the two apart, and Harness's wire did not retry it either.
- **Every other code or type**, `billing_error` and `timeout_error` included, and a known code in
  another spelling.

**Before the stream opens** these classes arrive as an HTTP status, and the transport's status
table already marks it retriable: 429, and 500-599 with 529. That is `status_error` in
`crates/llm-http/src/transport.rs`, held by
`every_status_harness_maps_has_its_code_dispatch_and_retry_class` in
`crates/llm-http/tests/retry_classes.rs`.

**A 429 that reports an exhausted quota is final** (`story:quota-429-is-final`, decided
2026-10-20). It reads `refused`, dispatch `rejected`, not retriable: the class the same quota has
inside a Responses stream, so routing ends the run as it does there. Whether a quota should fall
back to another account instead is a separate decision, not taken. The transport knows no
provider's error document: each protocol client states its rule (`QuotaRule`,
`HttpClient::with_quota_rule`), and the transport reads a 429's body for it only, up to
`MAX_REFUSAL_BYTES` (16 KiB) and for at most `REFUSAL_WAIT` (1 s), within the exchange's own
deadline and the caller's cancellation. A body declared or found larger, one that has not ended
in time, and one that is not JSON leave the 429 a retriable `rate-limited`. The rule answers yes
or no; nothing it reads reaches a diagnostic, and a response tap is still shown the head alone.

| Protocol | A 429 is final when | Provider documentation, read 2026-10-20 |
| --- | --- | --- |
| Responses | its body's `error.code` is one of the five billing, spend and quota codes above, or its `error.type` is `insufficient_quota`, `usage_limit_reached` or `usage_not_included`, each exactly spelt | [OpenAI error codes][openai-errors]: "Retrying billing, spend, or quota errors won’t restore API access." The official Codex CLI at the commit cited below maps a 429 with either kind of name to a quota or usage-limit error ([`codex-api/src/api_bridge.rs:185`-`238`][codex-quota-429]) and gives each no retry delay ([`protocol/src/error.rs:410`-`437`][codex-terminal]); its test `map_api_error_distinguishes_http_quota_errors_from_rate_limits` keeps `{"type": "insufficient_quota"}` without a code apart from a rate limit ([`api_bridge_tests.rs:503`][codex-quota-test]) |
| Messages | its body's `error.type` is `rate_limit_error` and its `error.details.error_code` is `enforced_spend_limit_reached`, whatever its headers | [Anthropic rate limits][anthropic-spend-cap], "Reaching your spend cap": "On the Messages API, `error.details.error_code` is `enforced_spend_limit_reached`. Use it to tell this response apart from a rate limit." The same section: "Retrying, including the SDK's automatic retries, fails until access resumes." |
| Chat Completions | never: no rule is stated, and every 429 stays retriable | — |

The Messages rule reads the body field the page names, not the absence of `retry-after`. Both
pages also say the spend-cap 429 has no `retry-after` header, but the page's header table says of
it: "Not every 429 carries it; the spend-cap 429 never does", so a missing header does not tell a
spend cap from a rate limit. A `rate_limit_error` 429 without that code stays a retriable
`rate-limited` with or without a `retry-after`. That includes the 429 a subscription token meets
without the preamble (measured by Harness with no rate-limit headers,
`crates/llm-messages/tests/subscription.rs:308`), which keeps its retriable class. A spend-cap
body is final with or without a `retry-after`, whose hint is kept.

**A refusal names its cause when a structured code states one** (`story:context-length-refusal-is-typed`,
decided 2026-10-29). `Error::refusal` holds a `RefusalCause` from a closed set, read from an exact
code a protocol client names and never from prose; today the set is `context-length-exceeded`. The
cause changes no code, dispatch or retry class: such a 400 is the final `refused`, dispatch
`rejected`, that every 400 is, and routing halts on it as on any refusal. Its message is fixed text,
`HTTP endpoint returned status 400: the request does not fit the model's context window`. The
transport reads a 400's body for the client's `RefusalRule` (`HttpClient::with_refusal_rule`)
within the quota rule's bounds, `MAX_REFUSAL_BYTES` and `REFUSAL_WAIT`; a body it cannot read in
them, or one the rule names nothing for, leaves the 400 as `status_error` gives it.

| Protocol | Named cause when | Source |
| --- | --- | --- |
| Responses | `error.code` is `context_length_exceeded`, exactly, as a 400 or inside the stream | The official Codex CLI reads that code on a failed response as a context-window error ([`codex-api/src/sse/responses.rs:413`-`420`, `:703`-`705`][codex-context]) |
| Chat Completions | a 400's `error.code` is `context_length_exceeded`, exactly | The `OpenAI` error document the Responses rule reads; not re-read against provider documentation for this change |
| Messages | never | The wire states an over-long prompt only in an `invalid_request_error`'s message, prose this crate does not match |

**A relayed failure reads back by the receiving decoder's rules.** The ingress encoders write a
failure under the wire code that reads back as its class. No wire field carries the relaying
side's retry decision, so a downstream llm client applies its own rules. A Responses `fail` writes
`unavailable` as `server_is_overloaded` or `server_error`, and `rate-limited` as
`rate_limit_exceeded`. A Messages `fail` writes `unavailable` as `overloaded_error`, and
`transport`, `protocol` and `cancelled` as `api_error`. When the gateway streamed nothing before
the failure, a downstream llm client reads each of these back as retriable, whatever the gateway
decided. A Responses `fail` also writes a refusal naming `context-length-exceeded` as
`context_length_exceeded`, which reads back as the same final refusal with its cause; Messages
has no such code, so its `fail` relays a plain refusal.

Other clients decide for themselves, and they differ:

- Codex CLI classes a failed response's `server_error`, and every code it does not name, as
  `ApiError::Retryable` ([`codex-api/src/sse/responses_error.rs:98`-`101`][codex-errors]).
  `rate_limit_exceeded` and `server_is_overloaded` get variants of their own (`:87`-`97`).
- The official Python SDKs do not retry an error inside a 200 stream. Each decides a retry from the
  HTTP status and headers alone (`_should_retry` in [openai-python][openai-python-retry] and
  [anthropic-sdk-python][anthropic-python-retry]). Each raises an `error` event read from a
  200 stream to the caller ([openai-python][openai-python-stream],
  [anthropic-sdk-python][anthropic-python-stream]). Anthropic's errors page says such an error
  "doesn't follow these standard mechanisms" ([Anthropic errors][anthropic-errors]).

**A status that says not to retry is final** (`story:relayed-failures-keep-their-retry-decision`,
design option D, decided 2026-10-20). A failure before any output reaches a client as an HTTP
status, not as a stream: llm-gateway answers it 502 `upstream-failed`, which the status table
marks retriable. So without a signal llm's routing may call a gateway again after the gateway has
spent its retries on the same failure, and each such call can spend them again. The signal is
`x-should-retry`, which each official SDK reads in its retry decision ahead of its status rules
(408, 409, 429 and 5xx retried). Two of them decide one case before they read it, as the last
column says:

| SDK | Where it reads the header, at the commit read 2026-10-20 | Decided before the header |
| --- | --- | --- |
| openai-python | `_should_retry`, [`src/openai/_base_client.py:909`-`918`][openai-python-should-retry] | no retry when `Retry-After` exceeds its maximum delay ([`:900`-`907`][openai-python-retry-after]) |
| anthropic-sdk-python | `_should_retry`, [`src/anthropic/_base_client.py:930`-`939`][anthropic-python-should-retry] | nothing |
| openai-node | `shouldRetry`, [`src/client.ts:2076`-`2081`][openai-node-should-retry] | nothing |
| anthropic-sdk-typescript | `shouldRetry`, [`src/client.ts:1765`-`1770`][anthropic-ts-should-retry] | a 401 on a request that used its cached bearer token is retried once, after the cache is invalidated, whatever the header says ([`:1753`-`1763`][anthropic-ts-refresh]) |

Each SDK obeys `false` as "do not retry" and `true` as "retry", under the comment "If the server
explicitly says whether or not to retry, obey." llm reads `false` only. `status_error` marks an
answer carrying `x-should-retry: false` not retriable whatever its status, with its code, dispatch
and retry hint unchanged, so the Responses, Messages and Chat Completions clients all read it
final. Routing then does not retry the target, and a 502's dispatch `unknown` halts the run as
ambiguous instead of falling back. The signal only narrows: `x-should-retry: true` never makes a
final status retriable, so a 400 carrying it stays final. The name matches in any case. The value
must be exactly `false`, surrounding whitespace aside. A repeated field is no signal: combined as
RFC 9110 combines field lines, its value is `false, false`. llm-gateway has to send the header
when the failure it answers is final, for example once its own retries are spent; that is its own
story, in its own repository. Until it does, a gateway's 502 stays retriable here.

The failure path inside a stream is unchanged: what an ingress encoder's `fail` writes still reads
back by the receiving decoder's rules, as above.

[openai-errors]: https://developers.openai.com/api/docs/guides/error-codes
[openai-responses]: https://developers.openai.com/api/reference/resources/responses/methods/create
[anthropic-errors]: https://platform.claude.com/docs/en/api/errors
[anthropic-spend-cap]: https://platform.claude.com/docs/en/api/rate-limits#reaching-your-spend-cap
[codex-errors]: https://github.com/openai/codex/blob/8196bcdc189ad2c189d631b321fa54b8e09b9bb0/codex-rs/codex-api/src/sse/responses_error.rs#L87-L101
[codex-quota-429]: https://github.com/openai/codex/blob/8196bcdc189ad2c189d631b321fa54b8e09b9bb0/codex-rs/codex-api/src/api_bridge.rs#L185-L238
[codex-terminal]: https://github.com/openai/codex/blob/8196bcdc189ad2c189d631b321fa54b8e09b9bb0/codex-rs/protocol/src/error.rs#L410-L437
[codex-quota-test]: https://github.com/openai/codex/blob/8196bcdc189ad2c189d631b321fa54b8e09b9bb0/codex-rs/codex-api/src/api_bridge_tests.rs#L503
[codex-context]: https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/codex-api/src/sse/responses.rs#L413-L420
[openai-python-retry]: https://github.com/openai/openai-python/blob/8968560fb926240aea5fed8ee24e189eade57118/src/openai/_base_client.py#L899-L941
[openai-python-stream]: https://github.com/openai/openai-python/blob/8968560fb926240aea5fed8ee24e189eade57118/src/openai/_streaming.py#L78-L90
[anthropic-python-retry]: https://github.com/anthropics/anthropic-sdk-python/blob/bcd55fe1d3f0f1240765f8cef04295b67337f55a/src/anthropic/_base_client.py#L929-L962
[anthropic-python-stream]: https://github.com/anthropics/anthropic-sdk-python/blob/bcd55fe1d3f0f1240765f8cef04295b67337f55a/src/anthropic/_streaming.py#L160-L173
[openai-python-should-retry]: https://github.com/openai/openai-python/blob/0e5794dc48b8f8d51f6047f8a0b1814eb1e65850/src/openai/_base_client.py#L909-L918
[anthropic-python-should-retry]: https://github.com/anthropics/anthropic-sdk-python/blob/bcd55fe1d3f0f1240765f8cef04295b67337f55a/src/anthropic/_base_client.py#L930-L939
[openai-node-should-retry]: https://github.com/openai/openai-node/blob/e8c295c97159d56738d69aa18522f1acd4a1d093/src/client.ts#L2076-L2081
[anthropic-ts-should-retry]: https://github.com/anthropics/anthropic-sdk-typescript/blob/04a4c28c70de498d6d0e930e43edb5edb9ac545e/src/client.ts#L1765-L1770
[openai-python-retry-after]: https://github.com/openai/openai-python/blob/0e5794dc48b8f8d51f6047f8a0b1814eb1e65850/src/openai/_base_client.py#L900-L907
[anthropic-ts-refresh]: https://github.com/anthropics/anthropic-sdk-typescript/blob/04a4c28c70de498d6d0e930e43edb5edb9ac545e/src/client.ts#L1753-L1763

### Call records

`llm-calls` gives a routed caller one `CallRecord` per model call, on a `CallSink` it injects:
the route, the target that answered or failed last, the upstream model the far side reported, the
role (`primary`, `shadow`, `replay`) and kind (`turn`, `classification`, `verdict`,
`embed-batch`), the outcome, the error code and dispatch of a failure, the attempt count, the
start time, the latency, the time to the first event, the usage and the priced cost. llm keeps
no ledger of these records; the deployed ledger is llm-gateway's.

The record has no text field of any kind. Every field is an identifier, a closed set, a count, a
time or an exact amount, so no prompt, answer, reasoning, tool argument or error message can
reach it, and a failure is its code alone. Usage is the last attempt's report, and a counter it
did not report stays absent, never zero.

The cost and a failure's dispatch cover every attempt, because routing retries and falls back
after a retriable failure whatever its dispatch, `accepted` included. The cost prices each
attempt that may have cost something under the caller's `llm-cost` price book, matched by
serving model and binding revision, and adds them up; only an attempt that was not sent and
reported no usage is left out. Any unknown line, an earlier accepted attempt's unknown usage
included, leaves it `complete: false` with the known lines' subtotal as a lower bound. With no
price book, or no attempt to price, the record has no cost. A failed call's dispatch is the
furthest any attempt or the final failure reached (not sent, rejected, unknown, accepted), so a
limit refusal, deadline or cancellation after an accepted attempt does not record it as not
sent.

`record_turn` wraps `Catalog::run_turn` and returns its run unchanged. It reads the target, the
attempt count and the usage from `FallbackRun.attempts`, so routing needs nothing new. A counting
wrapper around the caller's stream sink marks the first event that is not a warning (a retry
warning is not output); latency runs from the start of the run to its return. Both are read from
a `Clock` the caller injects (wall-clock milliseconds and a monotonic reading), so tests script
time. `CallSink::record` returns nothing and cannot fail: a sink can never change what the
caller receives. A turn the run refuses before it starts is no call and writes no record; the
caller has the refusal. `CallRecord::new`, `Stopwatch` and `Cost::quote` build a record without a
turn, for the classification, verdict and embedding calls that later stories add. The contract
is `spec/domains/calls.yaml`.

A `Shadow` binds a primary route to a shadow route, with a daily cap in its `ShadowLedger`'s
currency and a reserve: the explicit amount each shadow call holds until its cost settles it,
above zero and at most the cap. `record_turn_shadowed` runs the primary call exactly as
`record_turn` does and returns its run with a `ShadowCalls` handle that has sent nothing. The
caller answers its own caller first and then drives `ShadowCalls::run` on any executor it
chooses (the handle is `Send`), or drops it, which sends and holds nothing (dropped mid-flight,
the shadow in flight stays held for its day); llm owns no runtime,
as `Ports::pause` owns no timer. Each shadow of the primary's route then gets the same request,
re-addressed to the shadow route's alias. Its events go to no sink and no admission hook of the
caller's sees it: its answer reaches only its own record, role `shadow`, id
`<primary id>/shadow/<shadow id>` (or `shadow-sha256-<hex>` of the two ids when that would pass
the 256-byte id bound, so a long primary id still gets its shadow recorded), `primary_record_id`
naming the primary's record. A primary that made no attempt (its caller's admission refused it,
or it was cancelled first) keeps its record and gets no shadow: there is nothing to compare, and
nothing is paid. A shadow failure, slowness or skip cannot change, delay or fail the primary,
whose result the caller already holds.

Admission reuses the `llm-cost` budget engine; there is no second ledger. A `ShadowLedger` keeps
one `BudgetEngine` per shadow and UTC day of the injected clock, its limit the shadow's daily
cap. Before the shadow call is sent, its reserve is reserved and started in one step under the
ledger's lock, so two shadows in flight never both take the last room under the cap. A refused
reservation (past the cap, too many unsettled, spend the day cannot know, or a day earlier than
one already admitted, which never opens again) sends nothing and writes a record `skipped`,
dispatch `not-sent`, no attempt, no latency and no cost; the caller's `ShadowResult` carries the
ledger's refusal as its cause. Afterwards the reservation is settled with the record's cost,
which covers every attempt because routing retries and falls back after accepted failures too.
A cost that is incomplete, absent for want of a price book, or quoted in another currency
settles nothing beyond its known part and stays held at its reservation, never settled as zero;
the engine then admits no further shadow that day. A call that costs more than its reserve is
settled at its cost, so the cap can be passed by at most the excess of the shadows already in
flight. A call counts against the day it was admitted on; the next UTC day opens a fresh engine.
The cap of `ShadowLedger::new` is per process and lost on restart: the engine is in memory, and
`llm-cost` documents it as no durable permission, so one ledger must serve every shadow of a
deployment for as long as it runs. With the `sqlite` feature, `ShadowLedger::durable(dir, ..)`
keeps each shadow's day on a `llm-cost` `SqliteLedger` at `<dir>/<shadow id>/<utc day>`: the
day's spend survives a restart, and a second ledger on the same directory is refused while the
first is alive, so its shadows are skipped and never sent. A day ledger that cannot be opened
(changed cap, damaged files, an earlier clock, or a later day already on disk) skips that shadow
for the day.

`replay` sends a recorded corpus to a candidate route and reports, case by case, whether the
candidate's answer is the recorded one (story:offline-replay-eval). The corpus is the caller's
document, `llm.corpus/1`: an ordered list of cases, each a turn request, a classification request
(label set, instructions, input) or a verdict request (rubric with its schema, subject), with its
id, the caller's input-token estimate, optionally the id of the record it was taken from, and the
recorded answer: the text, the label or the verdict. A caller that keeps texts assembles it from
its own store. llm writes no corpus and keeps no text of one: a run, a refusal and a record hold
identifiers, counts, codes and amounts.

`Corpus::parse` takes the document only whole, in one pass over its bytes. It refuses as
`invalid-request` a document that is cut or followed by more bytes, a member given twice at any
depth, another format version, a case kind the format does not have, a member `llm.corpus/1`
does not declare at that place, a case id given twice, a case id that holds `/`, a rubric id of
more than 177 bytes and a recorded label outside the case's label set; and as `too-large` a
document longer than 16 MiB, with more than 4096 cases or nested deeper than 64 levels. The
refusal names the member by its path (`cases[1].kind`) and a fixed reason. The path shows a
member by its name only where `llm.corpus/1` defines that name; every other member is `*`: one
the format does not define, and each one inside a turn document, a verdict schema or a recorded
verdict (`cases[0].rubric.schema.*.*`), so a refusal holds no name the writer of the corpus
chose. A label set and a rubric are read for their shape only: what `llm-decide` asks of them is
checked when the case is sent. A recorded verdict is taken as given and not held to its rubric's
schema; a case whose recorded verdict the schema does not hold reports `differs` on every run,
because a candidate's verdict is held to the schema. The digest a run reports is the lowercase
hex SHA-256 of the bytes as given, so the same cases with other white space or key order are
another corpus.

A `Replay` names the run, the candidate route by its id, a ledger, a currency, a money cap and a
reserve. `replay` sends the cases in the corpus's order, one at a time, through a `Candidate`:
the port that turns a case into calls on the candidate route. It is a port because the
classifier and the judge are in `llm-decide`, which depends on `llm-calls`; `llm_decide::Routed`
is the candidate `llm replay` and the conformance target use. It sends a turn case with
`record_turn`, a classification case with `ModelClassifier` on its JSON-schema path and a verdict
case with `judge`; a turn or a classification that fails is not tried again. An answer equal to
the recorded one counts `same`: a turn's assistant text and a classification's label byte for
byte, with confidences not compared, and a verdict as canonical JSON, so key order and white
space do not count and `1` and `1.0` differ. Any other answer counts `differs`, as does one the
candidate cannot use: no label of the set, or no verdict the rubric's schema holds. A call that
failed, a classification answered past its deadline (`deadline`) and a label set or rubric
`llm-decide` refuses (`invalid-request`, before any call) count `failed` with the error code,
never `differs`.

Within one run no two calls share a record id. A case's request id is `<run id>/<case id>`; it
is the id of a turn's record and of the record of a case not sent, a classification's record is
`<request id>/classify` and a verdict's `<request id>/<rubric id>`. When the longest of the ids a
case writes would pass the id bound of 256 bytes, the request id is `replay-sha256-<hex>` of the
run id and the case id instead, so no case the reader takes fails for the length of an id (the
bound of 177 bytes on a rubric id is what is left of 256 after that form and a `/`). The rule
that makes the ids distinct is the reader's: no case id holds `/`, so in a readable record id
the case id ends at the first `/` after the run id, and two digest forms differ as their case
ids do. A readable id could equal a digest one only for a run id that is `replay-sha256-` and
the SHA-256 of an input that holds that run id itself.

Money is exact and held on an `llm-cost` budget ledger of the run's own, whose limit is the cap.
Cap, reserve and spend are `Amount`s, decimals with nine fractional digits in the run's currency;
a price becomes spend only through the caller's price book, as for any call record. Before a
case is sent its reserve is reserved and started; afterwards it is settled with the priced cost
of the records the case wrote. A case is admitted while the spend so far plus the reserve is at
most the cap: equal to the cap it is sent, one nanounit past it it is not. The first case the
ledger refuses is not sent and writes one record with outcome `skipped`; the run stops, and that
case and each one after it count `not-sent` (`stopped` is `limit-exceeded`).

The cap bounds admission, not the spend. The reserve is the caller's upper bound for the cost of
one case: while no case costs more than it, the spend stays within the cap. A case that costs
more is settled at what it cost and can take the spend past the cap. The run then stops at once,
when that case is the last one too: no later case is sent and `stopped` is `limit-exceeded`, the
cause a further admission would be refused with. The spend of a run is therefore at most the
cap, less the reserve, plus the cost of the one case that passed the cap.

A cost that is not known (a call that reported no usage, a price the book does not hold) is not
settled as zero: the case's reserve stays held and the run stops at that case, whether or not a
case follows, with `stopped` `uncertain-spend`. `spent` is the ledger's exposure when the run
ended. With `stopped` absent or `limit-exceeded` it is what the run's calls cost. With `stopped`
`uncertain-spend` it is the cost of the cases before that one plus what the ledger holds for it,
its reserve (or the known part of its cost where that is more): a hold, not money known to be
spent. A call that was not sent and reported no usage cost nothing.

A run is refused as `invalid-request` before any case is sent for a reserve of zero or above the
cap, a candidate route id the catalog does not declare, and no price book in the run's currency;
an embed route is `unsupported`. Every record written during a run has role `replay` and the
case's `primary_record_id`. The `ReplayRun` carries the corpus digest, the counts of the four
results, the spend and one report per case in the corpus's order; two runs of one corpus over the
same answers report the same digest and counts. The run's own loops are linear in the corpus; the
budget engine reads every reservation of its ledger at each command, so the ledger's work grows
with the square of the cases sent, bounded by the 4096 cases a corpus holds.

`llm replay --route <id> --corpus <file> --cap <amount> --reserve <amount>` runs it from files:
the route's declared `price` is the price book and names the currency, and the summary is printed
as one line of JSON on stdout. It exits 0 when every case was sent and the spend is known and
within the cap, 3 when the run stopped (the summary is printed, and stderr says whether it
stopped at its cap, past it or at a case whose cost is not known, and how many cases were not
sent) and 2, with nothing sent, for a route without a price, a corpus the reader refuses or a
run `replay` refuses. It keeps no call record.

### Usage read from an answer's bytes

A relay that passes an answer through untouched, such as llm-gateway's, records what the answer
reported without decoding a turn. Each protocol crate exports `UsageReader` (`new`,
`feed(&[u8])`, `finish() -> ReportedUsage`) and `read_usage(&[u8])` for a whole body
(story:usage-reader, asked for by llm-gateway's `dependency-blocker:llm-usage-decoders`).
`llm_core::ReportedUsage` holds the model the answer named, a `Usage` and `final_usage`.

- One body, streamed or not, in any pieces. `llm_http::AnswerBody` decides from the first byte
  that is not white space: `{` is one JSON answer, read when the body ends; anything else is an
  event stream, framed by `SseDecoder` as the bytes arrive. Two pieces split at any byte, or one
  byte at a time, read the same as the whole body.
- One reading. The counters and the model come from the functions the turn decoder reads them
  with: Responses `usage` and `identifier` on each response object, Messages `Header` and its
  `Snapshot`, Chat Completions `read_reported` and `Reported::counters`. A counter means what it
  means in `Usage`: input includes cache read and cache write, so a Messages input total is
  known only when all three parts were reported.
- Unknown stays unknown. A counter the answer did not report is `None`; a report a decoder would
  refuse, or counters that contradict each other, are not taken, and the reader keeps what it
  had. Reading never fails: a cut or malformed answer reports what came before, with
  `final_usage` false until the terminal event (`response.completed`, `.incomplete` or
  `.failed`; `message_stop`; `[DONE]`) or the whole JSON body was read. A stream past
  `SseDecoder`'s bounds stops being read there.

### Typed decisions

`llm-decide` turns a routed model into a typed decision. It depends on `llm-core`,
`llm-routing` and `llm-calls` only: no protocol crate and no transport, so a decision runs over
any `Model` a caller's catalog binds. The contract is `spec/domains/decisions.yaml`.

**Classification.** A `LabelSet` is an id, a version and at least two distinct, non-empty labels
in the caller's order (at most 256, each at most 256 bytes); anything else is a typed
`SetupError` before any call. A `Classifier` returns exactly one of two things: a `Decision`,
one label of the set with every label's confidence in basis points (0 to 10000, unknown left
absent, never 0), its latency and the id of the call record; or an `Abstention` with its cause,
`Timeout`, `ProviderError` with the llm `ErrorCode`, or `OffLabel`. A label outside the set is
never a decision.

A model classifier asks for `{"label": <one of the labels>, "confidences": {<every label>:
<integer basis points>}}` with no other key, on one of two paths. `JsonSchema` sends that schema
as the turn's strict `controls.output_format` and takes the answer with the reader the judge
uses (`answer::whole`): the one assistant text of a turn that ended normally, read as one JSON
value. `ForcedTool` publishes one tool, `classify`, whose parameters are the same schema, forces
it with `ToolChoice::Named` and takes the arguments of the one call to it, of a turn that stopped
for its tool call (`answer::call`; `call_tool` of `llm-tool-call` sends one unrouted attempt and
so cannot be reused). An answer that is not whole on its path (the Verdict part below lists what
that is), does not decode as the schema, names no label of the set, or whose label's confidence
is under the classifier's `min_confidence_bps` abstains `OffLabel`; an unknown confidence meets
no floor. The caller owns the prompt (`with_instructions`) and the label set.

Every model call goes through `llm_calls::record_turn`, so it is routed with the route's retry and
fallback and writes one record, relabelled kind `classification`, to the caller's sink, with the
id `<request id>/<classifier id>`. The decision reads its answer from the run's final result and
its failure code from the run's final error; latency and cost are the record's, which cover every
attempt. A call the run refuses before any attempt abstains `ProviderError` and has no record.
The caller passes `Chain::classify` its input-token estimate, which routing admits each target
against as it does for `record_turn`; llm counts no tokens, and without an estimate routing finds
no compatible target, so each model classifier abstains `ProviderError` (`unsupported`).

**Chain.** A `Chain` holds one label set, one deadline and its classifiers in order (at least one,
distinct ids). It asks each in turn, telling it the time left on the recording clock, until one
decides; every earlier one left an abstention, and when all abstain the last abstention is
`final`; only the chain sets that flag. Every attempt is returned. `Classifier` is a public
trait, so the chain holds every result again to its set and the classifier's floor: a decision
naming a label outside the set, a confidence above 10000 or given twice, or one under the floor
abstains `OffLabel`. A model call still open at the deadline is cancelled through its own child
of the caller's `Cancel`, then awaited so its record is written, and abstains `Timeout`; an answer
that arrives at or after the deadline (compared as durations, not whole milliseconds) is not
used; routing is also given the deadline so no attempt or retry wait starts past it, and its
halting there is a `Timeout` too. A provider's own deadline error inside the chain's deadline is
a failed call: `ProviderError` with code `deadline`. A model classifier reached with no time
left abstains `Timeout` without a call. The deadline timer is the caller's `Pause`, the same port
routing waits on between retries, so no executor is assumed.

A `LexicalClassifier` wraps the caller's `Lexical` (`classify(label_set, input) ->
Option<Answer>`), synchronous and with no model call and no record; `None` abstains `OffLabel`,
and its answer is held to the set and the floor like a model's. Because it makes no call, it runs
even past the deadline, so a chain ending in one always ends in its answer or its abstention.

**Verdict.** A `Rubric` is an id, a version, a text, the JSON Schema of the verdict and a route
alias. Its digest is the lowercase hex sha256 of the text's length in bytes as eight big-endian
bytes, the text, and the schema as canonical JSON (object keys sorted at every depth, no
whitespace); the id, the version and the route do not enter it. `Rubric::new` refuses an empty
version, an empty text and a schema the library cannot hold an answer to (`RubricError`, codes
`empty-version`, `empty-text`, `unsupported-schema`). The checked keywords are `type`,
`properties`, `required`, `additionalProperties`, `items`, `enum`, `const`, `minimum`, `maximum`
and `anyOf`, with `title` and `description` as annotations; any other keyword refuses the rubric,
so no answer is held to less than its schema. llm takes no JSON Schema dependency for this. A
schema nests at most 64 levels below its root: a subschema is one level below the schema that
holds it, a `const` or `enum` literal starts at its schema's level, and a member of an array or
object inside the literal is one level below that array or object, the `enum` list being such an
array. `enum` and `const` compare numbers by value, `1` and `1.0` being one number, at every depth
of a literal; `minimum` and `maximum` compare exact values, so a bound holds for every integer
from the least `i64` to the greatest `u64` and for every double, with no conversion that rounds.
The rubric keeps the schema as it was given, with its keys put in canonical order and no number
written and read back, so the digest, `Rubric::schema` and the request carry the same bytes.

`judge(rubric, subject, request_id, input_tokens, ports)` makes one call. The request is the
rubric's text as the instructions, the subject as the one user item and the schema, in canonical
key order, as the strict `controls.output_format` named `verdict`, with temperature 0 where the
route's target declares temperature: its bytes are a function of rubric and subject, and the
request id, the clock and the answer do not enter them. The route must have exactly one target
and fallback off, otherwise the judge refuses `route-not-pinned` before any call, so the model
cannot change between runs. The call goes through `llm_calls::record_turn` with the same-target
retry of every routed call and writes one record, relabelled kind `verdict`, with the id
`<request id>/<rubric id>`; a failed call is its error and still has its record.

A `Verdict` names the rubric id, version and digest, the route target, the upstream model the far
side reported and the call record id. `verdict_json` is the answer as canonical JSON when it is
the one whole JSON value the model ended on and valid against the schema; otherwise it is absent
and `error_code` is `protocol`. The answer is taken as it arrived: no key is dropped, no value
coerced, no second call asked. An answer whose far side reports no upstream model is refused
`protocol`, because a verdict cannot pin what it was not told; Bedrock Converse reports none, so
a rubric routed there is refused.

One reader, `answer::whole`, serves the judge and the `JsonSchema` classifier. An answer is whole
and has one reading only when all of this holds; the judge refuses anything else and the
classifier abstains `OffLabel` on it, both with the call's record written as succeeded:

- the turn ended normally (`StopReason::EndTurn`): an answer cut off at the output limit or
  reported incomplete is not taken, even when what arrived is valid;
- the outcome holds exactly one assistant text item, and beside it only state the provider keeps
  (`Item::Opaque`): two text items are not joined, since `1` and `2` would read as `12`, and a
  refusal beside the text refuses the answer. An empty or whitespace-only text item counts as a
  text item: a whole answer beside one is two texts and is not taken. The Messages decoder emits
  one item for each text block, an empty block included, and the Responses decoder an empty item
  for an output message with no parts;
- the text is one JSON value with nothing but JSON whitespace around it;
- it nests at most 128 levels, an array or object being one level and a scalar none: 128 arrays
  or objects one inside another are read, and the 129th is refused where it opens. The library
  counts the levels itself and switches the JSON reader's own limit off (`serde_json` feature
  `unbounded_depth`), so the bound is this one number;
- no object gives a key twice, at any depth, however the key is escaped;
- every number written without a fraction or an exponent is an integer `i64` or `u64` holds, and
  is written back digit for digit; one outside that range is refused rather than rounded. The one
  exception is `-0`: JSON readers hold it as the double minus zero, so it is written `-0.0`, the
  same value.

A number written with a fraction or an exponent is the double nearest to it, as JSON readers have
it (RFC 8259, section 6), read with correct rounding (`serde_json` feature `float_roundtrip`, so
the result does not depend on which other crates are linked) and written with the fewest digits
that read back as the same double, in the form `serde_json` writes: `1E2` is written `100.0`,
`1e30` is written `1e+30`, and a number no double holds (`1e400`) is refused.

A judge call publishes no tool. An outcome that holds a tool call, or says it stopped for one, is
refused by the routed call itself: the judge returns that error, `protocol`, with the call's
record written as failed, and no verdict.

For a classifier the points on the turn's end, the one text item, the one JSON value and the
repeated key decide. A label set's schema nests two levels, so a repeated key is `label`,
`confidences` or a label inside `confidences`, and its integers run from 0 to 10000, inside both
bounds of the list. A replayed classification (`Routed::label`) is read by the same classifier,
so a recorded case answered cut, split or with a repeated key names no label and `differs`.

A `ForcedTool` classifier reads no text. It takes a call's arguments when the turn stopped for
its tool call (`StopReason::ToolCalls`) and the outcome holds one call to `classify` and no
refusal. A call of a turn cut off at the output limit, a call of a turn reported incomplete, two
calls, and a refusal beside the call each abstain `OffLabel`, with the call's record written as
succeeded, even when the arguments would decode as a decision. Assistant text beside the call,
and state the provider keeps (`Item::Opaque`, which is how reasoning arrives), are not part of the
answer: the call is taken. The arguments reach the classifier as a parsed value, so a key the
model gave twice in them is not seen here; that reading belongs to the protocol decoders
(`story:tool-arguments-with-a-repeated-key`). An outcome the routed call refuses is a failed
call, `ProviderError` with `protocol` and the record written as failed: a call to a tool other
than `classify`, a call of a turn that says it ended normally, a stop for a tool call with no
call, and two calls under one call id.

**Decision endpoint.** `llm-openai-decisions` is the client of OpenAI's `POST /v1/decisions` and
the classifier's third path, `Path::DecisionEndpoint`. The wire is read from
`openai/openai-openapi` at commit `b4cad10d7cff7fc286f2dbf1992569989522c6f0` (path `/decisions`,
operation `createDecision`); the reference names one model for it, `gpt-6-luna`. A decision
request is not a turn, so the client is not a `Model` and does not go through `record_turn`:
`OpenAiDecisions::select` picks the first compatible target of the turn route the request's
`model` names as its alias (admitted against the caller's input-token estimate, as a turn with no
tools and no controls is), and `decide` sends one request there, to the endpoint's base URL
followed by `decisions`, with the credential the target's account declares. The body carries the
target's upstream model in place of the alias. One attempt is made: nothing is retried and
nothing falls back. A target served over another protocol than `responses` or `chat-completions`
is refused `unsupported` before any I/O.

`llm-decide` still depends on `llm-core`, `llm-routing` and `llm-calls` only. It holds the port
`DecisionEndpoint` (`choose(ChoiceCall) -> ChoiceAnswer`) and `EndpointClassifier`, which asks it;
`llm-openai-decisions` depends on `llm-decide` and implements the port, so the chain takes any
other implementation as well.

A `DecisionRequest` holds an input text and 1 to 400 questions, each a predicate, a choice (2 to
255 choices with distinct values, a value being a text or a boolean, the text `"true"` and the
boolean `true` distinct) or a score (2 to 10 levels). `validate` holds it to the reference's
bounds before any I/O and refuses `invalid-request` naming the path: 0 and 401 questions, 1 and
256 choices, 1 and 11 levels, a repeated choice value, an input over 10485760 characters, a name,
instructions, value, description or label over 1048576, a `safety_identifier` over 128 and a
`prompt_cache_key` over 64. Lengths count characters (Unicode scalar values), not bytes.
`DecisionRequest::from_json` reads a request from one whole JSON document and refuses a member
the reference does not name, at every level, by its path (the name is written only when it is 1
to 64 bytes of ASCII letters, digits, `_` and `-`), a missing or mistyped member, and the
reference's forms this client does not send (an input given as a list of items, instructions
given as a list of parts) as `unsupported`. The encoded body is also held to the transport's
request bound of 16 MiB (16777216 bytes is sent, one byte more is refused `too-large`), which a
request inside every bound above can exceed.

An answer body is taken only whole. The client reads the transport's stream to its end (the
transport bounds a body at 32 MiB and refuses an answer that does not name `application/json`),
and only then reads the bytes as one JSON document with nothing but JSON whitespace around it. A
body the transport reports as cut (short of the `content-length` it declared, or a chunked body
without its end) is a failure of `llm-http`, handed on unchanged as for every other client in
this workspace: `transport`, dispatch `accepted`, retriable. A body that arrived whole by the
transport's account and is not one JSON document is `protocol` with dispatch `accepted` and not
retriable: a document that ends early (which includes a body delimited only by the connection's
end and cut there), bytes or a second value after the document, a member given twice in one
object at any depth and however its name is escaped, and a nesting deeper than 16 (an array or
object being one level: 16 one inside another are read, the 17th is refused). A member the
reference does not name is part of the document, so these rules hold for it too, but its
presence refuses nothing and its value is not interpreted: a later version of the endpoint that
adds a member still answers. The body must name
its `model` and hold exactly one answer per question, in question order: each of its question's
type or a `refusal`, each with the member `name` present and equal to its question's name (`null`
for a question without one), and each with the members the reference requires for its type. An
answer of any other type, of another type than its question, or of another name is refused
`protocol` by its path, such as `answers[0].type`. A refusal message is a fixed text and a path;
it never carries a value of the body.

Usage is read apart from the answers, so a body refused for an answer still reports what it
counted. A counter is taken only when the body writes it as a whole number from 0 to 2^64 - 1
without a fraction or an exponent; absent, `null`, negative, fractional, of another type or
larger, it is unknown, never zero. The reference bounds each counter to -2147483648 to
2147483647 (`ResponseUsageResource`); that bound is not enforced: a whole number in `u64` is a
count, so 2147483648 is taken as written, and a negative counter is unknown, not an error.
Counters that contradict each other (cached and cache-written input together more than the
input, more reasoning output than output: `Usage::validate`) are all unknown. `total_tokens` is
not read.

`decide_recorded` writes one record of kind `classification` per call that reached its
credential: the route, the target, the upstream model the body named, one attempt, the usage as
read and, under a price book, its cost through `Cost::of_attempts`, as `llm-embed` prices a
batch. An unknown counter leaves its line unknown and the cost incomplete. A call refused before
that point (an invalid request, another protocol, a body over the bound, a `Cancel` already
cancelled) writes no record; a failed or cancelled call that got further writes one with its
error code and dispatch.

`EndpointClassifier` sends the label set as one unnamed choice question, the labels as text
values in the set's order, with the caller's instructions, and holds the answer to the set
(`LabelSet::decode_choice`). The chosen value is the label; a refusal answer, a chosen value that
is no label of the set, a boolean value, a probability for a value outside the set, and one label
given two probabilities abstain `OffLabel`. A label set of 256 labels is past the endpoint's 255
choices: the request is refused before any I/O and the classifier abstains `ProviderError`
(`invalid-request`) with no record. Each probability becomes basis points by one rule
(`basis_points`): it must be finite and from 0 to 1, minus zero being 0, otherwise the answer
abstains `OffLabel`; the result is the whole number nearest to the probability times 10000,
computed exactly on the double with integer arithmetic, and a value exactly halfway rounds up
(`0.03125` is 313). The probabilities are not normalized and their sum is not checked, so the
basis points of one answer can sum to more or less than 10000 (`0.03125`, `0.34875` and `0.62`
give 313, 3488 and 6200). A label the answer gives no probability keeps an unknown confidence
and meets no floor; the answer's own `confidence` member is not used. The classifier's floor,
the chain's deadline and its second check of every result apply as for a model classifier: a
call still open at the deadline is cancelled, awaited so its record is written, and abstains
`Timeout`. The record id is `<request id>/<classifier id>`, and a decision or abstention names
it only when that record reached the sink.

### Embeddings

An embedding is not a turn, so it has its own port and its own route kind. The contract is
`spec/domains/embeddings.yaml`.

**Route kind.** `llm.catalog.Route` has an optional `kind`, `turn` or `embed`. Absent is `turn`,
and a turn route does not write it, so a catalog from before the field keeps its bytes, revision
and digest. Selection refuses a turn on an embed route (`Catalog::explain`, `resolve` and
`run_turn`) and an embed request on a turn route (`Catalog::resolve_embed`) as `unsupported`,
before any port is bound or credential resolved. An embed request goes to the route's first
target only: vectors of two models are not comparable, so there is no fallback, and a catalog
declaring `fallback_enabled` on an embed route is refused. A serving model may declare
`embed_batch_limit` in its capabilities, the most texts one request to it carries; absent sends
every text in one request and is not written, and zero is refused.

**Port.** `llm_core::Embedder::embed(&EmbedRequest, &Cancel)` takes texts, optional
`dimensions` and a timeout, and returns `Embeddings`: one `f32` vector per text in input order,
all of one length, the input tokens in `Usage`, and the model the far side reported. `llm-core`
holds the port and no transport. A request with no text, an empty text, more than
`MAX_EMBED_TEXTS` texts or more than `MAX_REQUEST_BYTES` of them, zero dimensions or a zero
timeout is refused before any I/O, and no message quotes a text.

**Wire.** `llm-embed`'s `OpenAiEmbedder` serves OpenAI's `POST /v1/embeddings` (OpenAI and
compatible servers) at `<endpoint base URL>embeddings`, over `llm-http` with the credential the
account declares, resolved through the injected resolver. The body is `{model, input: [texts],
encoding_format: "float", dimensions?}`; `input` is always an array, `dimensions` is sent only
when asked for. A target served over Messages has no such endpoint and is refused as
`unsupported`. A request larger than the batch limit is split into batches in input order and
sent one after the other; each answer is placed back by its `index`. An answer is refused as
`protocol`, dispatch `accepted`, final, unless it holds exactly one vector per text of its batch,
names every index of the batch once, every vector is non-empty and of the length of every other
vector of the request (the asked dimensions when asked), and the batches name one model. The
answer's `usage.prompt_tokens` is the batch's input tokens; an absent count stays unknown, and
the request's count is the sum only when every batch reported one, never zero and never the
known part.

**Timeout and failure.** The timeout bounds the whole request, every batch, credential and read
included; past it the request fails `deadline` with the fixed message "the embed request passed
its timeout", dispatch `unknown` once a batch was on the wire and `not-sent` before. A deadline of
the HTTP client's own limits is the transport's `deadline` with its own message. A failed batch
ends the request with its own code, dispatch and retry class, and later batches are not sent;
nothing is retried or fallen back, so a caller that retries a retriable failure resends the
whole request. Cancellation ends it `cancelled`.

**Records.** `OpenAiEmbedder::embed_recorded` writes one `llm_calls::CallRecord` of kind
`embed-batch` per batch sent, with the id `<request id>/<batch position>`: route, target,
reported model, outcome, error code and dispatch, one attempt, the batch's timings and input
tokens, and under a price book the priced input of that batch: the wire has no output and no
cache classes, so `llm-cost` is handed those as known zeros, while the record's usage keeps only
the count the answer reported, and an unreported input count leaves the cost incomplete. A
request refused before its
first batch writes none. A request id too long for its longest batch record id (the `Id` bound
is 256 bytes) is refused `invalid-request`, not sent, before the first batch, so no two batches
share one record id. A batch refused by its credential is recorded `not-sent` without a cost.
The record has no text field, so no text reaches it.

## Client request fields

Accepted for wave 2026-10-16-w08 (llm 0.7.0). Coding-agent clients send fields the 0.6.0 ingress
decoders refused, and the Chat and Responses decoders passed over unknown keys below the top level.
Every field a decoder reads now has exactly one rule, and every key it does not name is refused by
its path (`messages[].content[].cache_control`), at every level:

| Rule | Meaning |
| --- | --- |
| carried | held in the neutral `TurnRequest` and re-emitted by every egress wire that has the concept |
| translated | re-emitted under another name by the written table below |
| advisory | a hint about caching, verbosity or presentation; a wire without the concept omits it, and the omission is reported |
| refused | `unsupported`, with a diagnostic that names the field |

### Neutral additions

`TurnRequest.controls` (`llm_core::Controls`) holds what is not sampling:

- `thinking`: `adaptive` (the model decides), `enabled` with a positive `budget_tokens`, or
  `disabled`.
- `parallel_tool_calls`: whether the model may propose several calls in one turn. It needs
  published tools. `false` allows one call at most: a returned turn with more is refused as
  `Protocol`, whatever the tool choice and `return_unpublished_calls`.
- `hints` (`llm_core::Hints`), all advisory: `prompt_cache` (cache what this turn shares with the
  next, with an optional lifetime `5m` or `1h`), `prompt_cache_key`, `verbosity` (`low`, `medium`,
  `high`), `reasoning_summary` (`auto`, `concise`, `detailed`), `anthropic_beta` (the Anthropic
  betas a client opts into), and `unforwarded`: what ingress read and no egress wire sends
  (`input-item-ids`, `client-metadata`). A cache key is non-empty and at most 512 bytes, like every
  other identifier the turn bounds. `anthropic_beta` holds at most 32 distinct names, each 1 to 128
  ASCII letters, digits, `-`, `_` or `.`.
- `return_unpublished_calls`: a model call to a tool the turn did not publish is returned to the
  caller as an ordinary call, for the caller to answer as a refusal, instead of refusing the whole
  turn. Absent (the default) refuses, as before. Only outcome validation reads it; no
  wire sends it and no ingress decoder sets it. Every other outcome check still refuses: a repeated
  call id, a call other than a named tool choice, a second call when `parallel_tool_calls` is
  `false`, oversized arguments. Bedrock Converse also refuses an unpublished name outside
  `[A-Za-z0-9_-]`, which its next turn could not carry back.
- `output_format` (`llm_core::OutputFormat`): a JSON Schema the answer's text is held to, with an
  optional `name` and an optional `strict` flag ("Structured output" below).

A turn whose controls are empty is written as `llm.turn/3`, byte for byte as before; any other is
`llm.turn/4`. Both are read, and a `/3` document carrying controls is refused.

### Reasoning

The neutral effort is still a name (`Sampling::reasoning_effort`); `llm_core::wire_effort` is the
one table that turns it and `thinking` into what a wire sends, and `TurnRequest::validate_for`
checks the binding's `reasoning_efforts` against the name the wire will receive.

| Neutral | Messages | Chat Completions | Responses |
| --- | --- | --- | --- |
| effort `max` | `output_config.effort` `max` | `reasoning_effort` `xhigh` | `reasoning.effort` `xhigh` |
| any other effort | the same name | the same name | the same name |
| thinking `adaptive` | `thinking` `{"type":"adaptive"}` | nothing: adaptive is this wire's own behaviour | nothing, likewise |
| thinking `enabled` | `thinking` `{"type":"enabled","budget_tokens":n}` | refused: no thinking budget | refused: no thinking budget |
| thinking `disabled`, no effort | `thinking` `{"type":"disabled"}` | `reasoning_effort` `none` | `reasoning.effort` `none` |
| thinking `disabled` and an effort | both, as read | refused: a contradiction this wire cannot hold | refused, likewise |
| `reasoning_summary` hint | omitted (advisory) | omitted (advisory) | `reasoning.summary` |

### Tool calls

| Neutral | Messages | Chat Completions | Responses |
| --- | --- | --- | --- |
| `parallel_tool_calls` `b` | `tool_choice.disable_parallel_tool_use` `!b` (with `{"type":"auto"}` when the choice is automatic) | `parallel_tool_calls` `b` | `parallel_tool_calls` `b` |

### Structured output

Accepted for wave 2026-11-04-w25 (story:structured-output). `controls.output_format` is carried:
every egress wire sends it or refuses the turn before any I/O, and none drops it. The schema is
the caller's: llm checks that it is a JSON object within the tool argument bound and sends it as
it is. **llm does not validate the answer against the schema**; the answer is the model's text,
and checking it is the caller's. A name is 1 to 64 ASCII letters, digits, `_` or `-`. A model
may declare `structured_output`; one declaring `false` is rejected for a turn with a format as
`structured-output` ([Structured output and image input
declarations](#structured-output-and-image-input-declarations)).

| Neutral | Messages and Bedrock InvokeModel | Chat Completions | Responses | Bedrock Converse |
| --- | --- | --- | --- | --- |
| the format | `output_config.format` `{"type":"json_schema","schema":…}`, beside the effort | `response_format` `{"type":"json_schema","json_schema":{"name","schema","strict"}}` | `text.format` `{"type":"json_schema","name","schema","strict"}`, beside `verbosity` | `outputConfig.textFormat` `{"type":"json_schema","structure":{"jsonSchema":{"schema":"<JSON text>","name"}}}` |
| `name` | not sent: a label this wire has no place for | required: a format without one is refused | required: refused likewise | sent when present |
| `strict` absent | nothing | nothing: the provider's default | nothing: the provider's default | nothing |
| `strict` `true` | nothing more: the route always holds the answer to the schema | `strict: true` | `strict: true` | nothing more, likewise |
| `strict` `false` | refused: the route cannot be asked not to | `strict: false` | `strict: false` | refused, likewise |

Each choice follows this design's rules: absent stays absent, so a `strict` nobody set is not
written; a field is carried or refused, never dropped, so a wire that requires a name refuses a
format without one, and llm invents none; a flag the wire cannot honour is refused, and one it
always honours sends nothing. Messages, Bedrock InvokeModel and Converse always constrain the
answer to the schema ([Anthropic structured outputs](https://platform.claude.com/docs/en/build-with-claude/structured-outputs),
[Bedrock structured output](https://docs.aws.amazon.com/bedrock/latest/userguide/structured-output.html)),
so `strict: true` is honoured by construction and `strict: false` is refused by name. A name is
sent wherever the wire has a place for it: Chat and Responses require one, and Converse sends it as
`structure.jsonSchema.name`. Messages has none, and a name there is the one exception that is not
sent: it labels the format and asks for nothing the route would do differently, so nothing is lost.

Ingress reads the same field back on every wire that has it, and the egress encoder of each wire
writes it back as it was read:

- Responses `text.format` and Chat `response_format` of type `json_schema` are the format; any
  other key inside it is refused by its path (`text.format.description`,
  `response_format.json_schema.description`). `{"type":"text"}` and `null` are the wire's own
  default and read as no format. `json_object` (JSON mode) asks for any JSON at all, which no
  schema the neutral turn holds says, and is refused naming that type; any other type is refused
  without quoting it.
- Messages `output_config.format` of type `json_schema` is the format, with no name and no strict
  flag; `name`, `strict` or any other key inside it is refused by its path, and another type is
  refused.

### Advisory hints

| Hint | Messages | Chat Completions | Responses |
| --- | --- | --- | --- |
| `prompt-cache` | carried: a client's own marks where it placed them (`cache_marks`), else the breakpoints this projection places (the system block, the rolling tail), each with its lifetime | omitted | omitted |
| `prompt-cache-key` | omitted | `prompt_cache_key` | `prompt_cache_key` |
| `verbosity` | omitted | `verbosity` | `text.verbosity` |
| `reasoning-summary` | omitted | omitted | `reasoning.summary` |
| `anthropic-beta` | carried: one `anthropic-beta` header, after the subscription beta when there is one | omitted | omitted |
| `input-item-ids`, `client-metadata` | omitted | omitted | omitted |

Bedrock InvokeModel shares the Messages column and writes `anthropic-beta` as the body's
`anthropic_beta` array instead of a header; Bedrock Converse omits every hint.

On Responses a client given a conversation sends that conversation's identifier as
`prompt_cache_key`; a turn whose own key differs is refused rather than one of the two silently
winning.

An omission is never silent. `omitted_advisories` lists hints in the order of the table above. Each protocol crate exports `omitted_advisories(&TurnRequest)`, the
hints its projection does not send, so a gateway can report them before it forwards anything; and
each client emits one `Warning` stream event with code `advisory-omitted` naming them once the turn
has succeeded. Not before: a warning on the sink counts as visible output, and an attempt that
showed output is no longer eligible for fallback. A sink that refuses that warning fails the turn with its own error and
`accepted` dispatch, as any sink refusal after dispatch does: the turn was served and is not
resent.

### Ingress rules

The per-wire pages hold the full tables: `docs/messages.md` "What the declared subset carries",
`docs/responses.md` "The pinned request subset", `docs/chat.md` "Gateway ingress". In short:

- Messages: `thinking` and `tool_choice.disable_parallel_tool_use` are carried; a `system` of text
  blocks is the instructions, its blocks joined by a line feed as Chat already joins system
  messages (a block of another type is refused); `cache_control` (`ephemeral`, optional `ttl`) on a
  system block, a text, `tool_use` or `tool_result` block, or a tool is the `prompt-cache` hint, and
  two different lifetimes in one request are refused; `output_config.effort` takes any name,
  `max` included; `output_config.format` is the output format; `metadata` is `client-metadata`. The `anthropic-beta` request header is not in
  the body: a gateway that passes the headers to `decode_request_with_headers` gets its
  comma-separated names as the `anthropic-beta` hint, and a bad name is refused by the header's
  name.
- Responses: `instructions` is the instructions, followed by the text parts of the developer
  messages that open `input`; `parallel_tool_calls`, `prompt_cache_key`, `text.verbosity`,
  `text.format` and `reasoning.summary` are carried; an item `id` is `input-item-ids` and
  `client_metadata` is `client-metadata`; a `function_call_output` string that is not the pinned
  envelope is a successful result carrying that string. A tool of a type other than `function` is
  refused by its type.
- Chat Completions: `parallel_tool_calls`, `prompt_cache_key`, `verbosity` and `response_format`
  are carried, and an assistant message's `refusal` text is an `Item::AssistantRefusal`; every
  object below the top is closed, and a `strict` function schema is refused. A `tool` message whose
  content is exactly the failure object `{"error":<output>,"ok":false}` is a failed result
  ("Failed and rejected tool results" above); any other content is a successful string.

Structured output is carried ("Structured output" above): a JSON Schema `output_config.format`,
`text.format` or `response_format` is read, and JSON mode (`json_object`) is refused by its type.

Still refused, by name, and outside this design: server-side context editing
(`context_management`), hosted and custom tools, `thinking.display`, and an instruction in the
middle of the conversation (a developer message after the first item of Responses `input`, a Chat
system message after the first message): the neutral turn has no item that places an instruction
between two turns.

One exception, decision D262 (story:messages-ingress-system-messages): a `system` message in
Messages `messages` is read. Those before the first message of another role join the instructions;
a later one stays at its place as user text in the fixed mark `<system-message>` … `</system-message>`,
or after the tool results that answer a tool call it follows (`docs/messages.md`). The mapping is made at ingress, so every backend receives the same items.

## Gateway and hosting

This half of the design moved with its crates to
[beyond10x/llm-gateway](https://github.com/beyond10x/llm-gateway) (operator, 2026-10-05); llm keeps
the client side. The target below is unchanged and is kept here as the record it was accepted in.

Serve one owner/trusted deployment with authenticated access. Translate only the published neutral
subset across three protocol ingress surfaces; preserve streaming, tools, cancellation and failure
semantics. Listing or explaining routes must not resolve secrets or provision resources.

Existing endpoints do not require provisioning. Hosting adapters have explicit deploy/readiness,
owned-resource reconciliation, concurrency, lease and cleanup behavior. Implement Runpod and Modal
independently behind this seam. Cloud-specific provisioning capabilities require documented control
plane evidence; unsupported lifecycle actions must be reported rather than simulated. Prevent two
controllers from owning one billed resource. No deployment or paid qualification is authorized by
a normal repository gate.

## Source evidence and draft limits

- Harness `crates/harness-wire/src/{port,turn,bearer}.rs`: neutral turns/usage, credential injection,
  and the tool-authority coupling to leave behind; `crates/harness-loop/src/price.rs`: existing prices.
- Harness `crates/harness-cli/src/provider.rs`: endpoint, model, wire and credential bundle.
- Platform `runtime/inference/crates/inference-route/src/{catalog,vocabulary}.rs`: endpoint/model
  capabilities; archived source is evidence, never a retained dependency.
- llmgw `src/config.rs` and `src/runpod.rs`: provider/model configuration and deployment mechanics.
- Operator decisions: full foundation first; single owner; injected custody; caller-managed
  subscription auth; opt-in fallback; accounting/limits; subset translation; later llmgw retirement.

The catalog ESS domain describes declaration records with a single Declared state. The routing
and secrets verification domains describe adapter observations of real library calls; their
authored scenarios assert returned selection, refusal, request preservation and secret-resolution facts. They do not
promise runtime event publication, persistent routing jobs or hosting/budget state machines.
Budget ownership/scope is now resolved by [the single-owner ledger contract](budgets.md). Its ESS
domain declares the policy, owned obligations and inspection records with closed runtime phase
values; authored programs observe the real durable engine's transitions and refusals.
The hosting lifecycle is resolved: `spec/domains/hosting.yaml` declares the permitted transition
table the library enforces, and a pair absent from it is refused.
