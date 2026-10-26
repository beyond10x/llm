---
format: aep.planning-md/3
id: approval-record:wave-2026-10-26-w19-proposal
kind: approval-record
status: draft
title: Wave 2026-10-26-w19 and its release, authorised by b10x-chef-orchestrator under the owner's grant
tags:
- non-interactive
relations:
- decides: story:opencode-go-session
revision: 1
---
## The stop

The wave skill's stage-1 proposal stop for wave 2026-10-26-w19 (`story:opencode-go-session`), and
the release that follows it.

## What authorises passing it

Session `b10x-chef-orchestrator`, on 2026-10-26, under the owner's grant for class-C items in
repositories under the workspace: "Item: the OpenCode Go anthropic path answers HTTP 400 to llm's
anthropic-messages requests. [...] Fix: llm sends an `x-opencode-session` header on the
anthropic-messages wire when the endpoint is OpenCode Go, as a named behaviour of that wire [...]
Not a generic \"extra headers from config\" surface [...] Decide the trigger (base-URL match vs an
explicit wire/endpoint option) yourself and record the choice in the store; prefer what takt's
endpoint file can express (takt 0.11.1 endpoint file has base_url/wire/model/credential)."

Procedure named in the same message: "through the AEP store (a story if none exists [...]),
/aep:implementing wave mode with an adversary pass, one full gate, bot commit/merge/PR/fast-forward
per AGENTS.md, then release per AGENTS.md § Releases (release-check decides the version and the
Breaking line), run verify-release.sh and report the tag."

The run is non-interactive: the session runs as a sub-agent of the orchestrator and has no operator
turn.

Covered: the commits of wave 2026-10-26-w19 (the open commit, the `impl/opencode-go-session`
commits, the merge into `wave/2026-10-26-w19`, the coordinator's integration commit, the close
commit), bot pushes of the wave branch, the wave pull request, the fast-forward of `main`, and the
source release through `AGENTS.md` § Release as the bot. Not covered: registry publishing, consumer
pin moves (harness, takt), any other story.

## Decisions taken inside the item

- Trigger: the base URL (https, host `opencode.ai`, default port, path under `/inference/go/`), not
  an explicit option: takt's endpoint file can express it with no new field.
- Value: one identifier minted per client and sent on every request of that client; a caller may
  name it instead. The provider asks for "a stable session ID [...] for each conversation"
  (<https://opencode.ai/docs/go/>, read 2026-10-26) and gives no format.

## The units, and the objective each serves

One unit, `story:opencode-go-session`, serving `vision:portable-model-inference`. Selection by
reading: one candidate, filed for this item.
