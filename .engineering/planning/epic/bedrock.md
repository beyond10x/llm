---
format: aep.planning-md/3
id: epic:bedrock
kind: epic
status: draft
title: Amazon Bedrock Runtime behind the model port
summary: 'llm-bedrock: Converse and Anthropic InvokeModel on Bedrock, event-stream decoding, hand-written SigV4, injected AWS credentials'
relations:
- decomposes: initiative:llm-foundation
- serves: vision:portable-model-inference
revision: 2
---
## Outcome

A gateway that holds AWS credentials answers its callers from models hosted on Amazon Bedrock,
through the same `Model::turn` port every other wire here serves. The new crate `llm-bedrock`
speaks Bedrock Runtime Converse and ConverseStream for any model, and InvokeModel and
InvokeModelWithResponseStream with the Anthropic Messages body for Anthropic models. It decodes
AWS event-stream frames and signs each request with Signature Version 4 written on `ring`; no AWS
SDK crate and no `aws-lc` enter `Cargo.lock`. Credentials come from a source the caller injects:
static keys through a secret resolver, or the container credentials endpoint (EKS Pod Identity).

## Done when

- `story:bedrock-signing`, `story:bedrock-event-stream`, `story:bedrock-converse`,
  `story:bedrock-anthropic-invoke` and `story:bedrock-specification` are implemented on their
  acceptance evidence;
- `RUSTUP_TOOLCHAIN=1.98.0 task check` passes on the merged wave;
- `cargo tree -p llm-bedrock` lists no crate whose name starts with `aws-`;
- llm is released as its next minor through `AGENTS.md` § Release.

## Out of scope

- The llm-gateway hosted target for Bedrock (that repository's work).
- A live call to Bedrock or a run on a cluster: fixture evidence only. EKS Pod Identity is
  documented from AWS documentation and labelled unverified.
- Bedrock routes in `llm.routes/1` and the catalog: a caller constructs `BedrockClient` itself.

## Evidence

Approved by `approval-record:prefix-and-bedrock-2026-10-22`. The owner on signing, relayed
2026-10-22: "use the handwriting then".
