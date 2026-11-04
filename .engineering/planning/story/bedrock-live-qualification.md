---
format: aep.planning-md/3
id: story:bedrock-live-qualification
kind: story
status: draft
title: Bedrock Converse and InvokeModel are qualified with live turns
tags:
- from-adopter-report
relations:
- decomposes: epic:bedrock
- serves: vision:portable-model-inference
revision: 1
---
## Context

A downstream adopter reading llm 0.20.0 noted that no provider route is qualified live. Verified on
2026-11-04: `README.md:10` ("no provider route is qualified yet"); `docs/implementation-status.md`
rows `bedrock-converse` and `bedrock-anthropic-invoke` say "Fixture evidence only"; `openai-access`
and `anthropic-access` are pending; `foundation-qualified` is a draft. The Anthropic subscription
run waits on `credential-blocker:anthropic-live-qualification`; no story covers a live Bedrock run.

## Outcome

One live Bedrock turn per API is recorded as evidence, so the Bedrock rows stop saying fixture only.

## Acceptance

- A report records one Converse turn and one Anthropic InvokeModel turn against a real Bedrock
  region, streaming and not, with a tool call, naming the model ids, region and credential source.
- At least one turn uses container credentials (EKS Pod Identity) or web identity (IRSA) from a pod;
  the report says which.
- Usage counters, stop reasons and failure classes observed are compared with the fixtures; every
  difference becomes a fix or a story.
- `docs/implementation-status.md` and `docs/bedrock.md` cite the report.

## Needs

An AWS account with Bedrock model access, an IAM role, a paid call, and for the pod case an EKS
cluster: credentials and spending are the operator's decision. No agent runs it. No gate runs it.
