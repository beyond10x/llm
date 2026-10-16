---
format: aep.planning-md/3
id: review-result:routes-ingress-acceptance-round-1
kind: review-result
status: active
title: Acceptance critic, round 1 (epic:routes-and-ingress)
relations:
- reviews: epic:routes-and-ingress
- reviews: story:routes-document
- reviews: story:responses-ingress-encoder
- reviews: story:messages-ingress-encoder
revision: 1
---
needs-revision

story:routes-document — the unit-test paragraph is not checkable and leaves most of the Design's rules with no named check anywhere. "cover the same rules" points at the conformance list, which omits a duplicate key, a `keychain:` reference without an injected store, a reference whose feature is off, `~/` expansion against the caller's home, and the mapping form with `capabilities`. "serialising nothing from the document ever carries secret material" names no serialisation (expanded catalog, `Debug`, error text) and no assertion, so it is true whenever someone says it is — `.engineering/planning/story/routes-document.md:94`

story:routes-document — "a name naming an undeclared provider is refused" names no error code, while every other refusal in the list names `invalid-request` or `unauthorized`. The `llm.routes.Load` observation records a refused probe as `<name>!<error code>`, so the scenario cannot pin one outcome without it — `.engineering/planning/story/routes-document.md:82` (spec/domains/routes.yaml, entity `llm.routes.Loading`, field `answers`)

**What I read:** 3 of 3 ids (story:routes-document, story:responses-ingress-encoder, story:messages-ingress-encoder), whole bodies via `aep plan artifact show`. I also ran `aep plan artifact kinds` and `aep plan artifact lifecycle story`. I checked the tree: `spec/domains/routes.yaml`, `AGENTS.md`, `contracts/baseline.json` (floor 705), `crates/llm-chat/src/ingress.rs`, `crates/llm-responses`, `crates/llm-messages`, `docs/responses.md` and `docs/messages.md`.

**What I could not establish:**
- Whether `llm.responses.EncodeIngress` and `llm.messages.EncodeIngress` fit their spec domains. Both are new commands, and only `llm.chat.EncodeIngress` exists today (`spec/domains/chat.yaml:157`). That is design and scope, outside my lane.
- Whether the two ingress stories' acceptances are one statement or several. Each is a list of checks that all have to pass. I treated that as one done-condition, as the routes story does, and raised no finding on it.
- `story:messages-ingress-encoder` says `decode_request` "yields a `TurnRequest`", but it returns `IngressRequest { request, stream }` (`crates/llm-messages/src/codec.rs:15`). That is imprecise wording and still checkable, so it is not a finding.

```findings
- file: .engineering/planning/story/routes-document.md
  line: 94
  category: acceptance
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "the unit-test paragraph says tests 'cover the same rules' and that 'serialising nothing from the document ever carries secret material', naming no serialisation and no assertion, and no check at all is named for the duplicate key, feature-off or keychain-without-store refusals, ~/ expansion or the capabilities mapping the Design requires"
- file: .engineering/planning/story/routes-document.md
  line: 82
  category: acceptance
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "'a name naming an undeclared provider is refused' names no error code, so the scenario's answers entry (<name>!<error code>) is not pinned to one outcome"
```
