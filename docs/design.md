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
adapters implement Responses, Messages and Chat Completions with one documented text/tool/streaming
subset. Unsupported fields and provider-specific opaque continuation state must be preserved or
refused, never dropped to make translation appear successful.

### Streaming declarations

A caller, such as a graph deciding how to schedule a model node, reads two declarations from
`Model::capabilities()` (`llm_core::Capabilities`), beside the settings a route admits:

| Declaration | Means | Every client in this workspace |
| --- | --- | --- |
| `output_streaming` | the model's text reaches the sink incrementally, as it is generated, not only at the end | true: Responses, Messages and Chat Completions all request a streamed answer |
| `input_during_output` | input can reach a turn that is still producing output | false, until story:input-into-a-running-turn decides otherwise |

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

## Routing and accounting

Fallback is opt-in and ordered. Only defined failures before output becomes visible are eligible;
request capabilities, state compatibility, deadlines and remaining spending limits constrain every
alternative. Do not replay an ambiguously accepted request by assuming it was free. Record every
attempt and preserve unknown spend. No semantic downgrade, cross-account switch outside the named
chain, or fallback after an exposed partial stream.

One deliberate relaxation, matching Harness: a failure whose retry class is retriable (408, 429,
5xx including 529, a request or body that failed in transit, a stream cut inside an event) and
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
`crates/llm-http/tests/retry_classes.rs`. The transport never reads an error body. So a 429 whose
body says the quota is exhausted is retried up to the retry policy's bound and then falls back,
while the same quota reported inside a stream is final. Classifying a 429 by its body needs a
transport change that this decision does not make (`story:quota-429-is-final`).

**A relayed failure reads back by the receiving decoder's rules.** The ingress encoders write a
failure under the wire code that reads back as its class. No wire field carries the relaying
side's retry decision. A Responses `fail` writes `unavailable` as `server_is_overloaded` or
`server_error`, and `rate-limited` as `rate_limit_exceeded`. A Messages `fail` writes `unavailable`
as `overloaded_error`, and `transport`, `protocol` and `cancelled` as `api_error`. When the gateway
streamed nothing before the failure, a caller reads each of these back as retriable, whatever the
gateway decided, including a failure it had marked final or one whose retries it had already spent.
The caller's own routing may then attempt the gateway again. This is accepted: vendor clients
already retry these provider codes. Keeping the relaying side's decision across the hop is
`story:relayed-failures-keep-their-retry-decision`.

[openai-errors]: https://developers.openai.com/api/docs/guides/error-codes
[openai-responses]: https://developers.openai.com/api/reference/resources/responses/methods/create
[anthropic-errors]: https://platform.claude.com/docs/en/api/errors

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
  published tools.
- `hints` (`llm_core::Hints`), all advisory: `prompt_cache` (cache what this turn shares with the
  next, with an optional lifetime `5m` or `1h`), `prompt_cache_key`, `verbosity` (`low`, `medium`,
  `high`), `reasoning_summary` (`auto`, `concise`, `detailed`), and `unforwarded`: what ingress read
  and no egress wire sends (`input-item-ids`, `client-metadata`). A cache key is non-empty and at
  most 512 bytes, like every other identifier the turn bounds.

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

### Advisory hints

| Hint | Messages | Chat Completions | Responses |
| --- | --- | --- | --- |
| `prompt-cache` | carried: the breakpoints this projection places (the system block, the rolling tail) take its lifetime | omitted | omitted |
| `prompt-cache-key` | omitted | `prompt_cache_key` | `prompt_cache_key` |
| `verbosity` | omitted | `verbosity` | `text.verbosity` |
| `reasoning-summary` | omitted | omitted | `reasoning.summary` |
| `input-item-ids`, `client-metadata` | omitted | omitted | omitted |

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
  `max` included; `metadata` is `client-metadata`.
- Responses: `instructions` is the instructions, followed by the text parts of the developer
  messages that open `input`; `parallel_tool_calls`, `prompt_cache_key`, `text.verbosity` and
  `reasoning.summary` are carried; an item `id` is `input-item-ids` and `client_metadata` is
  `client-metadata`; a `function_call_output` string that is not the pinned envelope is a
  successful result carrying that string. A tool of a type other than `function` is refused by its
  type.
- Chat Completions: `parallel_tool_calls`, `prompt_cache_key` and `verbosity` are carried; every
  object below the top is closed, and a `strict` function schema is refused.

Still refused, by name, and outside this design: structured output (`output_config.format`,
`text.format`, `response_format`), server-side context editing (`context_management`), hosted and
custom tools, `thinking.display`, and an instruction in the middle of the conversation (a
`system` message in Messages `messages`, a developer message after the first item of Responses
`input`): the neutral turn has no item that places an instruction between two turns.

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
