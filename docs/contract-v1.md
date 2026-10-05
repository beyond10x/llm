# Neutral inference contract (unreleased revision 2)

This is the first LLM contract. It is independent of Harness execution authority and supports
stateless text and tool turns. Image/audio, vendor-side tools, provider thread management and
unrecognized request settings are outside this version and must be refused by adapters.
The ESS declaration vocabulary is in `spec/domains/catalog.yaml`.

## Turn ownership

`Model::turn` is an object-safe asynchronous port taking one request, a caller-owned asynchronous
sink and cancellation token. Its future borrows those values. Dropping the future cancels local
work and releases its response; it cannot prove that an upstream did not accept or bill a request.
A client performs one attempt. Routing owns retries, alternate accounts and fallback decisions.
Sink emission is awaited, allowing bounded backpressure. Sink failure terminates the attempt.
Cancellation is monotonic, cloneable, wakes pending work and wins over simultaneous completion.

An `Item` contains user/assistant text, a tool call, its successful or failed result, or an opaque
provider item. Tool definitions contain only name, description and JSON Schema. They confer no
execution permission; LLM never executes a tool, carries an approval envelope or imports an agent
loop. A caller supplies tool results in a subsequent request.

Opaque items carry protocol, provider, account, endpoint, model and binding revision. Every
adapter checks all six against the selected binding before sending, including same-protocol
cross-model and cross-account requests. Providers may later admit a narrower documented exception
through a versioned contract; no implicit exception exists. Unsupported opaque state never
disappears during translation or fallback.

An ingress surface may carry opaque state it cannot attribute. A request body names no binding,
so a gateway reading one holds such state as an unattributed opaque item: the payload as a JSON value
(JSON-equal to what arrived, not byte-equal) and the protocol it was read from, never a binding. That item is not sendable: every egress
path and `TurnRequest::validate_for` refuse it by name (`Item::UNATTRIBUTED_REFUSAL`) until a
caller binds it. Route selection refuses it as `opaque-state`, the same rejection as state bound to
another binding, and does not tell the two apart; a caller does that before routing, with
`TurnRequest::validate_for` or by binding it with `TurnRequest::bind_unattributed`.
Binding it to a target is an explicit caller decision (`TurnRequest::bind_unattributed`), refused
for a target of another protocol; no adapter makes it, and ingress never stamps the reading binding
onto what it read. Provenance is not widened with a sentinel revision and the six-coordinate
comparison is never partial.

The binding revision hashes the complete validated single-binding declaration, including the
endpoint URL, upstream model, auth reference and capabilities. Repointing an existing ID therefore
invalidates old opaque state. Secret bytes and credential generations are excluded so rotation of
the same reference does not invalidate a continuation.

## Validation and accounting

Absent sampling fields remain absent. Unsupported settings refuse before network I/O. Identifiers
and individual fields are bounded; the whole serialized request is bounded as well. The request
model must match the selected target. Tool choices must name published tools. Core validates
structure and declared capabilities, not a tokenizer or the truth of an operator's capability
declaration. Context-window admission needs the routing/token-accounting implementation and is
not proved by request byte limits.

All usage fields are optional independently. Input tokens include the disjoint cache-read and
cache-write subsets, and output tokens include the reasoning subset. Unknown is not zero.
Contradictory known totals refuse rather than saturating a negative remainder. Adapters normalize
their wire's counts before producing these values; pricing is separate.

Every successful outcome has a `TurnObservation` pinned to its selected immutable binding. Its
upstream model and response ID are independently optional provider observations; the configured
model and caller alias cannot fill absent evidence. `final_usage` says reported counters are
terminal, not that every count is known or an invoice authenticated. Successful outcomes require
terminal evidence. Failures optionally retain the last valid snapshot in a boxed observation;
that snapshot may be partial or final. Validation refuses foreign binding coordinates,
contradictory usage, and upstream evidence attached to a `not-sent` failure. Adapters must attach
valid evidence when reporting cancellation, transport, protocol or sink failure after dispatch.
The core validates these values but cannot recover facts an adapter discarded.

Errors distinguish invalid input, transport, protocol, authentication, rate limits, refusal,
bounds, unsupported semantics, cancellation and deadlines. Dispatch evidence is independent:
`not-sent`, `rejected`, `unknown`, or `accepted`. A transport failure after dispatch is unknown,
not proof of a free retry. Error diagnostics never include authorization headers, request bodies
or arbitrary upstream error text.

Every error also carries a retry class, `retriable`, set by the producer that observed the
failure and separate from dispatch: dispatch is never rewritten to make a failure retriable. The
HTTP transport marks 408, 429 and 500-599 (529 included), a request that got no response, a body
that failed while streaming, and an end of stream inside an event. Deadlines, cancellation,
malformed frames, redirects and every other status (400, 401, 403 and 409 included) are final.
`Error::may_retry` acts on the class only for transport, rate-limited, unavailable and protocol
codes, so an unauthorized or refused failure is never retried whatever its mark says.

Routing retries one target before any output is visible, and only then. A failure that
`may_retry` and put nothing on the caller's sink is attempted again on the same target, by
default up to four attempts with waits of 1, 2 and 4 s (8 s at most; `RetryPolicy`), each wait
stated on the sink as a `turn-retried` warning and raced against cancellation. A server delay is
honoured up to 30 s inside that wait and never shortens it; on its own it is never a reason to
retry. Once the target's attempts are spent the run falls back to the next declared target, and
the final error goes up no longer retriable, naming the attempt count. Such a retry or fallback
may replay a request whose dispatch is `unknown` or `accepted`: that is deliberate for these
classes, and it never assumes the attempt was free. Each attempt is recorded with its own dispatch
evidence, so a possibly billed attempt stays `unknown` and its spend is recorded, and the caller's
limit decision is consulted before every attempt, retries included. A failure without the class
keeps the earlier rule: it may fall back only with `not-sent` or `rejected` dispatch, and is
never attempted again on the same target. A wait that would end at or after the caller's
deadline is not taken: the target counts as spent and the run falls back if a declared target
can still start. A caller that has cancelled is not told a retry is coming and no wait is asked
for. `FallbackPolicy::disabled()` is one attempt in total, with neither fallback nor a
same-target retry.

"Visible" is what reached the caller's sink, so a client that reads its stream before handing
events over must decide the class itself: the Responses client marks a cut stream final once the
provider produced any output item or delta, whether or not the caller has seen it, and keeps it
retriable only when nothing but lifecycle events (`response.created`, `response.in_progress`,
`response.queued`) arrived. The Chat Completions and Messages clients hand each event over as
it is decoded, so routing counts their output directly.

## Pricing

The separate pricing library accepts versioned `llm.prices/1` JSON/TOML and `llm.usage/2`
observations and emits `llm.cost/2`. Exact nonnegative amounts, explicit units and checked arithmetic
preserve unknown quantities and failed attempts. Reference usage valuations, metered/compute
estimates and recorded charges have separate totals. [Pricing](pricing.md) defines the complete
current contract; live billing qualification remains separate.

The optional SQLite budget journal uses `llm.budget/1` with immutable policy and replay-checked
commands/results. It commits before a one-shot dispatch receipt, preserves uncertain charges and
compute stop obligations, and refuses unsupported or inconsistent persisted data without reset.
[Budgets](budgets.md) defines scope, settlement, concurrency, ownership and storage boundaries.
Derived inspection views and in-process commands are not independently versioned vendor wires.

## Credentials
`SecretRef` is a validated opaque name, not a value or backend selection. `SecretResolver`
resolves at request time and may implement caller-owned renewal. Secret values are zeroized on
drop, redacted in Debug, and have no serialization or Display implementation. LLM never searches
ambient vendor directories, runs a login flow or writes credential files; the only paths,
environment variables and JSON pointers it reads are ones the caller names for a reference. A
coordinated resolver serializes resolution/refresh per reference and refreshes only the credential
generation actually rejected, preventing concurrent callers from refreshing the same generation
repeatedly.

Optional [local adapters](local-secrets.md) implement the same reference contract. File protection
checks currently support Linux; native-keychain constructors also support macOS and Windows.
These read-only sources identify exact content rather than issuer generations and return
`RefreshUnsupported`; independently replaced bytes are visible on the next resolve. Returned
material and content identity remain redacted and nonserializable.

Anonymous accounts require an absent reference; authenticated modes require a present reference.
Billing kind (metered, subscription, self-hosted) is independent of protocol and authentication
presentation. A rejected subscription credential never changes billing kind or account.

## Compatibility and source provenance

Published Rust APIs follow the crate release's semantic version. Persisted neutral requests carry
an explicit `llm.turn/3` envelope and outputs carry `llm.outcome/4`; old/unknown versions and fields refuse. Both moved when `Item` gained the
unattributed opaque variant, so a `llm.turn/2` or `llm.outcome/3` reader is never handed one. Unversioned Rust
values are in-process values, not a claim of a stable vendor wire format. Unsupported additions
need a version change before they are accepted. This contract does not establish any live provider
qualification or a released artifact.

Provider declarations use `llm.binding/1`; TOML catalogs use `llm.catalog/1`. Catalog selection
requires an input-token upper bound supplied by the caller and valid for every candidate. Unknown
input counts refuse admission. Fallback defaults off; opt-in permits local selection of the first
compatible named target. Same-target retry is on by default and described under errors above;
integrating pricing/budget admission with those attempts remains separate planned behavior.
Explain exposes safe IDs, capabilities and refusal reasons;
it does not resolve secrets or include prompt/opaque payloads.

Source port: `beyond10x/harness` commit `709a2ebadcc14602b82b6f3c240350e4ddc1c88c`,
`crates/harness-wire/src/{id,bound,item,turn,port}.rs` and
`crates/harness-http/src/{sse,status,transport}.rs`. LLM preserves the validated identifier,
field-bound, text/tool and terminal-truth semantics; it removes tool execution authority,
replaces wire-only opaque provenance, preserves every unknown usage field, and exposes an async
single-attempt transport. The Harness retry classes and retry policy (`harness-http/src/
{status,retry,witness}.rs` at `2fd7235b`) live in routing instead of the transport.
