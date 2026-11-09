---
format: aep.planning-md/3
id: story:routes-resolve-one-endpoint
kind: story
status: active
title: An endpoint whose source cannot be built refuses only its own routes
summary: 'Issue 36: already fixed on main by 4e18172f (0.12.0); proven by test output'
tags:
- from-issue
refs:
- provider: github
  reference: beyond10x/llm#36
relations:
- decomposes: epic:github-zero-2026-11-09
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: crates/llm-models/tests/routes.rs
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-11-09T22:06:21Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-09T22:06:21Z", actor: "human:timo", revision: 3}
---
## Context

Issue https://github.com/beyond10x/llm/issues/36 (opened 2026-10-17): in 0.6.0 `Routes::resolvers`
built a resolver for every endpoint that carries a secret and returned the first error, so one
unbuildable endpoint (a `file:~/` secret with no `HOME`, a `keychain:` secret on a build without
`keychain`) refused every other endpoint of the document.

## Outcome

An endpoint whose credential source cannot be built refuses only its own routes; every other
endpoint of the same document still resolves and serves.

## Finding

Already on `main`. Commit `4e18172f` ("fix: refuse only the routes of an endpoint whose credential
source cannot be built", 2026-10-23) changed `crates/llm-models/src/routes.rs`; `CHANGELOG.md:210`
records it as `**Breaking:** llm-models: Routes::resolvers refuses an unbuildable source per
endpoint, as unauthorized at port`. `git tag --contains 4e18172f` lists `0.12.0` first, so every
release from 0.12.0 on carries it. Nothing was built for this story.

## Acceptance

`crates/llm-models/tests/routes.rs::an_endpoint_whose_source_cannot_be_built_refuses_only_its_own_routes`
passes at `origin/main` `7c56b19b` on the feature sets the issue names (`environment,file` without
`keychain`, and with `keychain`), and the issue is closed through the bot citing that output.
