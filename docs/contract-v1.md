# Neutral inference contract v1

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

Opaque items carry protocol, provider, account, endpoint identity and model provenance. Every
adapter checks all five against the selected binding before sending, including same-protocol
cross-model and cross-account requests. Providers may later admit a narrower documented exception
through a versioned contract; no implicit exception exists in v1. Unsupported opaque state never
disappears during translation or fallback.

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

Errors distinguish invalid input, transport, protocol, authentication, rate limits, refusal,
bounds, unsupported semantics, cancellation and deadlines. Dispatch evidence is independent:
`not-sent`, `rejected`, `unknown`, or `accepted`. A transport failure after dispatch is unknown,
not proof of a free retry. A server delay is a hint, not permission to retry. Error diagnostics
never include authorization headers, request bodies or arbitrary upstream error text.

## Credentials

`SecretRef` is a validated opaque name, not a value or backend selection. `SecretResolver`
resolves at request time and may implement caller-owned renewal. Secret values are zeroized on
drop, redacted in Debug, and have no serialization or Display implementation. LLM never searches
ambient vendor directories, runs a login flow or writes credential files. A coordinated resolver
serializes resolution/refresh per reference and refreshes only the credential generation actually
rejected, preventing concurrent callers from refreshing the same generation repeatedly.

Anonymous accounts require an absent reference; authenticated modes require a present reference.
Billing kind (metered, subscription, self-hosted) is independent of protocol and authentication
presentation. A rejected subscription credential never changes billing kind or account.

## Compatibility and source provenance

Published Rust APIs follow the crate release's semantic version. Persisted neutral requests carry
an explicit `llm.turn/1` envelope and outputs carry `llm.outcome/1`; unknown versions and fields refuse. Unversioned Rust
values are in-process values, not a claim of a stable vendor wire format. Unsupported additions
need a version change before they are accepted. This contract does not establish any live provider
qualification or a released artifact.

Source port: `beyond10x/harness` commit `709a2ebadcc14602b82b6f3c240350e4ddc1c88c`,
`crates/harness-wire/src/{id,bound,item,turn,port}.rs` and
`crates/harness-http/src/{sse,status,transport}.rs`. LLM preserves the validated identifier,
field-bound, text/tool and terminal-truth semantics; it removes tool execution authority,
replaces wire-only opaque provenance, preserves every unknown usage field, and exposes an async
single-attempt transport instead of carrying the Harness retry policy into the new boundary.
