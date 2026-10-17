---
format: aep.planning-md/3
id: review-result:client-fields-acceptance-round-1
kind: review-result
status: active
title: Acceptance critic, round 1, epic:client-request-fields
relations:
- reviews: story:neutral-turn-controls
- reviews: story:messages-ingress-client-fields
- reviews: story:responses-ingress-client-fields
- reviews: story:chat-ingress-closed-levels
- reviews: story:client-turns-cross-wire
revision: 1
---
needs-revision
story:neutral-turn-controls — the acceptance "`wire_effort` answers every row of the reasoning table in `docs/design.md`, each row a test" cannot be met as written, because the table's last row (`reasoning_summary` hint) is a hint that `wire_effort` never returns; the bullet should list the rows it covers (effort `max`, other effort, the four thinking rows) — .engineering/planning/story/neutral-turn-controls.md:81 (rows at docs/design.md:97-105)
story:neutral-turn-controls — the acceptance "`ess specify validate --path spec` is valid" reads the same before and after the work, so it passes with `spec/domains/inference.yaml` untouched, while the Design says the spec declares the controls first; the acceptance should say the spec names `Controls` and the new types (`grep -c Controls spec/domains/inference.yaml` is 0 today) — .engineering/planning/story/neutral-turn-controls.md:87
story:messages-ingress-client-fields — the acceptance "each new rule decodes and `encode_request` re-emits it" cannot be checked as written, because rules the Design makes non-re-emitted (`metadata` to `client-metadata`, the `\n`-joined system blocks, a `prompt-cache` hint without a lifetime) would fail it; the bullet should enumerate which rules round-trip (`thinking`, `disable_parallel_tool_use`, a `1h` lifetime, effort) — .engineering/planning/story/messages-ingress-client-fields.md:71
story:responses-ingress-client-fields — the acceptance "each new rule ingests and `project_request` re-emits it" cannot be met for `input-item-ids`, `client-metadata` and the `prompt-cache` hint, which the Design and the omission table (`docs/design.md:121`) say no Responses egress sends; the bullet should name the rules that round-trip — .engineering/planning/story/responses-ingress-client-fields.md:81
story:client-turns-cross-wire — the acceptance joins two outcomes with "and", and the second ("each fails if the field it names is dropped from the projected body") gives no procedure anyone could run twice and get the same answer, such as a named mutation or a per-field assertion list; "the instructions arrive whole" is also undefined (joined by `\n`? placed where on each wire?) — .engineering/planning/story/client-turns-cross-wire.md:46

**What I read:** all 5 story ids, whole bodies, with `aep plan artifact show` for each. I also read `aep plan artifact kinds`, `aep plan artifact lifecycle story`, the epic body, `docs/design.md` "Client request fields", and the Claude Code fixture in `~/beyond10x/llm-gateway`.

**What I could not establish:**
- Whether "byte-identically to llm 0.6.0's `llm.turn/3`" has a golden to compare against. The 0.6.0 tag exists, but the acceptance names no fixture.
- Whether `task conformance` counts a skipped scenario against a ceiling. `skipped_ceiling` is in `checks/conformance/src/main.rs:49`, but I did not trace it.
- Out of my lane, so they did not set the verdict:
  - `story:responses-ingress-client-fields` has no acceptance bullet for its egress refusals (enabled budget, disabled thinking beside an effort, a conversation id that differs from `prompt_cache_key`) or for the metadata and item-id handling. That is scope or coverage.
  - `story:client-turns-cross-wire` does not say where `client_turns.rs` loads the Messages fixture from, since that fixture lives in another crate's tests. That is design coupling.
  - `story:client-turns-cross-wire` is missing from the epic's Stories list. That is scope.

```findings
- file: .engineering/planning/story/neutral-turn-controls.md
  line: 81
  category: acceptance
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "the acceptance 'wire_effort answers every row of the reasoning table in docs/design.md, each row a test' cannot be met as written, because the table's last row (reasoning_summary hint) is a hint wire_effort never returns; the bullet should list the rows it covers"
- file: .engineering/planning/story/neutral-turn-controls.md
  line: 87
  category: acceptance
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "the acceptance 'ess specify validate --path spec is valid' reads the same before and after the work, so it passes with spec/domains/inference.yaml untouched while the Design says the spec declares the controls first; it should say the spec names Controls and the new types"
- file: .engineering/planning/story/messages-ingress-client-fields.md
  line: 71
  category: acceptance
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "the acceptance 'each new rule decodes and encode_request re-emits it' cannot be checked as written, because rules the Design leaves non-re-emitted (metadata to client-metadata, joined system blocks, a prompt-cache hint without a lifetime) would fail it; the bullet should enumerate which rules round-trip"
- file: .engineering/planning/story/responses-ingress-client-fields.md
  line: 81
  category: acceptance
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "the acceptance 'each new rule ingests and project_request re-emits it' cannot be met for input-item-ids, client-metadata and the prompt-cache hint, which no Responses egress sends per docs/design.md:121; the bullet should name the rules that round-trip"
- file: .engineering/planning/story/client-turns-cross-wire.md
  line: 46
  category: acceptance
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "the acceptance joins two outcomes with 'and', its second ('each fails if the field it names is dropped from the projected body') gives no procedure that yields the same answer twice, and 'the instructions arrive whole' is undefined"
```
