---
format: aep.planning-md/3
id: story:bedrock-web-identity
kind: story
status: implemented
title: Bedrock credentials from web identity (IRSA) without an AWS SDK
tags:
- from-adopter-report
relations:
- decomposes: epic:bedrock
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: crates/llm-bedrock/src/credentials.rs
- confidence: inferred
  path: crates/llm-bedrock/src/lib.rs
- confidence: inferred
  path: crates/llm-models/src/lib.rs
- confidence: inferred
  path: crates/llm-models/src/routes.rs
- confidence: inferred
  path: docs/bedrock.md
- confidence: inferred
  path: spec/domains/bedrock.yaml
revision: 11
transitions:
- {from: "draft", to: "proposed", at: "2026-11-04T07:35:28Z", actor: "human:timo", revision: 8}
- {from: "proposed", to: "active", at: "2026-11-04T07:35:28Z", actor: "human:timo", revision: 9}
- {from: "active", to: "implemented", at: "2026-11-05T01:13:10Z", actor: "human:timo", revision: 11, decided_on: {"recorded":{"test_result":1,"review_outcome":2}}}
---
## Context

A downstream adopter reading llm 0.20.0 reported that Bedrock credentials cover EKS Pod Identity but
not IAM roles for service accounts (IRSA). Verified against the code on 2026-11-04:

- `docs/bedrock.md:124`: "Not supported: shared credential and config files, profiles, instance
  metadata (IMDS), web identity (IRSA) and SSO."
- `crates/llm-bedrock/src/credentials.rs` ships `StaticCredentials`, `ContainerCredentials` (ECS and
  EKS Pod Identity) and `ProcessCredentials`; `crates/llm-models/src/routes.rs:604` admits only
  `aws-keys`, `aws-container` and `aws-process`.
- The workaround, an `aws-process` route running `aws configure export-credentials`, needs the AWS
  CLI in the container and feature `process`.

IRSA, per AWS documentation: the pod gets `AWS_ROLE_ARN` and `AWS_WEB_IDENTITY_TOKEN_FILE`
(`/var/run/secrets/eks.amazonaws.com/serviceaccount/token`), refreshed by the kubelet
(https://docs.aws.amazon.com/eks/latest/userguide/pod-configuration.html). Credentials come from STS
`AssumeRoleWithWebIdentity` with `RoleArn`, `RoleSessionName` and `WebIdentityToken`; the call "does
not require the use of AWS security credentials" and answers XML `Credentials` with `AccessKeyId`,
`SecretAccessKey`, `SessionToken` and `Expiration`
(https://docs.aws.amazon.com/STS/latest/APIReference/API_AssumeRoleWithWebIdentity.html).

## Outcome

A pod using IRSA reaches Bedrock without the AWS CLI, an AWS SDK or feature `process`.

## Acceptance

- `llm_bedrock::WebIdentityCredentials` takes a role ARN, a session name, a regional STS endpoint and a
  secret reference the resolver binds to the token file; it reads the token on every fetch, calls
  `AssumeRoleWithWebIdentity` unsigned, and keeps and refreshes the answer with the single-flight
  refresh `ContainerCredentials` and `ProcessCredentials` use.
- The STS endpoint must be `https`, or `http` to a loopback address for fixtures; anything else is
  refused before a connection, and no credential reaches a redirect target or an ambient proxy.
- The answer is decoded straight into zeroizing strings, bounded in size; a member missing or named
  twice is refused. No new dependency on an AWS SDK crate or `aws-lc`.
- Failure classes as for container credentials: STS `ExpiredToken`, `InvalidIdentityToken`, a 4xx or
  an unusable answer is `unauthorized`, final; no answer, a cut, 5xx or `IDPCommunicationError` is
  retriable; all are dispatch `not-sent`, and no message quotes the token, the answer or the keys.
- A routes document admits `auth.kind = "aws-web-identity"`, its values supplied by the caller the
  way `aws-container` values are; no secret is a document value.
- ESS: the `llm.bedrock` domain declares the source and its failures; authored scenarios against a
  loopback STS fixture; the conformance baseline rises and skips none.
- `docs/bedrock.md` documents IRSA beside Pod Identity, marked unverified until a live run.

## Verification

Targeted `llm-bedrock` and `llm-models` tests and conformance scenarios against a loopback STS
fixture. No AWS account or live call; a live run is `story:bedrock-live-qualification`.

## Delivered in wave 2026-11-04-w25

The credential source, its failure classes, the ESS declarations and 14 authored scenarios shipped.
The routes-document item (`auth.kind = "aws-web-identity"`) did not: it needs a variant on
`AwsCredentialKind` (`crates/llm-models/src/routes.rs:760`), which is not `#[non_exhaustive]`, and the
wave ran additive only (`approval-record:wave-2026-11-04-w25-proposal`). It moved to
`story:bedrock-web-identity-route`, which ships with the breaking minor.
