---
format: aep.planning-md/1
id: story:messages-projection
kind: story
status: implemented
title: Messages projects the supported neutral subset
relations:
- decomposes: epic:inference
- depends_on: story:http-streaming
- depends_on: story:neutral-inference
- depends_on: story:provider-accounts
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: checks/conformance/src/messages.rs
- confidence: cited
  path: contracts/messages/scenarios
- confidence: cited
  path: crates/llm-messages
- confidence: inferred
  path: docs/messages.md
- confidence: inferred
  path: docs/verification/messages-falsification.json
- confidence: inferred
  path: docs/verification/messages-report.json
- confidence: inferred
  path: docs/verification/messages.md
- confidence: cited
  path: spec/domains/messages.yaml
revision: 11
---
## Context

Outgoing and ingress codecs share the contract. Authentication presentation is supplied by provider binding; API keys and caller-owned OAuth are not inferred from wire choice. Reject unsupported fields rather than copying provider-specific state across models.

## Acceptance

Pinned Messages request and stream fixtures preserve tools, thinking state, cache usage and failures across the declared subset.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

Derived 2026-09-20 by `story-scoper`. Every line is **cited** (read from the artifact, a diff or a
file opened in the tree) or **inferred** (worked out from the sibling domains and could be wrong).

- cited: `crates/llm-messages` — the artifact scope names it and the worktree already carries
  uncommitted work here (`src/lib.rs`, new `src/codec.rs`, new `src/usage.rs`, `Cargo.toml`
  dependencies). Sole owner; no other wave-1 candidate names this crate.
- cited: `spec/domains/messages.yaml` — named in the artifact scope; exists untracked in this
  worktree declaring `llm.messages.Project`, `llm.messages.DecodeStream`, `llm.messages.Inspected`
  and `llm.messages.LastInspection`.
- cited: `contracts/messages/scenarios` — the artifact scope cites `contracts/messages`; every
  implemented domain authors its scenarios under `contracts/<domain>/scenarios` (`budget`,
  `inference`, `pricing`, `routing`, `secrets` all do).
- inferred: `checks/conformance/src/messages.rs` — the artifact cites `checks/conformance` without a
  file; one adapter module per domain is the established shape (`inference.rs`, `pricing.rs`,
  `secrets.rs`, `budgets.rs`), and the sibling stories cite `responses.rs`, `chat.rs`, `hosting.rs`.
- inferred: `checks/conformance/src/main.rs` — **shared.** The module list at the top of the file is
  a flat `mod budgets; mod gate; mod inference; mod pricing; mod secrets; mod target;`; a new adapter
  must be declared there.
- inferred: `checks/conformance/src/target.rs` — **shared.** `CatalogTarget::execute_command` is one
  `match` over command names and `query_view` one `matches!` over view names; the two Messages
  commands and `llm.messages.LastInspection` land as new arms in both.
- inferred: `checks/conformance/Cargo.toml` — **shared.** The adapter needs a `llm-messages`
  dependency; the manifest currently carries explicit path deps for `llm-cost` and `llm-routing`.
- cited: `spec/system.yaml` — **shared.** The uncommitted diff in this worktree is exactly
  `+  - llm.messages` appended to the `domains:` list.
- inferred: `docs/messages.md` — the artifact scope cites `docs`; `docs/` carries a per-domain
  narrative for the finished domains (`budgets.md`, `pricing.md`, `local-secrets.md`), and the
  sibling stories all carry an inferred `docs/<domain>.md`.
- inferred: `docs/verification/messages.md` — verification record, by the convention of
  `docs/verification/{budgets,pricing,local-secrets,observations}.md`. The inference domain's record
  is named `observations.md`, not `inference.md`, so this filename is the weakest line in the set.
- inferred: `docs/verification/messages-report.json` — paired ESS count report, by convention.
- inferred: `docs/verification/messages-falsification.json` — paired mutation record, by convention.

The dominant surface is `crates/llm-messages`, which nothing else in wave 1 touches: the codec, the
usage snapshot and the streaming client all land inside it. Everything contentious is registration —
`spec/system.yaml` and the three `checks/conformance` integration files — where this story appends
one line or one arm alongside every other new-domain story in the wave.

`crates/llm-core`, `crates/llm-http`, `crates/llm-providers` and `crates/llm-credentials` are read
but **not changed**: `Protocol::Messages`, `MAX_REQUEST_BYTES`, `MAX_TOOL_ARGUMENT_BYTES`, `exceeds`,
`TurnRequest`, `ToolSpec`, `ToolChoice`, `Usage`, `SseDecoder` and `HttpClient` all already exist, so
the in-flight `crates/llm-messages/Cargo.toml` consumes them as workspace dependencies rather than
extending them. `crates/llm-gateway` is explicitly out of scope — the implementation contract defers
gateway server composition to its own story, and this story ships only the stateless ingress decoder
inside `llm-messages`.

Confidence: high — the artifact's own scope, an in-flight uncommitted implementation and three
finished sibling domains agree on the shape; only the four `docs/` filenames are convention rather
than evidence.

## Implementation contract

Implement the declared Messages text/tool subset using one shared codec for outgoing requests,
stateless gateway ingress, full responses and SSE decoding. Bind all returned/opaque observations
to the immutable selected target. Use the existing HTTP single-attempt client and injected resolver;
never refresh, retry, switch accounts, add subscription impersonation or infer billing from protocol.

Preserve ordered roles, published tool names/object arguments, tool-result failure flags, signed
thinking and redacted thinking. Refuse unsupported content, tool/server features, cache TTL mixtures,
unknown mandatory semantics, inconsistent terminal reasons, event-name/type mismatch, duplicate or
out-of-order transitions and truncated streams. Accept optional metadata only where its irrelevance
to the declared subset is explicit. Preserve true unknowns, exact counters and the last valid usage
snapshot on every failure. Cumulative usage replaces corresponding reported counters; no summing
successive deltas, no regression, overflow or fabricated zero. Anthropic disjoint input/cache fields
normalize to the core inclusive input only when every required component is known. One-hour cache
creation and nonzero server-tool use are outside this first pricing subset and explicitly refuse.

Producer evidence: anthropics/anthropic-sdk-python commit
0af0190679a9e80388bd1b0328d557c9a91a11b2, src/anthropic/types plus official streaming and caching docs
retrieved 2026-09-19. Harness source at 709a2ebadcc14602b82b6f3c240350e4ddc1c88c provides extraction
provenance; fix its configured-model substitution, unknown-as-zero and saturating arithmetic instead
of porting those defects. Fixtures retain producer metadata the declared subset does not consume.

Pure codecs perform no I/O. The client resolves credentials at request time, uses the configured API
prefix, bounds headers/body/stream and retains backpressure. Caller cancellation and absolute turn
deadline cover secret resolution, HTTP and blocked sinks. A terminal event is required; EOF never
manufactures success. Fixed safe errors retain dispatch and observations without raw upstream text.

Verify with deterministic producer-shaped Rust fixtures, local sockets, executable ESS observations
of the real public codec/decoder and mutation falsification. No paid calls. Anthropic API/subscription
presentation qualification and gateway server composition stay in their separate required stories.
The story remains unfinished until shared codecs, streaming client and declared-subset evidence exist.
