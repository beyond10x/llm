---
format: aep.planning-md/3
id: story:cli-web-identity
kind: story
status: draft
title: The llm command reaches Bedrock through web identity from the AWS environment
tags:
- from-review
relations:
- decomposes: epic:bedrock
- depends_on: story:bedrock-web-identity-route
revision: 3
---
## Context

`story:bedrock-web-identity-route` (wave 2026-11-05-w26) gave a routes document
`auth.kind = "aws-web-identity"`; a library caller supplies the role through
`SecretSources::with_web_identity` or `Resolvers::web_identity_role`. The `llm` command does not:
`llm_cli::Environment` reads no `AWS_ROLE_ARN`, `AWS_ROLE_SESSION_NAME` or
`AWS_WEB_IDENTITY_TOKEN_FILE`, and `sources()` (`crates/llm-cli/src/lib.rs:510`) never calls
`with_web_identity`. `Environment` has only public fields and is not `#[non_exhaustive]`, so adding
them breaks a caller that builds it by literal; the unit left it out for that reason.

## Outcome

`llm` reaches Bedrock through web identity (IRSA) from the standard AWS environment variables, as it
does through container credentials.

## Acceptance

- With `AWS_ROLE_ARN` and `AWS_WEB_IDENTITY_TOKEN_FILE` set, a route of kind `aws-web-identity` runs a
  turn through `llm`; without them it refuses before I/O, naming the missing variable, not its value.
- The session name and STS endpoint have a documented source; no token value appears in output.
- `Environment` gains the fields in a breaking minor, made `#[non_exhaustive]` with a constructor in
  the same release, or the fields come in without a break.
