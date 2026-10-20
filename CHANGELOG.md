# Changelog

All notable changes to this component are recorded here. Versions are component-scoped and released
under bare-version tags such as `0.1.0`. The workspace is `publish = false`; releases
are source releases at bare-version tags.

## [Unreleased]

### Added

- `b10x-llm-credentials`: `AnonymousResolver`, the resolver a caller hands a protocol client it
  builds itself (`ChatClient::new`, `ResponsesClient::new`, `MessagesClient::new`) for an
  anonymous binding, such as a keyless local server. It holds no secret and refuses every resolve
  with `SecretError::Missing`, so a credentialed binding handed it by mistake stops as
  `unauthorized` before sending and never falls back; refresh is unsupported. It is not
  feature-gated. `b10x-llm-models` now hands this resolver to the client of an anonymous
  account in place of its private copy; ports behave as before.
- `b10x-llm-http`: `require_tokio_runtime`, the check every protocol client and exchange makes
  first, and `within_tokio_runtime`, which runs a wait only while a Tokio runtime is current and
  refuses it otherwise (below).
- `docs/design.md` "Input into a running turn": llm does not carry input into a turn that is
  still producing output. Each client posts the whole request once over HTTP and reads the
  answer's server-sent events, and over that exchange none of Responses, Messages or Chat
  Completions takes input into a response still streaming. OpenAI's mid-turn steering for Responses
  (`response.steer`, GPT-6 models) needs a Responses WebSocket connection, which llm does not
  implement (`story:responses-websocket-steering`). `Capabilities::input_during_output` stays
  `false`. The supported pattern: cancel the running turn, carry the text its sink was shown as
  assistant text (a delta counts once its `emit` completed), or, for a turn that finished first,
  its outcome's items with a result for every tool call it proposed, then the new input. The guide
  "Interrupt a turn with new input" runs it offline (`crates/llm-docs/examples/interrupt_turn.rs`,
  five tests, one per way the first turn can end, two proposed calls included).
- `b10x-llm-chat`: three response streams in llama.cpp `llama-server`'s shape
  (`crates/llm-chat/fixtures/llama-server-*.sse`): a reasoning model's text turn, a tool call, and
  a stream answering a request without `stream_options.include_usage`. They are authored from
  llama.cpp's server source at `7879491cf` (release `b11752`), not captured; every field's source
  line is in `docs/verification/chat.md`. Five ESS scenarios carry them; `contracts/baseline.json`
  rises from 883 to 888.

### Changed

- `b10x-llm-messages`: a usage report without `cache_creation_input_tokens` whose `cache_creation`
  breakdown reports both `ephemeral_1h_input_tokens` and `ephemeral_5m_input_tokens` now has a
  cache-write count, their sum, and so an inclusive `input_tokens` when `input_tokens` and
  `cache_read_input_tokens` are reported too. Anthropic documents the total input as the sum of the
  three counts and the breakdown as the write split by TTL (`docs/messages.md` "Usage"). A
  breakdown that is absent or missing either count leaves the write count and the total unknown,
  never zero. The derived count is cumulative like a reported one: a reported
  `cache_creation_input_tokens` in the same event has to equal the breakdown sum, a later one may
  raise the derived count but never lower it, and a breakdown that contradicts a reported count is
  refused as `protocol`. The Harness emulator sends no
  breakdown, so its `absent-cache-write` scenario still reports no total.
- `b10x-llm-chat`: llama-server's `timings` object is read for its counters: `prompt_n` plus
  `cache_n` is the input, `cache_n` the cached input and `predicted_n` the output; a counter the
  `usage` report does not name is taken from there, so a llama-server stream without a usage chunk
  no longer reports every counter unknown. Where `usage` and `timings` both name a counter they
  must agree, or the turn is refused (`chat completion timings contradict its usage report`) and
  the failure reports neither. The other documented `timings` fields (durations, rates, draft
  counts) are accepted and not read, in the shape llama-server writes; any other shape is refused
  naming the field, not its value. A chunk refused while its report is read adds nothing to the
  evidence a failure carries.

### Fixed

- A turn of the Responses, Messages or Chat Completions client, or an exchange of
  `b10x-llm-http`, polled on a thread in no Tokio runtime is refused with `unsupported`, not
  retriable, and the message "an llm client needs a Tokio runtime; run the call inside one, or use
  b10x-llm-blocking for a single model", where it used to panic ("there is no reactor running").
  The first poll is refused before a credential is resolved or a connection opened (dispatch
  `not-sent`); a call moved out of its runtime part-way is refused at its next wait on Tokio, with
  the dispatch it had reached. The clients still start no runtime; `docs/design.md` "Executor"
  records the measured behaviour and the rule.

## [0.8.2] - 2026-10-20

### Added

- `b10x-llm-http`: `QuotaRule`, `HttpClient::with_quota_rule`, `MAX_REFUSAL_BYTES` (16 KiB) and
  `REFUSAL_WAIT` (1 s). With a rule, the transport reads a 429's body, and only a 429's, up to
  `MAX_REFUSAL_BYTES` and for at most `REFUSAL_WAIT` within the exchange's deadline and the
  caller's cancellation, and asks the rule whether it reports an exhausted quota. The transport
  knows no provider's error document; a response tap is still shown the head alone.

### Changed

- An HTTP 429 that reports an exhausted quota, spend limit or subscription usage limit is now
  final: `refused`, dispatch `rejected`, not retriable, the class the same document already had
  inside a Responses stream, so routing ends the run instead of retrying and falling back.
  `b10x-llm-responses`: a 429 whose body's `error.code` is `insufficient_quota`,
  `credit_balance_exhausted`, `organization_spend_limit_exceeded`, `project_spend_limit_exceeded`
  or `organization_usage_limit_exceeded`, or whose `error.type` is `insufficient_quota`,
  `usage_limit_reached` or `usage_not_included` (the Codex backend's subscription limits), as the
  official Codex CLI treats them (`openai/codex` 8196bcdc,
  `codex-rs/codex-api/src/api_bridge.rs:185`-`238`). `b10x-llm-messages`: a 429 whose body's
  `error.type` is `rate_limit_error` and whose `error.details.error_code` is
  `enforced_spend_limit_reached`, Anthropic's tier spend cap. Every other 429 (a
  `rate_limit_error` without that code included, with or without `retry-after`), a Chat
  Completions 429, and a 429 whose body is larger, slower or not JSON stay a retriable
  `rate-limited` (`docs/design.md` "Provider failures retried before output").
- `b10x-llm-http`: an error status whose answer carries `x-should-retry: false` is not retriable,
  whatever its status; code, dispatch and retry hint are unchanged. It is the header the official
  OpenAI and Anthropic SDKs obey ahead of their status rules (lines and commits in
  `docs/design.md`, with the two cases two SDKs decide first). It only narrows:
  `x-should-retry: true` never makes a final status retriable. The Responses, Messages and Chat
  Completions clients all read it, so routing neither retries such a failure nor, for a 502 with
  dispatch `unknown`, falls back. This closes the 0.8.0 "Known" risk for a failure before output
  once llm-gateway sends `x-should-retry: false` on the 502 it answers after its own retries are
  spent; until it does, its 502 stays retriable. A failure a gateway relays inside a stream still
  reads back by the receiving decoder's rules.

### Fixed

- `b10x-llm-messages`, `b10x-llm-chat`: a stream cut after output the caller was not shown is
  final, as on Responses. The transport marks a cut retriable (a body that ends inside an event,
  or fails in transit); `MessagesClient` and `ChatClient` now drop that mark once their decoder
  has answered. Messages counts any payload but `message_start` and `ping`, so a block that
  opened empty counts. Chat Completions counts non-empty `content` or `reasoning_content`, any
  `tool_calls` fragment, and a finish reason; the chunk that opens the stream (`role` with an
  empty or null `content`) does not. Before this, routing retried such a turn on its target. A
  cut before output keeps its retry class (`docs/design.md` "Provider failures retried before
  output").

## [0.8.1] - 2026-10-20

### Fixed

- `b10x-llm-messages`: a Messages response carrying `container` is no longer refused. 0.8.0
  refused a live streamed turn with `Unsupported` ("Messages field is outside the declared subset:
  message_start.message.container"). `decode_message` and `StreamDecoder` accept `container` on a
  message, on `message_start.message` and on `message_delta.delta` as `null`, or as an object
  holding exactly `id` and `expires_at` (strings) and `skills` only as `null`, and do not consume
  it: Anthropic documents it as the container a container tool ran in, and llm declares no such
  tool. Any other shape is refused as `protocol` without its value.
- `b10x-llm-messages`: more fields Anthropic's API reference documents are accepted in the value
  the route reports when the request asked for nothing that fills them: `diagnostics` on a message
  (`null`), `stop_details` on a message and on `message_delta.delta` (`null`), `usage.speed`
  (`null` or `standard`), and on a `tool_use` block `toolset_name` (`null`) and `caller`
  (`{"type":"direct"}`, the documented default). Any other `diagnostics`, `stop_details`, `speed`
  or `toolset_name` value is refused as `protocol` rather than dropped; a `caller` other than
  `direct` stays `unsupported`. The block reader is shared with ingress, so a request's assistant
  `tool_use` carrying either is accepted there too.

## [0.8.0] - 2026-10-20

### Added

- `b10x-llm-core`: `Capabilities` declares `output_streaming` (the model's text reaches the sink
  as it is generated) and `input_during_output` (input can reach a turn still producing output).
  `Capabilities::text` and every client in this workspace declare `true` and `false`;
  `with_output_streaming` and `with_input_during_output` set them. In `llm.binding/1`,
  `llm.catalog/1` and `llm.routes/1` documents both are optional, default `true` and `false`, and
  are not written at their defaults, so a document written before them keeps its bytes, its
  binding revision and its configuration digest.
- `Capabilities::validate_protocol(Protocol)`: refuses `input_during_output: true` ("no protocol
  in this workspace carries input during output") and `output_streaming: false` ("every protocol
  in this workspace streams its output"). `BindingDocument::bind` (and so every catalog) and
  `Routes::parse` call it; a routes refusal names the route. `Capabilities::validate` does not.
- `b10x-llm-core`: `SHOWN_TEXT_KEPT` and `SHOWN_TEXT_UNPAIRED`. `StreamEvent` and `Model::turn`
  document that every event is final and that a successful turn's assistant text is the text
  deltas shown for it (`docs/design.md` "Streamed events are final").
- A call the caller rejected before running it (a policy, an approval, a bad argument) is sent as
  `failed: true` with the reason as the caller's text in `output`. No wire in scope carries a
  separate rejected state, and `Item::ToolResult` is unchanged (`docs/design.md` "Failed and
  rejected tool results", with what each wire sends for a failure and how each ingress reads it).
- 15 conformance scenarios (883 in all; baseline floors 868 → 883). Six for the streaming
  declarations: both refusals at bind, in a catalog and in a routes document naming the route,
  and a catalog writing the defaults out keeps its pinned digest and revision; the `llm.routes`
  capabilities projection reports both declarations. Seven for shown text:
  `kept-text-a-terminal-message-under-the-same-id-gives-way-to-the-shown-deltas`,
  `kept-text-a-streamed-message-gives-way-to-the-shown-deltas`,
  `kept-text-deltas-naming-no-item-at-two-indexes-stay-two-messages`,
  `kept-text-a-message-no-delta-showed-keeps-its-own-text`,
  `kept-text-shown-text-more-than-one-message-could-be-is-carried-in-none`,
  `kept-text-deltas-naming-no-coordinate-belong-to-the-message-done-next` and
  `a-text-block-that-opens-with-text-shows-it-as-a-delta`. Two authored Chat ingress scenarios for
  the failure object.

### Changed

- **Breaking for struct literals:** `Capabilities` has two more public fields, so a full
  `Capabilities { … }` literal no longer compiles. Write
  `Capabilities { tools: true, …, ..Capabilities::text(context_window, max_output_tokens) }`, or
  set both fields. Known consumer literals: Loom
  `loom-intake-router/tests/adversary_classify.rs:49`, `loom-intake-router/tests/classify.rs:42`,
  `loom-plugin/tests/support/mod.rs:277`, `loom-plugin-slack/tests/support/mod.rs:266`;
  llm-gateway `llm-gateway-cli/src/translate.rs:226`; the Harness spike
  `harness-cli/src/llm_port.rs:286`. Literals that already use `..Capabilities::text(…)` are
  unaffected.
- `b10x-llm-models`: a string route's default capabilities are stated per field (tools, tool
  choice, temperature and `top_p` allowed; output streamed; no input during output), not as "every
  boolean true".
- `b10x-llm-responses`: a streamed turn's assistant text is the text deltas the caller was shown.
  Shown text is paired with exactly one message: by `id`; for deltas naming neither an item nor
  an index, the message whose `response.output_item.done` came next; the `output_index` the stream
  named (a terminal message takes the index of the streamed message it is); the terminal
  position; equal text; then by elimination. A message the server finished with other text takes
  the shown text, and `ResponsesClient` emits one `StreamEvent::Warning` `shown-text-kept`. Shown
  text more than one message could be overwrites none: the server's messages stay as sent and the
  client emits `shown-text-unpaired`. Both come after the turn passed its checks, before
  `advisory-omitted`, never for a refused turn; `decode_stream` ends its events with them. Before,
  the server's message outranked the deltas. The scenario
  `kept-text-a-terminal-message-outranks-the-shown-deltas` is renamed
  `kept-text-a-terminal-message-gives-way-to-the-shown-deltas` and asserts the shown text and the
  warning.
- `b10x-llm-messages`: text a `text` block opens with in `content_block_start` is shown as a
  `StreamEvent::TextDelta` when the block opens; it was folded into the turn without being shown.
- `b10x-llm-responses`: a provider failure with code `server_error` (`Unavailable`) or
  `rate_limit_exceeded` (`RateLimited`), whether a top-level `error` or inside `response.failed`,
  is now retriable before any output and final after it, the rule `server_is_overloaded` already
  followed. Inside a stream an exhausted quota or spend limit (`insufficient_quota` and the other
  billing codes) stays a final `Refused`; as an HTTP 429 before the stream it is still the
  transport's retriable `RateLimited` (`story:quota-429-is-final`).
- `b10x-llm-responses`: `response.queued` is in `ACCEPTED_STREAM_EVENTS`. It was already a
  lifecycle payload for the retry rule, but it reached the caller as an `unknown-stream-event`
  warning and an opaque item; it is now ignored like `response.in_progress`.
- `b10x-llm-messages`: a stream `error` of type `overloaded_error` (`Unavailable`) or `api_error`
  (`Transport`) is now retriable when no payload but `message_start` and `ping` came before it,
  and final after any other, a block that opened without showing anything included. Every other
  type stays final, `rate_limit_error` included. Harness's wire crates retried these four classes
  (rows R28 and M27 of `docs/harness-parity.md`); the provider documentation for each is in
  `docs/design.md` "Provider failures retried before output". No wire field carries a gateway's
  retry decision, so a downstream llm client applies its own rules: a gateway's `fail` of these
  classes reads back retriable when it streamed nothing first, whatever the gateway decided, and
  llm's routing may call the gateway again after it spent its retries
  (`story:relayed-failures-keep-their-retry-decision`).

### Fixed

- `b10x-llm-chat`: gateway ingress reads a `tool` message whose content parses to exactly the
  failure object `{"error": <output>, "ok": false}`, which the Chat projection itself sends for a
  failed result, as `failed: true` with the `error` value as the output. It read every tool
  message as succeeded, so a failed result `project_request` sent decoded back as a success. Any
  other content, JSON or not (`{"ok": false}` alone, an object with a third key or a repeated
  `error` or `ok` key, `ok` true), is still a successful result carrying the content as a string,
  byte for byte. Content is parsed only when it opens with `{`, fits the 256 KiB tool-result bound
  and is at most 127 levels deep; a failed output nested deeper than 126 levels reads back as a
  successful string. On Responses, as before, any output nested that deep, failed or not, reads
  back as the envelope's text.

### Known

- The Responses ingress encoder still refuses `close` on two messages streamed back to back with
  nothing between them (`story:ingress-block-boundaries`); pinned by
  `two_messages_shown_back_to_back_are_refused_by_the_relay_until_ingress_block_boundaries`.
- A client on 0.8.0 calling an llm-gateway may retry a failure the gateway already retried: the
  four newly retriable classes read back retriable when relayed with nothing streamed first, and
  no wire field carries the gateway's decision (`story:relayed-failures-keep-their-retry-decision`,
  next wave).

## [0.7.0] - 2026-10-17

Every field a coding-agent client sends now has one written rule: carried, translated, advisory
with a reported omission, or refused by name; no key at any level is passed over
(`docs/design.md` "Client request fields"). Evidence: llm-gateway's authored Claude Code first
turn, and Codex 0.160.0 requests captured against a loopback recorder (no paid call).

### Added

- `b10x-llm-core`: `TurnRequest.controls` (`Controls`): `thinking` (`Thinking::Adaptive`,
  `Enabled { budget_tokens }`, `Disabled`), `parallel_tool_calls`, and advisory `hints` (`Hints`:
  `prompt_cache` with an optional `5m` or `1h` lifetime, `prompt_cache_key` of at most 512 bytes,
  `verbosity`, `reasoning_summary`, and `unforwarded` names of fields no wire sends). `wire_effort`
  is the one reasoning table; `Advisory`, `Hints::omitted_by`, `advisory_warning` and
  `field_path`. A turn with controls is the envelope `llm.turn/4`; one without is `llm.turn/3`,
  byte for byte as before.
- Each protocol crate: `omitted_advisories(&TurnRequest)`, the hints its projection does not
  send, and its client emits one `advisory-omitted` warning naming them after a successful turn.
- `b10x-llm-messages`: ingress carries `thinking`, a `system` of text blocks (joined by a line
  feed), `cache_control` on system blocks, text, `tool_use` and `tool_result` blocks and tools (the
  prompt-cache hint; two lifetimes refused), effort `max` and
  `tool_choice.disable_parallel_tool_use`; `metadata.user_id` is an unforwarded name. Egress
  re-emits thinking and the parallel choice and gives its breakpoints a `1h` lifetime when asked.
  `thinking.display`, `context_management`, `safeguards`, `output_config.format` and a `system`
  message inside `messages` stay refused by name.
- `b10x-llm-responses`: ingress carries `instructions` (followed by the leading developer
  messages' parts), `parallel_tool_calls`, `prompt_cache_key`, `text.verbosity` and
  `reasoning.summary`; item `id`s and `client_metadata` are unforwarded names; a
  `function_call_output` string that is not the pinned envelope is a successful result carrying
  that string. Egress re-emits them and refuses a conversation whose identifier differs from the
  turn's `prompt_cache_key`. Fixtures: Codex 0.160.0's first and second turns, captured against a
  loopback recorder and sanitized.
- `b10x-llm-chat`: `parallel_tool_calls`, `prompt_cache_key` and `verbosity` are carried both
  ways.
- 91 conformance scenarios (868 in all; baseline floors 868), and a cross-wire test
  (`checks/conformance/tests/client_turns.rs`) that projects each client's first turn onto the
  other two wires.

### Changed

- `TurnRequest::validate_for` checks `reasoning_efforts` against the effort the wire receives:
  effort `max` reaches Chat Completions and Responses as `xhigh`, disabled thinking as `none`, and
  a thinking budget is refused there.
- Every unknown key in a Chat Completions or Responses request is refused naming its path, at
  every level; 0.6.0 passed over keys below the top level. The top-level refusals now name the
  field too: `chat request field is outside the published subset: <path>` and `request body
  carries a field outside the pinned subset: <path>`. A Chat `strict` function schema is refused
  instead of dropped, and a hosted or custom Responses tool is refused naming its type.
- The shared source gates workflow runs Gates 0.1.21 instead of 0.1.3: its scan limit is 512 MiB
  instead of 256 MiB, and it reads the compressed policy secret `B10X_GATES_POLICY_GZ` first.

## [0.6.0] - 2026-10-16

### Added

- `b10x-llm-models`: model routes, format `llm.routes/1` (YAML, module `llm_models::routes`).
  `Routes::parse` reads `endpoints` (base URL, protocol `messages`, `responses` or
  `chat-completions`, and `auth` of kind `anonymous`, `bearer` or `api-key` whose secret is only a
  reference: `env:<VARIABLE>`, `file:<path>` or `keychain:<service>/<entry>`) and `routes` (a name
  for `<endpoint>/<model-id>`, split on the first `/`, or a mapping with its own `capabilities`).
  A name that is not a route but reads `<declared endpoint>/<model-id>` resolves directly. The
  document expands into an `llm.catalog/1` `Catalog`, so `port` and `CatalogModels::build` build
  its ports unchanged; `Routes::port` builds one for any name. `Routes::resolvers(&SecretSources)`
  builds `RouteResolvers` (an `llm_models::Resolvers`) from the references through the existing
  resolvers, behind the new default-off features `environment`, `file` and `keychain`; nothing is
  read when they are built, and an unset variable or a missing file is refused `unauthorized` by
  the endpoint's reference before any request is sent, with no fallback. Parsing does no I/O and
  refuses unknown fields, repeated keys, an unknown format, protocol, auth kind or secret source,
  a literal secret and a route to an undeclared endpoint, naming the field and echoing no value;
  YAML aliases are bounded so a small document cannot cost more than its length allows. Routes
  without capabilities take a default (tools, tool choice, temperature and `top_p` allowed;
  efforts `low`, `medium`, `high`; context 200000; output 16384). ESS domain `llm.routes` and 24
  scenarios.
- `b10x-llm-routing`: `CatalogDocument::new`, so a catalog can be built from declarations.
- `b10x-llm-responses`: the answer half of gateway ingress. `encode_ingress_response` and
  `IngressStream` (`new`, `chunk`, `close`, `fail`) turn neutral events and a `TurnOutcome` into the
  Responses event payloads and response object this crate's `decode_stream` reads back as the same
  turn: text, function calls, reasoning, every stop reason, only reported counters, one caller
  identifier per answer, and `response.failed` without the error's message or a partial usage
  snapshot. ESS command `llm.responses.EncodeIngress` and 14 scenarios.
- `b10x-llm-messages`: the same for Messages. `encode_ingress_message` and `IngressStream`
  (`new(id, model)`, `chunk`, `close`, `fail`, `without_reasoning_relay`) emit `message_start`,
  `content_block_*`, `message_delta` with the stop reason and reported usage, `message_stop` and
  `error`, each checked against this crate's decoder before it is sent. ESS command
  `llm.messages.EncodeIngress` and 11 scenarios.

### Known limits

- `StreamEvent` carries no item boundary, so the Responses encoder refuses two consecutive messages,
  or one message another item interrupts, and the Messages encoder refuses redacted thinking between
  streamed blocks (story `ingress-block-boundaries`). A streamed Messages answer names a model only
  when one is announced at `IngressStream::new`.

### Conformance

- The suite answers 777 scenarios and skips none (baseline floor 705 → 777).

## [0.5.1] - 2026-10-08

### Documentation

- The status page lists the Secrets resolver (0.3.0), the catalog model port (0.4.0), the provider
  description (0.5.0) and the written contract as shipped; it no longer lists the gateway cutover
  or an operator command line as llm's planned work.
- Pages, README.md and AGENTS.md link llm-gateway's repository instead of describing
  serving; the index and landing page name the catalog model port, provider
  descriptions and the Secrets adapter; Harness and AEP links point at their ecosystem pages.

## [0.5.0] - 2026-10-08

### Added

- `b10x-llm-providers`: provider descriptions, format `llm.provider-description/1` (TOML, at most
  64 KiB, unknown fields refused). `ProviderDescription::parse` reads a provider, an inference
  base-URL template holding `{instance}` once in the host's first label, followed by a fixed
  domain, its wires and
  authentication, and an optional control plane: a pinned OpenAPI document by HTTPS URL and
  SHA-256 digest, an HTTPS server and four distinct operation IDs. `inference_base_url` refuses an
  instance that is not 1-48 bytes of `[a-z0-9]` before substitution. Parsing does no I/O. Runpod's
  description ships as `descriptions::runpod()`. ESS command `llm.providers.Describe` and 21 new
  conformance scenarios (baseline floor 704).

## [0.4.0] - 2026-10-08

### Added

- `b10x-llm-models`: `port` builds the `Model` a catalog serving model declares, the binding's
  protocol selecting `ChatClient`, `ResponsesClient` or `MessagesClient` and the account selecting
  the caller's resolver; a credentialed account without one is refused `unauthorized` before any
  I/O. `CatalogModels` builds every serving model's port and implements `llm_routing::Models`.
  `Catalog::bindings` lists the validated bindings. ESS domain `llm.models`, ten new conformance
  cases (702 in all; baseline floor 684).

### Changed

- ESS 0.56.0: the CI `ess` pin and the `ess-conformance` and `ess-primitives` crates.

## [0.3.1] - 2026-10-07

### Fixed

- Responses `server_is_overloaded` errors are temporary `Unavailable` failures eligible for
  caller-owned retries before output, instead of request refusals. Any prior output payload makes
  the failure final, including an opening item without a visible event. Unknown error codes and
  actual refusals remain final; provider prose is never included in diagnostics.

### Changed

- ESS moves to 0.55.0: CI installs the `ess` 0.55.0 release asset after checking it against the
  release's `SHA256SUMS`, and the conformance runner takes `ess-conformance` and `ess-primitives` at
  tag `0.55.0`. The regenerated suite differs only in its provenance (`suite_version`
  `ess-conformance/35`, `scenario_initial_state: empty`); its 691 scenarios, its coverage and the
  schemas are byte-identical.

## [0.3.0] - 2026-10-07

Anthropic subscription access over Messages, and credentials resolved through the secrets library.
No provider route is qualified yet; every guarantee is still held against fixtures.

### Added

- Anthropic subscription access over Messages. `llm-core`'s `AuthKind` gains `SubscriptionOauth`
  (`subscription-oauth`): the caller's own subscription token, resolved through the account's
  `SecretRef` on every turn and never read from a file, presented as a sensitive bearer. A binding
  refuses it on any billing but subscription and any protocol but Messages. For this kind only,
  `llm-messages` sends `anthropic-beta: oauth-2025-04-20` and opens `system` with the subscription
  client preamble (`docs/messages.md`). Parity rows C8, C21, M5, M6 and M44 are covered.
- `llm-routing`: a catalog refuses a secret reference shared between a subscription-oauth account
  and any other account, and a route with fallback that mixes a subscription target with a target
  under other billing, in either order; a subscription token is never presented as a metered
  bearer, and a turn never moves from subscription to metered billing.
- `llm-credentials` feature `secrets`: `SecretsResolver` resolves references by name in one scope
  of a `secrets` v0.5.0 storage (`SecretsResolver::new`) or of its keychain backend
  (`SecretsResolver::keychain`). Every storage error code maps to an existing `SecretError`; the
  version is the backend's version, hashed, or a content hash; a refresh re-reads and is
  `RefreshRejected` while the version is unchanged; a backend without `Read` is
  `UnsupportedPlatform` and is never read. No core crate depends on the library, and the boundary
  tests treat any crate from the secrets source as the library's.
- `llm-credentials`: the example `live_subscription_turn` (features `secrets` and
  `native-keychain`) runs one Messages turn over the operator's own subscription token, read from
  the platform keychain through the secrets library where `secretsctl put` stores it, and prints a
  JSON report: endpoint, auth and billing kind, header names, whether the OAuth beta and the client
  preamble were sent, stop reason, usage, latency or the typed error. `--rotate-check` runs a
  second turn after the token is replaced and reports whether the version changed. The report
  never carries the token. Its logic is tested on a mock store and a loopback fixture; no gate
  runs it live. `docs/live-qualification.md` gives the operator's steps.
- `live_subscription_turn --capture-response <path>` writes the response line, response headers
  and event stream the route sent to a new file, mode 0600 on Unix, and never a request header or
  the token; the report gives the bytes written. It rides on `llm-http`'s new `ResponseTap`
  (`HttpClient::with_response_tap`), which is shown streamed responses only: never the request,
  and never a JSON exchange such as a credential refresh.

### Changed

- `AuthKind` is not `#[non_exhaustive]`, so an exhaustive `match` on it needs a
  `SubscriptionOauth` arm.
- `llm-messages`: a field outside the declared subset is refused with its path, for example
  `Messages field is outside the declared subset: message_start.message.usage.<name>`. The
  name is copied only when it is a bounded name (`?` otherwise); a value never is.
- ESS moves to 0.52.0: CI installs the `ess` 0.52.0 release asset after checking it against the
  release's `SHA256SUMS`, and the conformance runner takes `ess-conformance` and `ess-primitives` at
  tag `0.52.0`. The regenerated suite (665 scenarios) and schemas are byte-identical. The
  conformance suite now runs 691 scenarios (secrets resolver, subscription access and its catalog
  refusals).
- `task rust` also runs clippy on the pinned 1.98.0, the toolchain CI uses.

### Documentation

- The documentation site publishes from this repository at `/llm/` (`pages.yml`,
  `b10x-docs-site.yml`); the generated unified-site bundle, check and redirect workflows and
  `b10x.docs.yaml` are gone.

## [0.2.0] - 2026-10-05

The serving side leaves llm: llm is the client libraries, and serving lives in
[beyond10x/llm-gateway](https://github.com/beyond10x/llm-gateway).

### Removed

- The serving crates moved to [beyond10x/llm-gateway](https://github.com/beyond10x/llm-gateway),
  with their tests and history: `b10x-llm-gateway`, `b10x-llm-provision`, `b10x-llm-runpod` and
  `b10x-llm-modal`. The `llm.gateway`, `llm.hosting` and `llm.runpod` ESS domains, their 156
  authored scenarios, `docs/gateway.md`, `docs/hosting.md` and their verification records moved
  with them. llm is the client side; none of its client crates depended on the serving crates, so
  no client API changes. The conformance suite ran 664 scenarios after the move (was 823); its floor is 664.

### Fixed

- `llm-messages`: a `thinking` block whose `content_block_start` carries no `signature` field is
  signed by its `signature_delta`, as Harness accepted; a block never signed and a signature delta
  into any other block are still refused. Found by building Harness on llm 0.1.7; one conformance
  scenario added (665).

### Documentation

- `docs/harness-parity.md`: rows R45 (a forced tool choice answered in prose stays refused,
  deliberately) and M47; row C1 is not needed (credentials come from the secrets library, no token
  files); the subscription rows name the decision to build Anthropic subscription access.

## [0.1.7] - 2026-10-05

Harness parity: `docs/harness-parity.md` maps every public item and test-pinned behaviour of
Harness's model crates to llm (146 rows: covered, partial, gap or deliberately different, each
cited).

### Added

- `llm-credentials`: feature `environment` (`EnvironmentResolver`, caller-named variables only)
  and feature `json-pointer` (`JsonPointerResolver`, RFC 6901 over another resolver). Refusals
  carry the secret reference, never its path, variable or value; `SecretError::Malformed` for a
  document that is not JSON or a target that is not a string.
- `llm-credentials` feature `codex-renewal`: `CodexAuthFile::renew` and `RenewingCodexAuthFile`
  renew a Codex login inside a 15-minute margin through one non-retried JSON POST
  (`HttpClient::post_json`, bounded by `MAX_EXCHANGE_BYTES`), write the token fields back in place
  through a same-directory temporary file with the original mode, refuse a file that changed during
  renewal, a symlink or a hard-linked file, never re-send a refused or uncertain grant for an
  unchanged file, and zeroize the request and the answer.
- `llm-routing`: same-target retry before any visible output (`RetryPolicy`, default 4 attempts,
  1/2/4/8 s back-off, server delay capped at 30 s), then fallback; a `turn-retried` warning before
  each wait; the wait races cancellation; the caller's limit runs before every attempt.
  `llm-core` `Error::retriable` and `may_retry`; `llm-http` marks 408, 429, 5xx, no response, a
  body failing mid-stream and end of stream inside an event as retriable.
- `llm-http`: `CONNECT_TIMEOUT` (15 s) and `HttpClient::with_connect_timeout`.
- `llm-responses`: `encode_request` (the exact request bytes the client sends), opt-in
  conversation identity (`Conversation`, `ResponsesClient::with_conversation`: `prompt_cache_key`,
  `session-id`, `x-client-request-id`) and `request_headers`.
- `llm-messages`: prompt-cache breakpoints on `system` and on the conversation tail; an unknown
  stream event, delta or content block is kept as an opaque item with a warning.
- New crate `b10x-llm-tool-call`: `call_tool` (one forced tool, returns its JSON input) and the
  Codex Responses preset.
- New crate `b10x-llm-blocking`: a blocking adapter over any `Model` for a synchronous loop.

### Changed

- `ResponsesClient` hands each event to the caller as it arrives; text the caller was shown
  through deltas stays in the turn when the terminal output omits it; `keepalive` is not an answer.
- A failure after any Responses output was decoded is final (never replayed).
- `Limits::default().idle` is 180 s (was 60 s).
- `prepare_auth` strips exactly one trailing line terminator from a token, and its refusal names
  the secret reference.
- `FallbackPolicy::disabled()` means one attempt in total.
- A misconfigured Codex login file is refused, not fallen back from: a login read whole that is not
  a Codex login document, or whose access token has no readable integer `exp`, is
  `SecretError::Malformed` (presented as `Unauthorized`, never falls back); a login that ends
  before its document does stays `Unavailable`.
- Breaking: `RenewalRefusal` has a new variant `Malformed` (code `malformed`).
- The workspace lints clean on rustc 1.99 as well as the pinned 1.98.0.

## [0.1.6] - 2026-10-04

### Fixed

- `ResponsesClient` completes a turn against the Codex backend. `llm-http` reads a 2xx with no
  `content-type` as an event stream when the request asked for `text/event-stream` (a 2xx naming
  another type is still refused), and the Responses decoder falls back to the streamed items when
  `response.completed` carries an empty `output` after items were streamed.

## [0.1.5] - 2026-10-04

### Added

- `llm-responses` has a client: `ResponsesClient` implements `Model` over the OpenAI Responses
  wire. It validates the request against its binding, bounds the projected body before the
  credential is resolved, stops at the first terminal event, validates the outcome, and attaches
  the best evidence decoded so far to every refusal after dispatch. All three protocol crates now
  have clients.
- `llm-credentials` feature `codex-auth-file`: `CodexAuthFile` resolves the access token of a Codex
  login's `auth.json` at an explicit absolute path, read-only and on every request. The JWT `exp`
  claim is judged against the caller's clock; renewal is running `codex`. Specified as
  `llm.catalog.CodexAuthFile`.

### Changed

- The AEP store is `aep.project/5`; CI validates it with aep 0.68.0.

## [0.1.4] - 2026-09-27

### Changed

- The specification has no open `UNMAPPED` markers. The gateway's route-count refusal and the
  transport's disabled ambient proxy are declared and covered by scenarios (700, up from 697);
  unbuilt work names its owning story, the gateway's single-owner scope is a recorded decision,
  and the constant-time owner comparison is noted as a property ESS 0.36 cannot express.

## [0.1.3] - 2026-09-27

### Added

- ESS domains `llm.runpod`, `llm.gateway`, `llm.providers` and `llm.transport`, each run by the
  conformance runner against the real crate: 697 scenarios, up from 523.

### Fixed

- The gateway refuses a header name followed by whitespace before its colon with `400
  request-malformed`, as RFC 9112 §5.1 requires; it was trimmed and authenticated.
- The gateway compares SHA-256 digests of the presented and expected owner secrets in constant
  time, so the expected secret's length is not observable through timing. The verifier keeps only
  the digest.

## [0.1.2] - 2026-09-27

### Changed

- The workspace is licensed under Apache-2.0, like the other beyond10x repositories. Releases up to
  0.1.1 remain under `LicenseRef-B10x-Proprietary`.

## [0.1.1] - 2026-09-27

### Changed

- The specification is at ESS source format `ess/14`; the conformance runner uses ESS 0.36.0 and
  CI installs ESS 0.36.0 and AEP 0.61.1.
- The conformance runner parses its command line with clap: `check`, or
  `run SUITE BASELINE OUTPUT_DIR SOURCE_IDENTITY` for one suite.
- `docs/design.md` and `docs/implementation-status.md` state the resolved hosting lifecycle and
  the stories shipped in 0.1.0.

### Added

- 61 authored conformance scenarios from a mutation audit and a refusal-reachability pass (the
  suite runs 523). `llm.secrets.ProbeFile` takes an optional `mode` fixture so each file and
  directory permission rule is reached on its own.

## [0.1.0] - 2026-09-27

### Added

- A mutation audit over the specification and implementation added 38 authored conformance
  scenarios (the suite runs 462); every killable mutant is killed.
- Ordered fallback in `b10x-llm-routing`: `run_turn` tries a route's compatible targets in declared
  order, bounded by attempts and deadline, and halts on visible output, ambiguous dispatch,
  incompatible opaque state, an ineligible failure, cancellation or a caller's limit refusal.
- The Runpod adapter in `b10x-llm-runpod`: single-flight startup, ordered GPU fallback, lost-create
  adoption by a per-controller request id, readiness and crash-window recovery, ownership-safe
  cleanup, proven against an in-process emulator.
- Streamed tool calls carry their announced name (`StreamEvent::ToolCallStarted`) in the Chat,
  Responses and Messages projections.
- Opaque continuation state ingress cannot attribute is carried as `Item::UnattributedOpaque` and
  is never sent until a caller binds it; the envelopes move to `llm.turn/3` and `llm.outcome/4`
  and older versions are refused by name.

- Neutral inference boundary in `b10x-llm-core`: an object-safe asynchronous `Model::turn` port
  taking one request, a caller-owned sink and a cancellation token; bounded text and tool items;
  typed failures with independent dispatch evidence; and opaque continuation state bound to its
  exact protocol, provider, account, endpoint, model and binding revision.
- Injected credential resolution in `b10x-llm-credentials`: an opaque `SecretRef`, request-time
  resolution, zeroized and redacted material with no serialization, and a coordinated resolver that
  refreshes only the credential generation actually rejected.
- Bounded single-attempt transport in `b10x-llm-http`: HTTP and SSE with explicit deadlines, no
  redirects, no automatic retries, and partial output preserved across failure.
- Provider bindings in `b10x-llm-providers`: independent protocol, provider, authentication and
  billing choices, including anonymous arbitrary endpoints.
- TOML catalog routing in `b10x-llm-routing`: strict `llm.catalog/1` parsing, deterministic
  configuration identity, ordered opt-in selection, capability and input-token admission, and a
  safe explanation that resolves no secret and performs no request.
- Attributed usage pricing in `b10x-llm-cost`: versioned `llm.prices/1` books, exact decimal
  arithmetic over `llm.usage/2` observations, `llm.cost/2` reports with six separate accounting
  bases, and unknown quantities preserved as unknown rather than zero.
- Optional local secret adapters behind the `file`, `keychain` and `native-keychain` features:
  explicitly mapped protected files on Linux and an explicitly injected or native credential store.
- Optional durable spending admission behind `b10x-llm-cost`'s `sqlite` feature: an immutable
  single-owner `BudgetPolicy`, reservations committed before dispatch, serialized concurrent
  callers, cross-process owner exclusion, uncertain charges retained across restart, and compute
  stop obligations.
- The Responses, Messages and Chat Completions protocol projections in `b10x-llm-responses`,
  `b10x-llm-messages` and `b10x-llm-chat`, each for its declared supported subset: independent
  authentication, billing and protocol choices survive translation; unsupported fields and
  provider-specific opaque continuation state are preserved or refused rather than dropped; and
  each adapter normalizes its wire's usage counts before producing neutral values, reporting a
  partial quantity only where it is a valid lower bound.
- An authenticated single-owner gateway in `b10x-llm-gateway`: nothing is decoded past the HTTP
  head before an owner verifier accepts, apart from a closed liveness and readiness surface; the
  route inventory is an immutable snapshot with no field for an endpoint URL, secret reference or
  credential material; and start, drain and stop are deliberate. It performs no protocol
  translation, proxies no model call, resolves no secret and reaches no network.
- The hosting lifecycle contract in `b10x-llm-provision`, with an in-process `FakeProvider` that
  demonstrates it: resource identity qualified by the provider's `incarnation` so a recycled name
  cannot be adopted as the same resource, requested state kept strictly separate from observed
  state, and a stop obligation that nothing but evidence discharges. It opens no socket, reads no
  credential and allocates no cloud resource.
- Executable ESS verification for the `accounting`, `budget`, `chat`, `hosting`, `inference`,
  `messages`, `responses`, `routing` and `secrets` domains — every domain except `catalog`, which
  declares rather than observes — with recorded falsification evidence under `docs/verification/`.
  Two naming notes: the suite directory `contracts/pricing/` exercises the `accounting` domain and
  there is no `pricing` domain; and `gateway-auth` has a falsification record but no ESS suite.
- Public documentation surface: a `b10x-docs/v4` manifest, a Docusaurus documentation tree under
  `website/`, this changelog, a proprietary licence notice, the shared Gates caller workflow and a
  `b10x-change/v1` feed entry.

### Changed

- The specification is at ESS source format `ess/13` and the conformance runner uses ESS 0.35.0;
  emitted payload fields the adapters observe are declared `generated`.
- The planning store is `aep.project/3` on a tree Git merges, planned with AEP 0.60.0; CI installs
  AEP 0.60.0.
- The workspace version is 0.1.0.
- Pin every action revision in `.github/workflows/gate.yml` to an exact commit and name the
  version beside it. Common Gates refuses a workflow whose action revisions are not exact commits
  or Docker digests, so the repository's own gate had to be pinned before the shared caller could
  be enabled. No step, trigger, permission or toolchain version changed: `1.98.0` moved from the
  `dtolnay/rust-toolchain` ref to its `toolchain:` input, which is how the action takes it once
  the ref is a commit.

### Fixed

- Keep transport deadline and dispatch evidence deterministic.
- Release the spending ledger's owner lock after connection close.
- Compile the spending ledger's directory synchronization only on Unix.
- Preserve bound inference observations and partial usage costs.

### Not yet implemented

The operator command line (`llm-cli`), the Modal hosting adapter (`llm-modal`) and protocol
translation in the gateway are not written yet. OpenAI and Anthropic access is unqualified: no live provider credential has been used
anywhere in this repository, so every guarantee above is held against fixtures, local sockets and
in-process fakes. No crate is published to a registry.
