---
format: aep.planning-md/3
id: story:docs-current-0-5
kind: story
status: active
title: llm's documentation describes 0.5.0 and links llm-gateway's own site
relations:
- decomposes: epic:serving-split
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: AGENTS.md
- confidence: cited
  path: CHANGELOG.md
- confidence: cited
  path: README.md
- confidence: cited
  path: docs/implementation-status.md
- confidence: cited
  path: website
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-10-08T20:02:33Z", actor: "human:timo", revision: 3}
- {from: "proposed", to: "active", at: "2026-10-08T20:02:33Z", actor: "human:timo", revision: 4}
---
## Outcome

llm's documentation describes 0.5.0: the site at `https://beyond10x.github.io/llm/`, README.md and
AGENTS.md agree with the code and with every CHANGELOG entry since 0.3.0, and the site describes
nothing of the gateway beyond linking llm-gateway's own documentation and repository.

## Why

Operator request 2026-10-08: refresh llm's documentation with the workspace `docs` skill in the
shared look and feel. The status record still lists the Secrets resolver (0.3.0), the catalog
model port (0.4.0) and the provider description (0.5.0) as pending and "not in a release yet";
pages still call the Secrets resolver planned; llm-gateway now publishes its own site.

## Acceptance

- `docs/implementation-status.md` marks `secrets-resolver`, `catalog-model-port` and
  `runpod-provider-description` shipped, gives the current scenario count, and lists no gateway
  cutover or operator command line as llm's pending work; `task docs-generate` regenerates
  `website/data/status.json` and `website/docs/status.mdx`.
- No hand-written page calls the Secrets resolver planned; the roadmap lists only llm's own open
  work.
- Every mention of llm-gateway on the site, the landing page, README.md and AGENTS.md links
  `https://beyond10x.github.io/llm-gateway/` and `https://github.com/beyond10x/llm-gateway`, and
  says nothing about the gateway's internals.
- Harness links its ecosystem page, the page its own route redirects to.
- `llm-docs generate --check`, `task check` and the site build exit 0.
