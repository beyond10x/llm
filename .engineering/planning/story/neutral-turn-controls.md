---
format: aep.planning-md/3
id: story:neutral-turn-controls
kind: story
status: implemented
title: The neutral turn carries thinking, parallel tool calls and advisory hints
summary: llm-core Controls, the one effort table, llm.turn/4
relations:
- decomposes: epic:client-request-fields
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: checks/conformance/src
- confidence: inferred
  path: crates/llm-chat/tests
- confidence: cited
  path: crates/llm-core/src
- confidence: cited
  path: crates/llm-core/tests
- confidence: inferred
  path: crates/llm-messages/tests
- confidence: cited
  path: crates/llm-responses/src/request.rs
- confidence: inferred
  path: crates/llm-responses/tests
- confidence: inferred
  path: crates/llm-routing/tests
- confidence: inferred
  path: crates/llm-tool-call/src/lib.rs
- confidence: cited
  path: docs/contract-v1.md
- confidence: cited
  path: spec/domains/inference.yaml
revision: 17
transitions:
- {from: "draft", to: "proposed", at: "2026-10-17T04:25:32Z", actor: "human:timo", revision: 15, decided_on: {"recorded":{"review_outcome":3}}}
- {from: "proposed", to: "active", at: "2026-10-17T04:25:33Z", actor: "human:timo", revision: 16, decided_on: {"recorded":{"review_outcome":3}}}
- {from: "active", to: "implemented", at: "2026-10-17T07:23:40Z", actor: "human:timo", revision: 17, decided_on: {"recorded":{"test_result":1,"review_outcome":5,"verification":1}}}
---
## Context

The neutral turn has nowhere to hold a thinking mode, a parallel-tool-call choice or a caching,
verbosity or presentation hint, so every ingress decoder must refuse them
(`crates/llm-core/src/turn.rs` `TurnRequest`, `Sampling`). The effort is a free name checked
against the binding's `reasoning_efforts` as received, so `max`, which Messages has and the other
two wires do not, is refused on every Chat and Responses binding
(`crates/llm-core/src/turn.rs` `validate_for`).

## Design

`docs/design.md` "Client request fields", sections "Neutral additions" and "Reasoning":

- `llm_core::Controls { thinking: Option<Thinking>, parallel_tool_calls: Option<bool>, hints: Hints }`
  as `TurnRequest.controls`, serde-defaulted and skipped when empty.
- `Thinking` (`adaptive`, `enabled { budget_tokens }`, `disabled`), `Hints { prompt_cache:
  Option<PromptCache>, prompt_cache_key: Option<String>, verbosity: Option<Verbosity>,
  reasoning_summary: Option<ReasoningSummary>, unforwarded: BTreeSet<Unforwarded> }`,
  `PromptCache { ttl: Option<CacheTtl> }`, `CacheTtl` (`5m`, `1h`), `Verbosity`, `ReasoningSummary`,
  `Unforwarded` (`input-item-ids`, `client-metadata`), and `Advisory`, the closed list of hint
  names with `Hints::advisories()` and `Hints::omitted_by(carried)`.
- `llm_core::wire_effort(protocol, &TurnRequest) -> Result<Option<String>, Error>`: the reasoning
  table, the one place it is written. `validate_for` checks `reasoning_efforts` against its
  result.
- `llm_core::advisory_warning(wire, &[Advisory]) -> StreamEvent`: the `advisory-omitted` warning a
  client emits once a turn has succeeded.
- `llm_core::field_path(at, key)`: the path a by-name refusal prints, copying only a bounded key
  of the wire's own name shape (the rule `crates/llm-messages/src/lib.rs` `path` applies today).
- `TurnDocument`: `llm.turn/3` when controls are empty, `llm.turn/4` otherwise; both read; `/3`
  with controls and `/4` without are refused.
- `validate`: a zero thinking budget, `parallel_tool_calls` without published tools, and an empty
  or over-long `prompt_cache_key` (`docs/design.md`: at most 512 bytes) are refused.

ESS first: `spec/domains/inference.yaml` declares the controls as types before the code.

## Shared files

Every crate's tests that build a `TurnRequest` literal gain `controls: Controls::default()`; this
unit lands before the three wire stories branch, so they inherit it. The one non-test literal, `crates/llm-responses/src/request.rs` `ingest_request`, changes the same way. `docs/design.md` "Client
request fields" lands with the wave's opening commit; this unit adds the contract paragraph to
`docs/contract-v1.md`.

## Acceptance

- A turn with empty controls serializes byte-identically to llm 0.6.0's `llm.turn/3` (a test pins the 0.6.0 bytes of one turn with tools, sampling and every item kind); a turn with
  any control serializes as `llm.turn/4` and reads back equal; `/3` carrying `controls` and `/4`
  without are refused.
- `wire_effort` answers each of these rows of the reasoning table in `docs/design.md`, one test per row and wire: effort `max`, another effort, thinking `adaptive`, `enabled`, `disabled` with no effort, and `disabled` beside an effort. (The `reasoning_summary` row is a hint, not `wire_effort`'s.)
- `validate_for` accepts effort `max` on a Chat Completions or Responses binding that lists
  `xhigh`, and refuses it on a Messages binding that lists only `xhigh`.
- `validate` refuses a zero budget, `parallel_tool_calls` without tools, and an empty or 513-byte
  cache key, each with its own diagnostic.
- `Hints::omitted_by` returns exactly the present hints a carried list does not name, in
  `Advisory` order.
- `advisory_warning` returns `None` for no omission and otherwise one `Warning` with code
  `advisory-omitted` whose message names each omitted hint; `field_path` copies a key of ASCII
  letters, digits, `_` and `-` of 1 to 64 bytes and writes any other as `?`.
- `spec/domains/inference.yaml` declares `llm.inference.Controls`, `Thinking`, `Hints`, `PromptCache`, `CacheTtl`, `Verbosity`, `ReasoningSummary`, `Unforwarded` and `Advisory`, and `ess specify validate --path spec` is valid.

## Verification

`cargo test -p b10x-llm-core`; `cargo clippy -p b10x-llm-core --all-targets -- -D warnings`; the
package gates of every crate whose tests changed; `ess specify validate --path spec`.
