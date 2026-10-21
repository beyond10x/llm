# Chat Completions projection

`llm-chat` projects the declared neutral text-and-tool subset onto the Chat Completions wire,
in both directions: outgoing calls to a provider or to a compatible local endpoint, and
gateway ingress from a client that speaks this wire. It is a library, not a client of any
particular provider: the endpoint, upstream model name and capabilities all come from the
selected binding.

This page documents the subset. Everything outside it is refused by name rather than ignored.
Nothing here establishes live provider, subscription or hosting qualification; the evidence is
[fixture evidence](verification/chat.md).

## Outgoing calls

`project_request(request, binding, streaming)` builds one request body from a validated
`TurnRequest`. Before building anything it runs `TurnRequest::validate_for` against the
binding's declared capabilities, so a setting the operator did not declare is refused before
any network I/O.

| Neutral value | Wire |
| --- | --- |
| the binding's upstream model name | `model` — never the caller's route alias |
| `instructions` | a leading `system` message, omitted when empty |
| `Item::UserText` | a `user` message |
| `Item::AssistantText` and `Item::ToolCall` | one `assistant` message carrying `content` and `tool_calls` |
| `Item::ToolResult` | a `tool` message keyed by `tool_call_id` |
| `Item::Opaque` | refused: this wire carries no continuation state |
| `Item::UnattributedOpaque` | refused by name (`Item::UNATTRIBUTED_REFUSAL`), in both directions: never sendable unbound, and not here even bound |
| `tools` | `tools[].function` with `name`, `description` and `parameters` |
| `ToolChoice::Required` / `Named` | `"required"` / `{"type":"function",...}`; `Auto` is omitted |
| `max_output_tokens` | `max_completion_tokens`, never the deprecated `max_tokens` |
| `temperature`, `top_p` | the same names, and absent when the caller did not ask |
| `reasoning_effort` and `controls.thinking` | `reasoning_effort`, as `llm_core::wire_effort` gives it: `max` is sent as `xhigh`, disabled thinking as `none`, adaptive thinking sends nothing more; a thinking budget, and disabled thinking beside an effort, are refused |
| `controls.parallel_tool_calls` | `parallel_tool_calls` |
| `hints.prompt_cache_key`, `hints.verbosity` | `prompt_cache_key`, `verbosity` (`low`, `medium`, `high`) |
| `hints.prompt_cache`, `hints.reasoning_summary`, `hints.unforwarded` | not sent: this wire has no field for them. `omitted_advisories(&request)` names them, and `ChatClient` reports them in one warning after a successful turn |

Each control and hint is sent only when the turn sets it.

A streamed request always sends `stream_options: {"include_usage": true}`. Without it this wire
reports no usage at all, and an absent report would be indistinguishable from a model that
consumed nothing.

One assistant turn is one message. A run of consecutive assistant items therefore collapses:
their text joins with a newline and their proposed calls become one `tool_calls` array.

A tool message carries text only, so a failed result travels inside it as
`{"error": <output>, "ok": false}`. This is the envelope `beyond10x/harness` `9e401e40` uses on
the equally text-only Responses wire (`crates/harness-responses/src/project.rs`), for the same
reason: an empty failed result must never read to the model as an empty success. A successful
string result is passed through unquoted; any other value is compact JSON. Gateway ingress reads
the failure object back as a failed result ([below](#gateway-ingress)).

A call the caller rejected before running it is sent the same way: `failed: true`, with the reason
as the caller's text in `output`, so the model reads `{"error": <reason>, "ok": false}`. This wire
has no separate rejected state, and llm adds none ([design](design.md#failed-and-rejected-tool-results)).

## Reading a response

`project_response_bytes` drives the shared bounded SSE framing and returns the neutral deltas
beside the outcome, so an attempt that fails after exposing output keeps that output
observable. `StreamProjection` is the same projection for a caller that already has framed
events, and `decode_completion` reads a non-streamed body.

Response fields this subset does not read are ignored, because a server may add fields at any
time. Every field it does read is refused when it contradicts the wire:

| Wire | Neutral |
| --- | --- |
| `delta.content` | `StreamEvent::TextDelta`, accumulated into one `AssistantText` |
| `delta.reasoning_content` (`vLLM` reasoning parsers) | `StreamEvent::ReasoningDelta`, never joined to the assistant text |
| `delta.tool_calls[]`, correlated by `index` | `StreamEvent::ToolCallStarted` once, then `StreamEvent::ToolArgumentsDelta`, and one `ToolCall` |
| `finish_reason` `stop` / `length` / `tool_calls` / `content_filter` | `EndTurn` / `MaxOutputTokens` / `ToolCalls` / `Incomplete{content-filter}` |
| `id`, `model` | `response_id`, `upstream_model` — absent stays absent |
| `usage` | `Usage`, counter by counter |
| `timings` (llama-server) | the counters `usage` does not name ([below](#counters)) |

Refused, each with a fixed diagnostic that carries no byte of the upstream response: a second
choice, any other finish reason, a stream that ends before its `[DONE]` sentinel, a stream that
carries no finish reason, a tool call whose identifier or name never arrived, arguments that
are not valid JSON, a fragment renaming a call already announced, counters that contradict each
other, `timings` that contradict the `usage` report, and a `timings` field in a shape the
server does not write ([below](#counters)).

llama.cpp's `llama-server` adds more to a chunk than the fields above. Its `reasoning_content`
is read as `vLLM`'s is, and its `timings` for the counters in it; the other `timings` fields
are accepted only in the shape the server writes them in, and not read. The rest is not read:
the build it writes into `system_fingerprint`, a `__verbose` debug copy of the result (only from
a server run at verbosity above 9), and `prompt_progress` (only for a request setting
`return_progress`, which nothing here sends). Its keep-alive, an SSE comment sent by default
after 30 seconds without a chunk, is skipped by the framing. The source of each is in
[the verification record](verification/chat.md#llama-server).

A chunk carrying more than one choice is refused however it numbers them. An index that is
not a whole count numbers nothing, and joining two completions would hand the caller one
assistant turn assembled out of two.

**The turn's text is the text the caller was shown.** A stream carries text only as
`delta.content`, and the turn's one `AssistantText` is those deltas joined, so it cannot differ
from what the caller was shown and `shown-text-kept` never appears on this route (`docs/design.md`
"Streamed events are final").

**Every one of those carries dispatch evidence that the request was served.** Nothing in this
direction is reachable before the endpoint has sent bytes, and dispatch is the retry signal:
`not-sent` on a response that arrived would invite a resend of a turn the provider already
served and may already have billed. The rule is applied once at each of the three public
entry points — `StreamProjection::accept`, `StreamProjection::finish` and
`decode_completion` — rather than at each refusal, so one added inside cannot escape it. An
earlier revision of this page said the rule covered every entry point while `accept` was
missing it; the count is now read out of the source by a case rather than asserted here.
Evidence a lower layer already qualified — the transport's `unknown` for an EOF inside an
event — is kept as it is.

The same holds one layer up. Inside `ChatClient::turn`, every failure raised after the
transport has returned a response is a failure after dispatch, whatever marker it arrives
carrying: a bounded sink refusing an event, a projection refusing a chunk, and the neutral
outcome refusing a model's unpublished tool call are all raised locally and default to
`not-sent`. The client stamps them and keeps the evidence the attempt earned.

A tool call whose accumulated arguments are empty reaches the caller as `{}`, which is what
this wire's empty argument string means: a call the model made with no arguments.

**A streamed call is announced where the provider named it.** The fragment that opens a call
carries its identifier and its name, and `StreamEvent::ToolCallStarted` is emitted from it, once,
before any `ToolArgumentsDelta` for that call. No argument fragment is relayed under a call the
caller has not been told about: fragments that arrive after the identifier but before the name
are held and relayed in one delta right behind the announcement. A call whose name never arrives,
or is not usable, is never announced, and like a fragment that renames a call already announced
it is refused by `finish` — after the counters the endpoint reports last — not mid-stream. A
second index opened under an identifier already announced is not announced again, and its
fragments are not relayed.

### Counters

`prompt_tokens`, `completion_tokens`, `prompt_tokens_details.cached_tokens` and
`completion_tokens_details.reasoning_tokens` map onto `input_tokens`, `output_tokens`,
`cached_input_tokens` and `reasoning_output_tokens`. No provider serving this wire reports a
cache-write counter, so against one `cache_creation_input_tokens` stays unknown; it is read
from `prompt_tokens_details.cache_creation_input_tokens` because the gateway below writes it
there, and a counter deleted on read-back is a counter this wire lost.

`llama-server` reports its counters twice. Beside `usage` it writes a `timings` object, on the
last chunk of a stream: the usage chunk when the request set `stream_options.include_usage`, as
every streamed request here does, and the chunk carrying the finish reason when it did not (and,
with `timings_per_token`, on every earlier chunk too, below). `prompt_n` (prompt tokens processed) and `cache_n` (prompt tokens reused) are the input
together, `cache_n` the cached part of it, and `predicted_n` the output. A counter `usage` does
not name is taken from `timings`, the input only when both its parts are there. A counter both
name has to agree, because the server writes both from the same count, so a pair that differs is
refused (`chat completion timings contradict its usage report`) rather than one of the two
believed.

`timings` is read only from the report that ends a stream: the chunk carrying the finish reason
or the usage chunk after it. With `timings_per_token` the server writes `timings` on every
earlier chunk as well (`server-context.cpp:2222-2225`, `server-task.cpp:1146-1148` at
`7879491cf`), counting the turn only so far. One there is held to its shape, below, and then
dropped: it is neither the turn's count, so a stream with no later report leaves its usage
unknown, nor evidence against the `usage` the stream ends with.

The other `timings` fields llama.cpp documents at `7879491cf` are accepted and not read, because
`Usage` has no place for a duration, a rate or a count of speculative decoding:

| `timings` field | What it is | Accepted as |
| --- | --- | --- |
| `prompt_ms`, `predicted_ms` | milliseconds spent on the prompt and on generation | a number |
| `prompt_per_token_ms`, `predicted_per_token_ms` | milliseconds per token | a number |
| `prompt_per_second`, `predicted_per_second` | tokens per second | a number |
| `draft_n`, `draft_n_accepted` | tokens drafted and accepted by speculative decoding | a whole count |

Accepting a field is not ignoring its value, the rule `llm-messages` applies to its accepted,
unread fields. One in any other shape, `null` included (the server guards every division behind
a rate, so it never writes the `null` its JSON library makes of a number that is not finite), says
the endpoint is not the server this subset was written against. It is refused by name, without
its value, or `timings` itself is when it is not an object:
`chat completion timings field has a shape llama-server does not write: timings.prompt_ms`.
A `timings` key llama.cpp does not document at that commit passes, as every unread key on this
wire does (`usage.total_tokens`, `prompt_tokens_details.audio_tokens`): refusing one would fail
every turn against a newer server that adds a field.

**An absent counter is unknown, and unknown is never zero.** A `usage` object that names one
counter leaves the others unknown; `usage: null` on every chunk leaves all of them unknown. A
zero the upstream actually reported is a report and stays one. The same rule governs the model
name: a response that names no model leaves `upstream_model` absent, and the configured alias
is never substituted for it.

`final_usage` becomes true when the stream terminates. Before that, `observation()` exposes a
partial snapshot, which is what a failure after dispatch retains. A chunk whose own report is
refused — an unusable identifier or model, a counter or `timings` field in the wrong shape, or
`timings` contradicting `usage` — adds nothing to it: the failure carries the snapshot as it
stood before that chunk, so a figure the refusal declined to believe is never reported as known.

## Gateway ingress

`decode_ingress_request` reads a client request strictly. The implemented fields are
`SUPPORTED_REQUEST_FIELDS`; the fields this subset knows about and deliberately does not
implement are `NAMED_UNSUPPORTED_REQUEST_FIELDS`, and a request carrying one is refused with
that field's own name (``chat request field `n` is outside the published subset``).

Every object of the request is closed, not only the top level. A key a level does not publish
is refused as `chat request field is outside the published subset: <path>`, the path written by
`llm_core::field_path`, which copies only a bounded name of the wire's own shape and writes any
other as `?`; no value the client sent is quoted. So a new vendor field is never silently
ignored, at any depth:

| Level | Published keys |
| --- | --- |
| the request | `SUPPORTED_REQUEST_FIELDS` |
| `messages[]`, role `system`, `developer` or `user` | `role`, `content` |
| `messages[]`, role `assistant` | `role`, `content`, `tool_calls` |
| `messages[]`, role `tool` | `role`, `content`, `tool_call_id` |
| `messages[].content[]` | `type` (`text`), `text` |
| `messages[].tool_calls[]` | `id`, `type` (`function`), `function` |
| `messages[].tool_calls[].function` | `name`, `arguments` |
| `tools[]` | `type` (`function`), `function` |
| `tools[].function` | `name`, `description`, `parameters`, `strict` (absent or `false`) |
| `tool_choice`, as an object | `type` (`function`), `function` |
| `tool_choice.function` | `name` |
| `stream_options` | `include_usage` |

A key the subset reads with a value of the wrong type is refused rather than read as absent
(`invalid-request`, or `unsupported` for a `tool_choice` or a `type` outside the subset); an
explicit `null` is absent for every optional key except `tools`, which must be an array when
present. `strict: true` is refused
(`chat strict function schemas are outside the published subset`): the neutral tool has no
strict mode. A tool call or object tool choice whose `type` is not `function` is refused by its
type.

`parallel_tool_calls` (a boolean) is carried in the turn's controls, `prompt_cache_key` (a
string) and `verbosity` (`low`, `medium` or `high`) in its hints; `project_request` sends each
back under the same name, so the body it builds decodes to the same turn.

Also refused: a role outside `system`/`developer`/`user`/`assistant`/`tool`, a content part
that is not text, `tool_choice: "none"` (the neutral subset cannot express it and must not
quietly treat it as `auto`), contradictory `max_tokens` and `max_completion_tokens`, a tool
declaration without a `parameters` schema, and system instructions that do not precede the
conversation.

A decoded request is then put through `TurnRequest::validate`, so the gateway refuses at the
door everything the neutral subset refuses: a named tool choice with no published tools, a
temperature outside its range, an output limit of zero, a tool message answering no call,
duplicate call identifiers, a model name the neutral vocabulary cannot hold. Accepting those
would only move the same refusal into `project_request`, after the client has been told yes
and after a route has been chosen. An ingress refusal is `not-sent`: nothing has been
dispatched anywhere.

A `tool` message decodes to a result answering its `tool_call_id`, which is checked before the
content is read. Its content, its text parts joined, is a failed result when it parses to a JSON
object of exactly the keys `error` and `ok`, each once, `ok` being `false`: the object the
outgoing projection sends for a failure. The output is then the `error` value, whatever its type,
so a failed result `project_request` sends decodes to the result it was, within two bounds ingress
keeps on client input:

- Content is parsed only when it opens with `{` and is no longer than the tool-result bound
  (`MAX_TOOL_RESULT_BYTES`, 256 KiB). Longer content is left to that bound, which refuses it as
  too large. The failure object adds 21 bytes around the output's JSON, so a failed output whose
  JSON is within 21 bytes of the bound is sent and then refused here.
- `serde_json` parses at most 127 levels deep (its recursion limit of 128), and the failure object
  is one of them, so a failed output nested more than 126 levels deep reads back as a successful
  string carrying the object's text. The limit is kept: lifting it would let a client's nesting
  reach the stack.

Any other content, JSON or not (`{"ok": false}` alone, an object with a third key or a repeated
`error` or `ok` key, `ok` true, an array), is a successful result whose output is that content as
a string, byte for byte: keeping it is lossless, and guessing that it was meant to be JSON would
not be. The `error` value itself is read as any JSON value is, so a key repeated inside it keeps
its last value. A successful result whose text is exactly the failure object therefore reads back
as failed; the wire cannot tell the two apart.

`encode_ingress_completion` and `IngressStream` answer on the same wire. The caller supplies
the response identity and creation time; this library owns no clock and mints no identity.
An unreported upstream model leaves `model` out of the answer, and an unreported counter leaves
its field out of `usage` — a total is derived only from two known parts. A reported
cache-write counter, which this wire has no field for, travels under
`prompt_tokens_details.cache_creation_input_tokens` rather than being deleted.

`IngressStream::chunk` re-emits a `ToolCallStarted` as the chunk that opens the call — its
identifier, `type` and the announced name, under the next wire index — and each following
`ToolArgumentsDelta` under that index. `IngressStream::close` does not repeat a call it already
announced; it sends the arguments of one only when none were streamed, so the client never
assembles an empty one, and refuses an outcome that contradicts what the client was already sent:
an announced call the outcome does not carry, another name, or other arguments — the relayed text
read by the projection's own rule, so blank text is `{}`. A call the stream
never announced — a model that returns its calls only in the outcome — is still emitted once,
complete, in the terminal chunks under the next wire index, rather than dropped or streamed under
an invented name.

## A compatible local endpoint

`ChatClient` joins the outgoing projection and the response projection over the shared bounded
HTTP/SSE transport: one attempt, no redirects, no automatic retry, explicit deadlines, and
cancellation. Retries, alternates and fallback belong to routing. An error status carrying
`x-should-retry: false` reads not retriable whatever its status, so routing does not send the
turn there again; the header never makes a final status retriable
([design.md](design.md#provider-failures-retried-before-output)). Every 429 stays a retriable
rate limit on this wire: no quota rule reads its body.

A connection cut inside the stream, inside an event or inside a chunk, keeps the transport's retry
class until the stream has carried output, shown to the caller or not: non-empty `content` or
`reasoning_content`, any `tool_calls` fragment, or a finish reason. After that the cut is final,
because another attempt would answer, and bill, again; a call whose name has not arrived and a
finish reason show nothing but count. The chunk that opens the stream, `role` with an empty or
null `content`, is not output. This is the rule Responses and Messages apply
(`story:messages-cut-after-silent-output-is-final`).

A turn carrying hints this wire does not send ends, once it has succeeded, with one `Warning`
event of code `advisory-omitted` naming them (`Chat Completions carries no prompt-cache; the
advisory hint was not sent`). Never before or during the turn: a warning counts as visible
output and would end fallback eligibility. A failed turn, or one that omitted nothing, emits
none. A sink that refuses the warning fails the turn with its own error, stamped as served.

Any endpoint serving this wire works, including an explicitly anonymous, self-hosted one — the
shape a local `vLLM` or `llama-server` deployment declares. `crates/llm-chat/fixtures/` holds
pinned response bytes in the shapes this wire is served in, and the integration test drives a
real socket against them. That is fixture evidence for the projection, not qualification of a
live `vLLM`, `llama-server`, provider or hosting deployment.
