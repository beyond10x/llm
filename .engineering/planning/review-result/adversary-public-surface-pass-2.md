---
format: aep.planning-md/1
id: review-result:adversary-public-surface-pass-2
kind: review-result
status: active
title: 'Adversary, public surface, pass 2: pages true here and false in the wave, plus three operator blockers'
relations:
- reviews: story:public-surface
revision: 1
---
## The pass

Second and final adversarial pass over `story:public-surface`, wave 1, across both worktrees.
Sixteen checks written: six on new ground, ten re-establishing pass 1's findings from the
artefacts rather than from the unit's account. Six red. Verdict: **needs-change**.

## The four corrections it was asked to verify all hold

Each was checked by running the real thing, not by reading the claim.

The catalog metadata gate: the pass committed a copy of the tree, copied the store, and ran the
real generator. It clears, and a control with the three fields re-emptied refuses — so the unit's
reasoning about a gate it could not observe was right. The false universal: all five table rows
match all five falsification records exactly, including the crate that carries none. The action
pins: each resolved through the API, all three exact. The counts: the store holds 24 stories and 8
implemented, the page's rows sum to 24, and its 8 are exactly the store's 8.

The pass also disclosed two of its own checks that were red for its reason rather than the unit's,
and fixed its harness instead of reporting them.

## The finding that is about the wave rather than the unit

The site says the protocol projections, the gateway and hosting are unimplemented five-line stubs,
across nine locations including the landing page, three status pages, the changelog and two Atlas
documents. **That is true in this worktree and false in the wave.** The five sibling worktrees
already carry implementations of 1136 to 2447 lines, and all five are merging into the same branch
these pages merge into.

Nothing the unit could do on its own tree would catch this. It is a statement about a set that
only exists at integration, which makes it the coordinator's to close and the reason it is
recorded here rather than sent back alone.

## Three blockers that no agent can close

All three are the Atlas half, and each is an operator action or a sequencing constraint rather
than a defect in the work:

The catalog now declares the repository public with a description and a homepage. GitHub reports
it private, with the old description and no homepage, and the generator compares exactly those
three fields in a check the Atlas gate runs unscoped. Of forty repository subjects this is the only
one that disagrees — and it disagrees *because* the pass-1 correction filled the fields that had
been empty, since empty skipped the comparison.

The repository is now in the Pages façade roster and has no Pages site, so the roster check gets a
404 where the control repository returns its site.

And the portal check refuses with source-lock roster drift naming this repository, against a change
that exits 0 on the base store. The story's scope names the website roster as another owner's step;
naming it does not stop the gate refusing. The Atlas commit has to be sequenced behind that change.

## Report

```
unit: story:public-surface
verdict: NEEDS-CHANGE
cases: executed 8→24, red 6
origin: introduced 6 / pre-existing 0 / undecided 0
wrote-outside-worktree: 20 paths, 179 MB peak, deleted
needs-coordinator: repository visibility, description, homepage and Pages are operator actions nothing names; the website roster must carry this repository before the Atlas change can merge green
```

```findings
- file: atlas/catalog/store/subjects/61746c61732e7265706f7369746f7279/6c6c6d.json
  line: 12
  category: acceptance
  severity: blocker
  verdict: CONFIRMED
  origin: introduced
  message: "the catalog declares the repository public with a description and homepage while GitHub reports private, the old description and no homepage; the generator compares exactly those three and the Atlas gate runs that check unscoped, and this is the only one of forty subjects that disagrees."
- file: atlas/catalog/store/subjects/61746c61732e646f63756d656e746174696f6e2d73757266616365/6c6c6d2f646f6373.json
  category: contract-drift
  severity: blocker
  verdict: CONFIRMED
  origin: introduced
  message: "the repository is now in the Pages facade roster the delivery check reads Pages settings for, and its Pages endpoint is a 404 while the control repository returns its site."
- file: website/sources.yaml
  category: contract-drift
  severity: blocker
  verdict: CONFIRMED
  origin: introduced
  message: "the portal check, which the Atlas gate and its fence script both run, refuses with source lock roster drift naming this repository against this change and exits 0 against the base store, so the Atlas commit must be sequenced behind a website roster change."
- file: website/docs/reference/crates.md
  line: 37
  category: acceptance
  severity: blocker
  verdict: NEEDS-CHANGE
  origin: introduced
  message: "the claim that each of these crates is a five-line stub, and eight other locations saying the projections, gateway and hosting are unimplemented, are falsified by the wave's own integration branch, where five sibling worktrees already carry implementations of 1136 to 2447 lines."
- file: CHANGELOG.md
  line: 34
  category: contract-drift
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "the changelog names four executably verified domains, omitting two that account for 83 of the 183 committed scenarios and naming one that is a suite directory rather than a domain, which is the same defect the unit corrected in another page."
- file: .github/workflows/gate.yml
  line: 13
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "the version comment on one pin names a moving branch that has since diverged from the pinned revision, so that pin cannot be re-verified from its own comment; five sibling repositories carry the same pair and one has already moved."
```

## What the pass attacked and could not break

Reconciliation end to end: 29 manifests, 29 collections, 27 producer callers, 29 Pages callers and
28 of each marked block are byte-current for this repository. Catalog validation, the rendered
projection check and release guidance all pass. All three documented examples run and produce what
the pages say, including the landing page's block matching one element of the real output exactly.
Nine documented numeric bounds and all eight format versions match their source constants. Every
documented Rust path compiles by signature. The shared gating workflow is byte-identical to both
reference repositories. Across all seventeen pages the pass found no claim that a projection, the
gateway, hosting or provider access works — the risk it was told to look for is absent, and its
mirror is the stale-stub finding above.

It also established that three unrelated repositories fail the same reconciliation check on the
base store, and patched them in scratch to reach this repository's checks rather than reporting
somebody else's drift as this unit's.
