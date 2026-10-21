---
format: aep.planning-md/3
id: approval-record:live-capture-claude-login-2026-10-21
kind: approval-record
status: draft
title: One Messages capture with the active Claude login's token, approved by the owner on 2026-10-21
relations:
- decides: story:live-provider-captures
revision: 1
---
## The stop

One live Messages capture run for story:live-provider-captures, authenticated with the access
token of the Claude login active on the operator's machine (`~/.claude/.credentials.json`).
`docs/live-qualification.md` says llm never reads Claude Code's login file; this run is an
exception to that rule for one capture process, not a change to it.

## What authorises passing it

The owner, 2026-10-21, typed in session `llm` at 11:4x Europe/Berlin and relayed by
`b10x-chef-orchestrator`: "for claude, in that test, just take the token from the currently
active claude instance in ~/.claude". Asked in the `llm` session to confirm it (option A: one
capture, token read at run time only, never stored, printed or logged), the owner answered "A".

## Limits

- The token was read at resolve time inside one uncommitted scratch process and held only in
  its memory. It was not written to the keychain or to any file, and it was not printed or logged.
- The scratch example was deleted before commit. No committed llm code reads a login file.
- One process run: two turns of the same request, recorded as a response-only capture.
- Not covered: any further run, a committed login-file reader, a release.
