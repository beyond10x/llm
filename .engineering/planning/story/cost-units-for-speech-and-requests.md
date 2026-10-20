---
format: aep.planning-md/3
id: story:cost-units-for-speech-and-requests
kind: story
status: draft
title: Prices cover audio seconds in and out, input characters and requests
relations:
- decomposes: epic:routing
- serves: vision:portable-model-inference
revision: 1
---
## Context

Reported by the `inference` repository on 2026-10-20 (Atlas ADR 0100: gaps in the model layer go
to llm), planning its epic usage-and-cost. inference depends on `b10x-llm-cost` at the pinned llm
tag behind its own `cost` feature, with `sqlite` off.

`b10x-llm-cost` prices six units (`Unit`, `crates/llm-cost/src/pricing.rs:21-28` at tag 0.8.0):
`InputToken`, `CachedInputToken`, `CacheCreationInputToken`, `OutputToken`, `ResourceMillisecond`
(compute duration) and `RecordedCharge`. The neutral `Usage` it prices from counts tokens only
(`crates/llm-core/src/turn.rs:273-284`; `llm.accounting.Usage`, `spec/domains/accounting.yaml:12`).
inference also serves speech and needs to price:

- audio duration: speech-to-text input, text-to-speech output, and realtime audio a provider
  reports as a duration;
- input characters, the unit text-to-speech is commonly billed in;
- requests, for a price per call.

Until those units exist, inference prices such lines as unknown.

## Outcome

A price book can state a rate per second of audio in, per second of audio out, per input character
and per request, and a quote prices a usage record that reports those quantities, keeping a
quantity the record does not report unknown.

## Acceptance

- `Unit` gains the four units, with their `llm.prices/1` spellings, and the price book reads a rate
  for each; an unknown unit name in a price book is still refused.
- The usage a quote reads carries the matching optional quantities, each unknown when not
  reported (never zero), with `llm.accounting.Usage` and the projected schema updated in the same
  change; whether they live on `llm_core::Usage` or on a separate record is decided and written in
  `docs/design.md`.
- A test per unit prices a known quantity exactly and keeps an unreported one unknown; the existing
  token, compute and recorded-charge tests are unchanged.
- Works with `b10x-llm-cost`'s `sqlite` feature off, as inference builds it.
