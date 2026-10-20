---
format: aep.planning-md/3
id: story:the-neutral-turn-in-ess
kind: story
status: draft
title: llm's neutral turn is modelled in ESS and held to it by conformance, as accounting is
relations:
- decomposes: epic:contracts
- serves: vision:portable-model-inference
revision: 1
---
## Context

Asked for by the owner on 2026-10-20, folding in the survey finding of the same day (Atlas ADR
0100): llm's inner workings, starting with the neutral port, have no ESS model, while Harness, Loom
and the inference repository pin that port, and the survey reports that ADR 0100's commit point is
defined against it.

What exists today, read at `5e00283d`:

- **The port.** `Model::turn` (`crates/llm-core/src/port.rs:75`) takes a `TurnRequest`
  (`crates/llm-core/src/turn.rs:77`: `model`, `instructions`, `items`, `tools`,
  `max_output_tokens`, `sampling`, `tool_choice`, `controls`), emits `StreamEvent`s
  (`port.rs:34`: `TextDelta`, `ToolCallStarted`, `ToolArgumentsDelta`, `ReasoningDelta`, `Warning`)
  and ends in a `TurnOutcome` (`turn.rs:350`: `stop_reason`, `items`,
  `observation`). None of `TurnRequest`, `StreamEvent`, `TurnOutcome`, `Item`, `ToolSpec`,
  `Sampling`, `ToolChoice` or `StopReason` is declared under `spec/`.
- **What is declared.** `llm.inference.TurnObservation` and the request controls (`Thinking`,
  `PromptCache`, `Hints`, `Controls` and their parts) in `spec/domains/inference.yaml:4-63`;
  `llm.accounting.Usage` (`spec/domains/accounting.yaml:12`), which `TurnObservation.usage`
  already uses; `llm.accounting.AttemptUsage` (`:80`), whose `request_id`, `route_target_id` and
  `serving_model_id` are plain strings; the catalog's `Endpoint`, `ServingModel`, `Route` and
  `RouteTarget` entities (`spec/domains/catalog.yaml:137-191`).
- **How accounting is held.** A verification-only command `llm.accounting.Quote`
  (`accounting.yaml:137-149`) hands raw versioned documents to the production parsers; its
  `observed` outcome emits `Quoted`, the view `LastQuote` publishes the result's facts, and the
  conformance target runs the real code for it (`checks/conformance/src/target.rs:196-199`), with
  authored scenarios under `contracts/pricing/scenarios/` (52 files). The specification header is
  `format: ess/14` (`spec/system.yaml:1`); the local `ess` is 0.60.0.

## Outcome

The neutral turn is a declared part of llm's specification: what a caller sends, what streams back
and what a turn ends in are types `ess` validates, their links to the catalog and to accounting are
relations rather than matching names, and a conformance suite runs real turns against them, so a
change to the port that the specification does not follow fails the gate.

## Acceptance

- `TurnRequest`, `StreamEvent`, `TurnOutcome` and the types they carry (`Item`, `ToolSpec`,
  `Sampling`, `ToolChoice`, `StopReason`) are declared in `spec/domains/inference.yaml` or a new
  `llm.turn` domain, every field and variant the Rust types have, reusing the declared
  `TurnObservation`, controls and `Usage` rather than restating them.
- The links are declared where a source in the tree settles them: what `TurnRequest.model` names
  (a `Route` alias, a `ServingModel`, or neither), and how an `AttemptUsage` row relates to the turn,
  route target and serving model it records. A link no source settles is an `UNMAPPED:` marker in
  the specification and a question in the story's report, not a guessed relation.
- A verification-only command in accounting's pattern runs one turn through the real port against
  a pinned, local fixture (no provider call): its input is the raw request and the recorded
  response bytes, its view publishes the events and the outcome, and the conformance target
  answers it with the production clients. Scenarios cover a text turn, a tool call, a refusal and
  a cut stream on each of the three protocols.
- A Rust-side check fails when a field or variant is added, removed or renamed in one of the
  declared types without the specification, and the reverse; a mutation of each type shows it red.
- `ess specify validate --path spec`, the projection drift check and the conformance check stay
  green; `contracts/baseline.json` rises by the new scenarios.
- The specification moves to the newest format the pinned `ess` implements
  (`ess specify formats`) only if a construct needs it; the format change is its own commit.
