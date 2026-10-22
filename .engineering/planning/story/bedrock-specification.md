---
format: aep.planning-md/3
id: story:bedrock-specification
kind: story
status: draft
title: Bedrock behaviour is specified in ESS and held by conformance scenarios
summary: llm.bedrock ESS domain, conformance adapter and scenarios, docs and status
relations:
- decomposes: epic:bedrock
- serves: vision:portable-model-inference
revision: 2
---
## Change

- ESS domain `llm.bedrock` in `spec/domains/bedrock.yaml`, its scenarios under
  `contracts/bedrock/scenarios/`, listed in `contracts/ess-inputs.yaml`, and the conformance
  adapter in `checks/conformance/`.
- `docs/bedrock.md`, a website page, `docs/implementation-status.md`, README.md, AGENTS.md
  (owns list and invariant table) and CHANGELOG.md.

## Acceptance

- `ess specify validate --path spec` is valid and `task conformance` runs the new scenarios with
  none skipped; `contracts/baseline.json` rises by their count.
- The docs label EKS Pod Identity unverified and say no live Bedrock call was made.
