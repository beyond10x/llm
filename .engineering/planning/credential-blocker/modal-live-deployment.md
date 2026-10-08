---
format: aep.planning-md/3
id: credential-blocker:modal-live-deployment
kind: credential-blocker
status: cleared
title: Modal live deployment needs a Modal account and paid run
relations:
- blocks: story:modal-hosting
revision: 3
transitions:
- {from: "open", to: "cleared", at: "2026-10-08T10:43:39Z", actor: "human:timo", revision: 3}
---
## Context

`story:modal-hosting` accepts only when it "separately records a live deployment/readiness/cleanup
result for its supported lifecycle". That record needs a Modal account, a credential and a paid run.
None exists in this tree, and AGENTS.md keeps paid provider calls out of the default gate.

## Clears when

An operator names the Modal account and credential source a qualification run may use, and
authorises the paid run.

## Consequence

`story:gateway-translation` depends on `story:modal-hosting`, so the gateway translation, the
operator CLI and `story:foundation-qualified` stay behind this blocker. The fixture half of
`story:modal-hosting` could be split into its own story if the operator wants it scheduled earlier.

Filed 2026-09-26 during wave 2 selection.
