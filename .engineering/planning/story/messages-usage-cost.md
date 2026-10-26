---
format: aep.planning-md/3
id: story:messages-usage-cost
kind: story
status: implemented
title: A streamed Messages usage cost is carried as a recorded extension, not refused
summary: message_delta.usage.cost in any shape is admitted on a stream and reported verbatim as a usage-extension warning
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-10-26T19:35:22Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-26T19:35:23Z", actor: "human:timo", revision: 3}
- {from: "active", to: "implemented", at: "2026-10-26T20:14:10Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":1,"review_outcome":3,"verification":1}}}
---
## Context

A streamed Messages turn on an Anthropic-compatible route ends with a refusal when the route reports
a `cost` inside `message_delta.usage`. Observed 2026-10-26 by the takt 0.13.1 trial on the OpenCode
Go Messages endpoint (model `claude-haiku-5-5`) through b10x-harness 0.20.0, which pins llm 0.15.0:
the harness exits 5 at its third step with

```text
error: model wire refused: Unsupported: Messages field is outside the declared subset: message_delta.usage.cost
```

The text deltas before it had already been shown (`{"kind":"text-delta","text":"ok"}`), so the
turn is lost after the caller saw its answer. The refusal comes from the usage accept list in
`crates/llm-messages/src/usage.rs` (`Snapshot::update`, the `fields(...)` call at lines 20-35 of
0.15.0), which admits ten names and no `cost`.

The shape of the value is unknown. The trial captured the refusal, not the frame: the refusal names
the path only, and no fixture, capture or producer type read for this projection shows a `cost`.
No usage type this projection was written against declares one.

## Decision: an admitted-and-recorded extension, not a typed field

`cost` becomes the one usage name this projection admits in any JSON shape, and it is carried to the
caller verbatim rather than read. Chosen over a typed optional field because:

- **The shape is unknown.** A typed field (`f64`, or an object with a currency) would be a guess at
  a producer's type, and a wrong guess either refuses the route's real value or reads it wrongly.
  The declared-subset rule exists to refuse guessed semantics; a typed field would reintroduce one.
- **The neutral types cannot take it without breaking consumers.** `llm_core::Usage` and
  `llm_core::TurnObservation` are `deny_unknown_fields` structs with public fields that consumers
  build by struct literal (Loom's executor and commission tests do). A new field there is a breaking
  change to a consumer, outside this item.
- **"Carry it or refuse it" still holds.** Admitting the name and reading it nowhere, as
  `service_tier` is, would drop a value that matters to attributable cost. Instead the value is
  carried on the stream the caller already watches.

The declared-subset rule keeps its intent: exactly one more name is admitted, on the two usage
objects a stream carries, and every other unknown key at every level is still refused by its path.

## Change

- **Where `cost` is admitted.** On `message_start.message.usage` and on `message_delta.usage` of a
  streamed turn (`StreamDecoder`). Any JSON value. `null` is absent, as every other optional field
  of this projection is: admitted, recorded nothing.
- **Where it stays refused.** On the usage of a complete, non-streamed message (`decode_message`,
  which Bedrock's invoke path uses) and on ingress: neither has a sink to carry it on, so there it is
  still `Messages field is outside the declared subset: usage.cost`, by its path. Carrying it there
  needs a place in the neutral outcome, which is a core change and not this story.
- **How it is recorded.** Each non-null `cost` is reported once as
  `StreamEvent::Warning { code: "usage-extension", message }`, the code a public constant of
  `b10x-llm-messages`. The message names the path (`message_start.message.usage.cost` or
  `message_delta.usage.cost`) and the value's compact JSON, verbatim. A route that reports it on
  both events yields two warnings, in order.
- **It reads into nothing.** No counter, no `Usage` field, no item and no stop reason changes because
  of a `cost`. A turn with it has exactly the outcome the same turn has without it, apart from the
  warnings. It never becomes an `Item::Opaque`, so it is never replayed to a route.
- **Transactional.** The warning is emitted only after the usage object carrying it was accepted
  whole; a usage object that is refused for any other reason emits nothing.
- **Eligibility is unchanged.** A warning counts as visible output to fallback
  (`crates/llm-routing/src/fallback.rs`, `Counting`). So a `cost` on `message_start` is held, and
  emitted only once the stream has answered (the first payload outside `ping` and `message_start`);
  a stream that fails before answering drops it with the attempt, and stays as retriable and as
  eligible for fallback as it was in 0.15.0. A `cost` on `message_delta` is emitted with that delta.
- **Bounded.** A value whose compact JSON exceeds 1 KiB is refused, naming the path, rather than
  copied into a warning.

## Out of scope

- A typed `cost` in the neutral usage or observation, and pricing from it (`b10x-llm-cost`). A
  follow-up once a captured frame shows the producer's shape.
- `cost` or any other extension on the Responses and Chat Completions wires: nothing was observed
  there.
- A live call to the route in the gate.

## Acceptance

- `crates/llm-messages/tests/usage_extension.rs`, red on 0.15.0 and green after:
  - `a_cost_on_message_delta_usage_decodes_and_is_carried_as_a_warning`: a stream whose
    `message_delta.usage` carries `"cost": 0.00042` completes with the same items, usage and stop
    reason as without it, and with one `usage-extension` warning naming
    `message_delta.usage.cost` and `0.00042`;
  - an object value (`{"total":0.00042,"currency":"USD"}`) is carried verbatim the same way;
  - a `cost` on `message_start.message.usage` is held until the stream answers; a stream that
    fails with `overloaded_error` before answering emits no warning and stays retriable;
  - `"cost": null` decodes and emits no warning;
  - a sibling unknown key (`message_delta.usage.price`) beside a `cost` is still refused with
    `Messages field is outside the declared subset: message_delta.usage.price`;
  - a usage object refused for a counter that goes backwards emits no `usage-extension` warning;
  - a `cost` over 1 KiB is refused naming its path;
  - `decode_message` still refuses `usage.cost` by path.
- Conformance scenarios in `contracts/messages/scenarios/`, listed in `contracts/ess-inputs.yaml`:
  `usage-cost-on-a-message-delta-carried-as-a-warning`,
  `usage-cost-object-carried-verbatim-as-a-warning` and
  `unknown-usage-key-beside-a-cost-refused-by-path`; `contracts/baseline.json` floors raised by
  the scenarios added, skipped ceiling 0.
- `crates/llm-messages/tests/wire_names.rs` classifies `cost` and `usage-extension` with the
  evidence above.
- `docs/messages.md` names `cost` as the one usage extension, where it is admitted and refused, and
  why it is not typed.
- `RUSTUP_TOOLCHAIN=1.98.0 task check` exits 0 on the wave head.
