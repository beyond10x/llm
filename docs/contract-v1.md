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
not proof of a free retry. A server delay is a hint, not permission to retry. Error diagnostics
never include authorization headers, request bodies or arbitrary upstream error text.

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
ambient vendor directories, runs a login flow or writes credential files. A coordinated resolver
serializes resolution/refresh per reference and refreshes only the credential generation actually
rejected, preventing concurrent callers from refreshing the same generation repeatedly.

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
compatible named target. Retrying after an HTTP failure and integrating pricing/budget admission
with those attempts remain separate planned behavior. Explain exposes safe IDs, capabilities and refusal reasons;
it does not resolve secrets or include prompt/opaque payloads.

Source port: `beyond10x/harness` commit `709a2ebadcc14602b82b6f3c240350e4ddc1c88c`,
`crates/harness-wire/src/{id,bound,item,turn,port}.rs` and
`crates/harness-http/src/{sse,status,transport}.rs`. LLM preserves the validated identifier,
field-bound, text/tool and terminal-truth semantics; it removes tool execution authority,
replaces wire-only opaque provenance, preserves every unknown usage field, and exposes an async
single-attempt transport instead of carrying the Harness retry policy into the new boundary.
