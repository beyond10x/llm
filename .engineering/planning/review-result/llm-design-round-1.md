---
format: aep.planning-md/1
id: review-result:llm-design-round-1
kind: review-result
status: active
title: LLM design critic, round 1
relations:
- reviews: architecture-design:llm-boundaries
- reviews: decision-blocker:subscription-access-contract
- reviews: dependency-blocker:connectors-arbitrary-secrets
- reviews: epic:access
- reviews: epic:connectors-secret-adapter
- reviews: epic:contracts
- reviews: epic:gateway
- reviews: epic:hosting
- reviews: epic:inference
- reviews: epic:routing
- reviews: initiative:llm-foundation
- reviews: specification:declaration-domain
- reviews: story:anthropic-access
- reviews: story:catalog-routing
- reviews: story:chat-projection
- reviews: story:connectors-secret-resolver
- reviews: story:foundation-qualified
- reviews: story:gateway-auth
- reviews: story:gateway-translation
- reviews: story:hosting-contract
- reviews: story:http-streaming
- reviews: story:local-secret-adapters
- reviews: story:messages-projection
- reviews: story:modal-hosting
- reviews: story:neutral-inference
- reviews: story:openai-access
- reviews: story:operator-cli
- reviews: story:ordered-fallback
- reviews: story:provider-accounts
- reviews: story:responses-projection
- reviews: story:runpod-hosting
- reviews: story:runtime-contracts
- reviews: story:secret-resolver
- reviews: story:spending-limits
- reviews: story:usage-pricing
- reviews: vision:portable-model-inference
revision: 1
---
needs-revision

specification:declaration-domain — The mandatory Account-to-SecretReference relation cannot represent the anonymous endpoints promised by story:provider-accounts, so model credential-free bindings explicitly and retain required credentials for authenticated bindings — llm/spec/domains/catalog.yaml:28; llm/.engineering/planning/story/provider-accounts.md:17

Read 54 reviewed artifacts: llm 36, Atlas 3, Harness 5, Metaharness 4, llmgw 6; also read three linked Atlas objectives and llmgw’s deployment-owner blocker. Ran `aep plan artifact show`, `relations`, `graph`, and `validate` in each repository; walked 111 relevant edges, including edges outside the reviewed set, and checked all five stores’ dependency/blocking graphs without finding a cycle. All validators returned `valid`; existing Atlas/Harness warnings are not design findings.

Could not establish machine-resolved cross-repository dependencies: the documented AEP limitation remains, and explicit local blockers preserve the intended adoption gate. Runtime behavior and live provider qualification remain future work.

```findings
- file: llm/spec/domains/catalog.yaml
  line: 28
  category: design
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: The mandatory Account-to-SecretReference relation cannot represent the anonymous endpoints promised by story:provider-accounts, so model credential-free bindings explicitly and retain required credentials for authenticated bindings
```
