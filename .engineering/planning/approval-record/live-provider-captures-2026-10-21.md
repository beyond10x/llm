---
format: aep.planning-md/3
id: approval-record:live-provider-captures-2026-10-21
kind: approval-record
status: draft
title: Two live captures on the owner's subscriptions, approved by the owner on 2026-10-21
relations:
- decides: story:live-provider-captures
revision: 1
---
## The stop

Two live provider calls on the owner's subscriptions, and the commit, pull request and landing of
story:live-provider-captures.

## What authorises passing it

The owner, 2026-10-21, on morning-queue item 9, relayed by session `b10x-chef-orchestrator`: "LLM
live captures, record both, decision A, option A". The relay states that the answer approves the
spend for these two recordings and nothing beyond, through the subscription logins already on this
machine (the Claude subscription token for Messages, the Codex login for Responses), with no paid
API key and no new credential. The commit, pull request and bot fast-forward landing are approved
under exception 6 of the owner's standing commit exceptions.

Covered: one recorded Codex Responses turn; two recorded Messages turns, the second repeating the
first prompt so the cache is read; their scrubbed fixtures, the replay tests and the docs; one
commit series, its pull request and the fast-forward of `main`. Not covered: a release, any further
live call, any credential acquisition.
