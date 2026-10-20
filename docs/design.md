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

A connection cut mid-stream follows the same count (`story:messages-cut-after-silent-output-is-final`).
The transport marks a cut retriable: a body that ends inside an event, or fails in transit. Each
client keeps that mark until its decoder has answered and drops it after, in its stream loop:
`ResponsesClient`, `MessagesClient` and `ChatClient`. On Chat Completions output is non-empty
`content` or `reasoning_content`, any `tool_calls` fragment, or a finish reason; the chunk that
opens the stream, `role` with an empty or null `content`, answers nothing.

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

**A relayed failure reads back by the receiving decoder's rules.** The ingress encoders write a
failure under the wire code that reads back as its class. No wire field carries the relaying
side's retry decision, so a downstream llm client applies its own rules. A Responses `fail` writes
`unavailable` as `server_is_overloaded` or `server_error`, and `rate-limited` as
`rate_limit_exceeded`. A Messages `fail` writes `unavailable` as `overloaded_error`, and
`transport`, `protocol` and `cancelled` as `api_error`. When the gateway streamed nothing before
the failure, a downstream llm client reads each of these back as retriable, whatever the gateway
decided.

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
  object below the top is closed, and a `strict` function schema is refused. A `tool` message whose
  content is exactly the failure object `{"error":<output>,"ok":false}` is a failed result
  ("Failed and rejected tool results" above); any other content is a successful string.

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
