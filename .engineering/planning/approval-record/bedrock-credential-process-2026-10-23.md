---
format: aep.planning-md/3
id: approval-record:bedrock-credential-process-2026-10-23
kind: approval-record
status: draft
title: The aws-process credential source and llm 0.14.0, asked for by the owner on 2026-10-23
relations:
- decides: story:bedrock-credential-process
revision: 1
---
## The stop

The wave skill's stage-1 proposal stop for `story:bedrock-credential-process` (an `aws-process`
Bedrock credential source that runs a `credential_process` command), and the release of
llm 0.14.0.

## What authorises passing it

The owner, in session on 2026-10-23: he proposed "in toml file for bedrock section we can provide
a secret command, so it would then call this command automatically to retrieve it ... this way it
remains in memory only". The session answered with option A (an `aws-process` auth kind running
one `credential_process` command), a story under `epic:bedrock`, one wave with a sub-agent
implementor and a reviewer, and the release of llm 0.14.0. The owner answered "do it".

Covered: the commits of wave 2026-10-23-w18 for `story:bedrock-credential-process` (the open
commit, the `impl/bedrock-credential-process` commits, the merge into `wave/2026-10-23-w18`, the
close commit), and the source release of llm 0.14.0 through `AGENTS.md` § Release, published by
this session as the bot. Not covered: registry publishing, consumer pin moves in other
repositories, and any serving or gateway change.

## Decisions taken inside the item

- One `aws-process` auth kind returning all keys and their expiry, not a general `command:`
  secret reference: three fields would run three commands and have no expiry, so an SSO session
  would stop working after its lifetime.
- Running a command is opt-in twice: a cargo feature `process` and a caller permission on
  `RouteResolvers`. A library consumer that loads a routes document it did not write runs no
  command; only `llm-cli` enables both.
- The command is a program and its arguments, run without a shell.
