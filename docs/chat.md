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
| `tools` | `tools[].function` with `name`, `description` and `parameters` |
| `ToolChoice::Required` / `Named` | `"required"` / `{"type":"function",...}`; `Auto` is omitted |
| `max_output_tokens` | `max_completion_tokens`, never the deprecated `max_tokens` |
| `temperature`, `top_p`, `reasoning_effort` | the same names, and absent when the caller did not ask |

A streamed request always sends `stream_options: {"include_usage": true}`. Without it this wire
reports no usage at all, and an absent report would be indistinguishable from a model that
consumed nothing.

One assistant turn is one message. A run of consecutive assistant items therefore collapses:
their text joins with a newline and their proposed calls become one `tool_calls` array.

A tool message carries text only, so a failed result travels inside it as
`{"error": <output>, "ok": false}`. This is the envelope `beyond10x/harness` `9e401e40` uses on
the equally text-only Responses wire (`crates/harness-responses/src/project.rs`), for the same
reason: an empty failed result must never read to the model as an empty success. A successful
string result is passed through unquoted; any other value is compact JSON.

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

Refused, each with a fixed diagnostic that carries no byte of the upstream response: a second
choice, any other finish reason, a stream that ends before its `[DONE]` sentinel, a stream that
carries no finish reason, a tool call whose identifier or name never arrived, arguments that
are not valid JSON, a fragment renaming a call already announced, and counters that contradict each other.

A chunk carrying more than one choice is refused however it numbers them. An index that is
not a whole count numbers nothing, and joining two completions would hand the caller one
assistant turn assembled out of two.

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

**An absent counter is unknown, and unknown is never zero.** A `usage` object that names one
counter leaves the others unknown; `usage: null` on every chunk leaves all of them unknown. A
zero the upstream actually reported is a report and stays one. The same rule governs the model
name: a response that names no model leaves `upstream_model` absent, and the configured alias
is never substituted for it.

`final_usage` becomes true when the stream terminates. Before that, `observation()` exposes a
partial snapshot, which is what a failure after dispatch retains.

## Gateway ingress

`decode_ingress_request` reads a client request strictly. The implemented fields are
`SUPPORTED_REQUEST_FIELDS`; the fields this subset knows about and deliberately does not
implement are `NAMED_UNSUPPORTED_REQUEST_FIELDS`, and a request carrying one is refused with
that field's own name. A field in neither list is refused too, with a diagnostic that quotes
nothing the client sent — so a new vendor field is never silently ignored.

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

A `tool` message decodes to a result with `failed: false`, because this wire has no place to
say a call failed. Its content stays a string: keeping it is lossless, and guessing that it
was meant to be JSON would not be.

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
cancellation. Retries, alternates and fallback belong to routing.

Any endpoint serving this wire works, including an explicitly anonymous, self-hosted one — the
shape a local `vLLM` deployment declares. `crates/llm-chat/fixtures/` holds pinned response
bytes in the shapes this wire is served in, and the integration test drives a real socket
against them. That is fixture evidence for the projection, not qualification of a live `vLLM`,
provider or hosting deployment.
