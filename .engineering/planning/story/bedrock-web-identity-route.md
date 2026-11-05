---
format: aep.planning-md/3
id: story:bedrock-web-identity-route
kind: story
status: implemented
title: A routes document reaches Bedrock through web identity (IRSA)
tags:
- from-adopter-report
relations:
- decomposes: epic:bedrock
- serves: vision:portable-model-inference
- depends_on: story:bedrock-web-identity
- depends_on: story:core-types-non-exhaustive
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-11-05T01:52:02Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-05T01:52:03Z", actor: "human:timo", revision: 3}
- {from: "active", to: "implemented", at: "2026-11-05T05:25:14Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":1,"review_outcome":1}}}
---
## Context

`story:bedrock-web-identity` shipped `llm_bedrock::WebIdentityCredentials` in wave 2026-11-04-w25,
which ran additive only (`approval-record:wave-2026-11-04-w25-proposal`). Its acceptance item for a
routes document, `auth.kind = "aws-web-identity"`, needs a variant on `AwsCredentialKind`
(`crates/llm-models/src/routes.rs:760`), a public enum that is not `#[non_exhaustive]`, so it was left
out. Until it lands, a caller builds the source through its own constructor.

## Outcome

A routes document reaches Bedrock through web identity (IRSA), as it does through container and
process credentials.

## Acceptance

- `auth.kind = "aws-web-identity"` in a routes document, YAML and TOML, builds a Bedrock endpoint
  with `WebIdentityCredentials`; its role ARN, session name, STS endpoint and token file come from
  the caller the way `aws-container` values do; no secret is a document value.
- Shipped in the breaking minor of `approval-record:image-input-breaking-minor`, with
  `AwsCredentialKind` made `#[non_exhaustive]` in the same release.
- Authored scenarios for the routes document; the conformance baseline rises and skips none.
