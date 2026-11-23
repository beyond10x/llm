# R-01 triage

Applied by rule (ui-iteration-loop § 3.3), without a person:

```
priority = severity weight × frequency × confidence ÷ cost
S1 = 9, S2 = 3, S3 = 1; frequency = pages affected ÷ 26 pages; cost S = 1, M = 2, L = 4
```

## Caveats

- Confidence is 1 for every finding: each has one source, the L2 metric. The two review agents of
  § 8 and the L6 trial sessions did not run in this round, so no second source can agree.
- R-01 is the first round: there is no earlier change set whose metrics could have moved.
- The screen-only user's frequency is per task, not per page; no task failed, so no finding comes
  from gate 4. UXF-001 names `screen_only.actions` as its second metric.

## Did the last change sets move their metrics?

| round | change | target metric | moved |
|---|---|---|---|
| none | first round | – | – |

## Change set

No finding is S1. All four fit the set of at most 5 changes, in priority order:

| order | finding | severity | frequency | confidence | cost | priority | target metric, R-01 value |
|---|---|---|---|---|---|---|---|
| 1 | UXF-003 links told apart by colour only | S2 (3) | 22/26 = 0.85 | 1 | S (1) | 2.54 | `axe.serious` 272 nodes |
| 2 | UXF-001 no search control | S2 (3) | 26/26 = 1.00 | 1 | M (2) | 1.50 | `always_visible.misses` 208 of 362, `keyboard.misses` 52, `screen_only.actions` 38 over 5 tasks |
| 3 | UXF-004 theme toggle hidden on a phone | S3 (1) | 26/26 = 1.00 | 1 | S (1) | 1.00 | `always_visible.misses` 104 of 362 |
| 4 | UXF-002 sidebar leaves the viewport at the footer | S3 (1) | 25/26 = 0.96 | 1 | S (1) | 0.96 | `always_visible.misses` 50 of 362 |

No change is a layout decision that needs a yes/no: each restores an element the adapter already
lists as always visible, or follows WCAG 1.4.1, so there is no `decision.md`.

Each change is built failing-first. Its regression asset is the L2 gate with the finding's waiver
removed, and for UXF-001, UXF-003 and UXF-004 a test in `ux/tests/l2-findings.spec.ts`; those four
tests failed on the R-01 build (`search box in the navbar` not found, `links without an underline`
in both themes, `theme toggle in the navbar` not found).

## Result of the change set

Measured after the four changes on the rebuilt site (gating viewports, both themes; the measured-only
viewports ran too). `ux/baseline.json` takes these values in the same change.

| target metric | R-01 | after | finding |
|---|---|---|---|
| `axe.serious` (nodes, all pages) | 272 | 0 | UXF-003 |
| `always_visible.misses` (all pages) | 362 | 0 | UXF-001, UXF-002, UXF-004 |
| `keyboard.misses` (all pages) | 52 | 4 | UXF-001; the 4 are `/llm/search`, UXF-005 |
| `screen_only.actions` (5 tasks) | 38 | 16 | UXF-001 |
| `screen_only.looks` (5 tasks) | 27 | 21 | UXF-001 |
| `screen_only.steps` (5 tasks) | 43 | 21 | UXF-001 |

Gates: 1 pass on 108 units, 2 pass on 108, 4 pass on 5 tasks, 5 pass on 106 and waived on 2 (the
skip-link check of `/llm/search`, UXF-005, open). The search page adds 4 moderate axe nodes
(`landmark-one-main`, `region`), which no gate counts.

New finding from the change set: UXF-005 (S3), left open with its reason in the record.
