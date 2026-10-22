---
format: aep.planning-md/3
id: approval-record:llm-command-2026-10-22
kind: approval-record
status: draft
title: The llm command and Bedrock routes, asked for by the owner on 2026-10-22
relations:
- decides: story:llm-command
revision: 2
---
## The stop

The wave skill's stage-1 proposal stop for the `llm` command, and the lifting of wave
2026-10-22-w15's decision that routes refuse Bedrock.

## What authorises passing it

The owner, in session on 2026-10-22: first "create a story" for the command he described (the
words are quoted in `story:llm-command` § Context), then "do it". The coordinating session relayed
the story and the instruction to implement it as wave 2026-10-23-w16 on 2026-10-23.

Covered: the commits of wave 2026-10-23-w16 for `story:llm-command` (the open commit, the
`impl/llm-command` commits, the merge into `wave/2026-10-23-w16`, the close commit). Not covered:
pushes, pull requests, tags, a release, consumer pin moves, and any serving or gateway change
(beyond10x/llm-gateway's own work). The coordinating session publishes.

## Decisions taken inside the item

- `llm.routes/1` keeps its format string and gains only optional keys, so every document that
  loaded before still loads, with the same catalog.
- The TOML file is the same document as the YAML one; a key is the same key in either.
