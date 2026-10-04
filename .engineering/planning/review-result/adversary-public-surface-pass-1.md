---
format: aep.planning-md/3
id: review-result:adversary-public-surface-pass-1
kind: review-result
status: active
title: 'Adversary, public surface, pass 1: needs-change on two blockers'
relations:
- reviews: story:public-surface
revision: 1
---
## The pass

First adversarial pass over `story:public-surface`, wave 1, across both worktrees the unit changed:
`wave1-public-surface` over base `f63386e`, and the Atlas tree `llm-plan-atlas` over `245f3f0c`.

Verdict: **needs-change**. Eight checks written, all eight red on first run. Eleven findings: nine
introduced, two pre-existing. The pass wrote nothing into either worktree; every path in both
diffs is the unit's own.

## The claim it set out to test, and what it found

The unit could not run `atlas docs reconcile` — the generator refuses on untracked publication
files — so it replicated the generator by hand and reported its replica byte-identical to two
reference repositories.

The pass did not re-read the replica. It copied the tree into scratch, committed the copy, copied
the Atlas store beside it, and **ran the real generator**. Everything the unit produced is
byte-identical to what the generator emits: the manifest, both workflows, both marked blocks, the
documentation surface subject and both delivery records, the latter `active` rather than `planned`.

What the unit missed is the check that runs *after* those. It flipped the repository to `public`
while `display_name`, `description` and `homepage` are still empty strings. `catalog validate` and
`catalog render --check` both pass, because those fields are optional with an empty default;
`atlas docs reconcile --check` refuses. The consequence is concrete: the delivery reconciliation
guards the public repository's About box and homepage link on a non-empty description, so the
repository would ship public with a blank About box and no link to its own documentation, and
nothing would report it.

That finding is `introduced` and the pass established it rather than assumed it: reconcile against
the base store does not refuse on this repository, because it was private and unreachable there.

## The finding that would have shipped a three-major upgrade as a pin

The unit reported pinning floating action tags, which is correct and necessary — the shared gate
refuses a non-exact revision. Two of the three pins are right. The third replaces
`actions/upload-artifact@v4` with a revision that is **v7.0.1**, not v4. It is a three-major
upgrade presented in the report as a pin, and v6 onward requires a newer Actions runner and Node
runtime. No pin carries the version comment every sibling repository writes beside one.

## What the pass could not break

Recorded because it bounds the finding list. The objective correction from `O1, O2, O6` to
`O1, O3, O6` is correctly sourced: the repository's own agent file and Atlas's roadmap both say
`O1, O3, O6` at the base commit. The roster judgement is right, and the ADR amendment states
exactly what the code change does. The shared gating workflow is byte-identical to the reference.
All three documented examples run and produce what the guides say they produce. Seven documented
numeric bounds each match their source constant. The eight crates called stubs are each a five-line
file. Across all seventeen pages the pass found no claim of a working remote client, of qualified
provider access, or of a release.

## Report

```
unit: story:public-surface
verdict: NEEDS-CHANGE
cases: executed 5→13, red 8
origin: introduced 9 / pre-existing 2 / undecided 0
wrote-outside-worktree: 21 paths under home-path:sha256:f20639117ffe06c4cd490a38950f13da5fe9c31d4936e05410f9848bb20d56a3
needs-coordinator: the catalog-subject fix is an Atlas store write; administrator Gates enrollment remains unowned
```

```findings
- file: atlas/catalog/store/subjects/61746c61732e7265706f7369746f7279/6c6c6d.json
  line: 12
  category: acceptance
  severity: blocker
  verdict: NEEDS-CHANGE
  origin: introduced
  message: "the llm repository subject was flipped to visibility public with display_name, description and homepage still empty, so `atlas docs reconcile --check` refuses with repository llm public metadata is not synchronized while catalog validate and catalog render --check both pass."
- file: website/docs/status/where-this-stands.md
  line: 54
  category: acceptance
  severity: blocker
  verdict: NEEDS-CHANGE
  origin: introduced
  message: "the claim that every implemented story is evidenced by an executable conformance suite plus recorded falsification is false for http-streaming: no falsification record mutates crates/llm-http and that crate is not a dependency of checks/conformance. atlas ROADMAP.md line 216 repeats it."
- file: website/docs/status/where-this-stands.md
  line: 27
  category: contract-drift
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "runtime-contracts is listed under Implemented and tested while the planning store the same page defers to reports its status as active."
- file: website/docs/status/where-this-stands.md
  line: 9
  category: boundary
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "six of the twenty-three required stories are implemented contradicts the page's own tables, which hold nine implemented and thirteen pending, and the store, which holds eight; line 47 calls the twenty-third story deferred."
- file: website/src/pages/index.tsx
  line: 28
  category: contract-drift
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "the landing-page block titled with the routing example command is not that command's output: the selected target is deleted from the targets array while the selection field still names it, and digests are elided."
- file: .github/workflows/gate.yml
  line: 37
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "actions/upload-artifact@v4 was replaced by a revision that is v7.0.1 rather than v4, a three-major upgrade presented as a pin; v6 onward requires a newer Actions runner and Node runtime, and no pin carries the version comment every sibling repository writes."
- file: CHANGELOG.md
  line: 40
  category: contract-drift
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "the changelog has no Changed section and records nothing about gate.yml, the one tracked workflow this change rewrote, including the action major bump."
- file: AGENTS.md
  line: 47
  category: acceptance
  severity: warning
  verdict: CONFIRMED
  origin: pre-existing
  message: "the Atlas-generated release-operations block is absent and atlas release guidance --check refuses, although the story's own Scope declared the file would receive it and this repository carries a primary release unit."
- file: atlas/src/docs.rs
  line: 1229
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "the unit edited Atlas's generator to classify this repository for discovery navigation: necessary, and following an existing precedent, but outside the brief's named Atlas assignment and absent from the four judgements it reported."
- file: website/docs/guides/run-the-checks.md
  line: 38
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "the page names a catalog domain plus routing, secrets, pricing and budget, but the repository has nine specification domains and five authored suites including inference, and there is no pricing domain: the file is accounting.yaml."
- file: website/docs/concepts/neutral-boundary.md
  line: 31
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: pre-existing
  message: "every adapter checks all six coordinates before sending is present tense over a population of zero adapters; the wording is faithful to the contract document but this unit is what puts it on a public page."
```
