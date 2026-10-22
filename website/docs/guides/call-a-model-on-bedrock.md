---
title: Call a model on Bedrock
sidebar_position: 2
description: Build a BedrockClient for an Amazon Bedrock Runtime model, sign it with keys or with EKS Pod Identity credentials, and run one turn through the neutral port.
lede: One signed Bedrock Runtime turn through the neutral port, with keys or container credentials, and no AWS SDK.
source: crates/llm-bedrock (client.rs, credentials.rs, sigv4.rs, eventstream.rs, tests/data), docs/bedrock.md, spec/domains/bedrock.yaml, contracts/bedrock/scenarios
---

# Call a model on Bedrock

`llm-bedrock` sends a neutral turn to Amazon Bedrock Runtime, signed with AWS Signature Version 4.
It speaks two wires: Converse, for any model Converse serves, and the Anthropic Messages body on
`InvokeModel`, for Anthropic models.

:::warning[Not run against Bedrock]

No live Bedrock call was made. The client is checked against vendored recordings, the AWS
signing test suite and loopback fixture servers only. The model id below is an example id, not
verified.

:::

A catalog, a binding document and an `llm.routes/1` document cannot name Bedrock yet: each refuses
`bedrock-converse` by name. You build the client yourself.

## The target

A `BedrockTarget` says which model, where, and over which wire:

| Field | Example | Notes |
| --- | --- | --- |
| `region` | `eu-central-1` | The request is signed for it. |
| `model_id` | `eu.anthropic.claude-haiku-5-5` | Example id, not verified. A model id, an inference profile id or an ARN. |
| `api` | `Api::Converse` | Or `Api::AnthropicInvoke` for an Anthropic model. |
| `stream` | `true` | Must equal `capabilities.output_streaming`. |
| `endpoint` | `None` | `None` means `https://bedrock-runtime.{region}.amazonaws.com`; an origin replaces it. |

The provenance protocol must be the wire's: `Protocol::BedrockConverse` for Converse,
`Protocol::Messages` for `InvokeModel`. `provenance.model` is the neutral name a turn's `model`
must match; it is independent of `model_id`.

## Credentials

The client asks its `CredentialSource` for credentials on every turn, so a rotated key is used
without a restart. Two sources ship:

- `StaticCredentials` resolves an access key id, a secret access key and an optional session token
  through a `SecretResolver` you inject, as every other account in llm does.
- `ContainerCredentials` reads the container credentials endpoint that EKS Pod Identity and ECS
  task roles serve. You pass the endpoint URI and a secret reference bound to the token file.

Shared config files, profiles, instance metadata, web identity, SSO and `credential_process` are
not supported. Resolve those keys yourself and hand them to `StaticCredentials`.

## A turn with Pod Identity credentials

This sketch is not one of the compiled guide programs. It needs `llm-bedrock`, `llm-core`,
`llm-http`, `llm-credentials` with the `file` feature, and `tokio`.

```rust
use llm_bedrock::{Api, BedrockClient, BedrockTarget, ContainerCredentials};
use llm_core::{Cancel, Capabilities, Id, Item, Model, Protocol, Provenance, TurnRequest, VecSink};
use llm_credentials::{SecretRef, file::FileResolver};
use llm_http::{HttpClient, Limits};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

let token = SecretRef::new("pod-identity-token")?;
let resolver = FileResolver::new(BTreeMap::from([(
    token.clone(),
    PathBuf::from(std::env::var("AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE")?),
)]))?;
let http = HttpClient::new(Limits::default())?;
let credentials = ContainerCredentials::new(
    http.clone(),
    &std::env::var("AWS_CONTAINER_CREDENTIALS_FULL_URI")?,
    Arc::new(resolver),
    token,
)?;
let target = BedrockTarget::new(
    Provenance {
        protocol: Protocol::BedrockConverse,
        provider: Id::new("bedrock")?,
        account: Id::new("my-aws-account")?,
        endpoint: Id::new("eu-central-1")?,
        model: Id::new("haiku")?,
        binding_revision: Id::new("2026-10-22")?,
    },
    Capabilities {
        tools: true,
        tool_choice: true,
        temperature: true,
        top_p: true,
        reasoning_efforts: Vec::new(),
        context_window: 200_000,
        max_output_tokens: 4096,
        output_streaming: true,
        input_during_output: false,
    },
    "eu-central-1",
    "eu.anthropic.claude-haiku-5-5",
    Api::Converse,
    true,
    None,
)?;
let client = BedrockClient::new(target, http, Arc::new(credentials))?;

let request = TurnRequest::new("haiku", vec![Item::user("Hello")]);
let mut sink = VecSink::new(64, 64 * 1024);
let outcome = client.turn(&request, &mut sink, &Cancel::new()).await?;
```

The capabilities above are placeholders. Declare the ones your model has.

The container source sends the token as `authorization`, keeps the answer until five minutes
before it expires, and admits only `https` or `http` to a loopback address, `169.254.170.2`,
`169.254.170.23` or `[fd00:ec2::23]`. Pod Identity needs the EKS Pod Identity Agent on Linux EC2
nodes. Whether `automountServiceAccountToken: false` removes the token volume is unknown. The
Bedrock page in the repository lists the AWS sources for these facts and marks them unverified.

## What to expect

- **Usage.** Converse's neutral `input_tokens` is the uncached input plus both cache counters.
  The Converse body never asks for prompt caching, so a cache counter the answer leaves out is a
  known 0, and `input_tokens` is known whenever the answer reports `inputTokens`. On Anthropic
  `InvokeModel` an absent cache counter stays unknown, and so does the input total.
- **Failures.** A throttling or temporary service exception, a 429 other than an exhausted service
  quota, and a cut connection may be retried, but only before the model answered anything. After
  that every failure is final. A 429 for an exhausted service quota is final.
- **No retries inside the client.** `BedrockClient` makes one attempt; retry and fallback belong
  to routing.

## Next

- Every wire rule, retry class and source:
  [docs/bedrock.md](https://github.com/beyond10x/llm/blob/main/docs/bedrock.md).
- Bind a reference to a file, a keychain entry or an environment variable:
  [Resolve a local secret](resolve-a-local-secret.md).
