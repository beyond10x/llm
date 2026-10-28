---
format: aep.planning-md/3
id: approval-record:takt-delivers-context-length-refusal
kind: approval-record
status: approved
title: takt delivers story:context-length-refusal-is-typed on its default protocol
summary: 'Decided by the orchestrator on 2026-10-28: accept the story and let takt deliver it, merge grant only'
relations:
- decides: story:context-length-refusal-is-typed
revision: 3
transitions:
- {from: "draft", to: "approved", at: "2026-10-28T19:15:28Z", actor: "human:timo", revision: 3}
---
## Decision

`story:context-length-refusal-is-typed` is accepted (`draft` to `proposed`) and is delivered by takt
on its default protocol, `story.delivery/2`, with no `--protocol` and no `--test-cmd`: one run to
the merge approval, one grant of `repository.merge` at the exact revision, one run to the merge. No
hand-written code: the change is takt's and its harness's.

## Instruction

Decided by the orchestrator on 2026-10-28 under the owner's standing grant, with the owner's words
quoted verbatim:

> what we're needing soon now actually is really proof that we can take this thing, point it to a
> story within the repo and see it solving that story.

## Why this story

It replaces `story:routes-refusal-names-a-later-route-field-line`, whose outcome is already on
`main`: `RawRoute` reads a mapping through `MapAccessDeserializer` since `095c8a3e` (2026-10-23). A
takt 0.18.0 run on it wrote tests that passed at once, found nothing to implement and could not
open a pull request, since `story.delivery/2` opens one only for a change of both tests and
implementation. That story stays `proposed` for its owner to close.

This one is open on `main` (`status_error` in `crates/llm-http/src/transport.rs` maps every 400 to
one refusal), touches one crate, has tests as acceptance and needs no network, credentials or new
dependency. The setup the earlier record describes (gates, assertions project, commit identity,
merge delegation) is reused unchanged.
