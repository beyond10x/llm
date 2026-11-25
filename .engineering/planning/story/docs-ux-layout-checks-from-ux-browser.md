---
format: aep.planning-md/3
id: story:docs-ux-layout-checks-from-ux-browser
kind: story
status: active
title: The docs UX gate takes its five layout checks from a pinned ux-browser release
tags:
- from-consumer-proposal
relations:
- decomposes: epic:contracts
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: .github/workflows/docs-ux.yml
- confidence: cited
  path: AGENTS.md
- confidence: inferred
  path: CHANGELOG.md
- confidence: cited
  path: Taskfile.yml
- confidence: cited
  path: ux
revision: 6
transitions:
- {from: "draft", to: "proposed", at: "2026-11-24T21:59:01Z", actor: "human:operator", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-24T21:59:02Z", actor: "human:operator", revision: 3}
---
## Context

The docs UX gate (`task ux-check`, `ux/`) runs five page checks from `ux/core/layout.ts` under
Playwright, beside axe and the screen-only user. On 2026-11-24 the uxui repository's session reported
that its `ux-browser` (Rust, its own DevTools client, pull request 11 of that repository) runs those
five checks with this repository's definitions and declarations (viewports 1440x900 and 390x844, light
and dark, the always-visible items and keyboard targets) and writes `ratchet-evidence/1`: 135 of 135
values equal to this repository's harness on the site built from `58e27a4bb`, and 30 of 30 planted
defects found. It also reported that three layout-shift values in `ux/baseline.json`
(`explain-a-route` 0.0006, `price-recorded-usage` 0.0006, `run-a-local-turn` 0.0013) read 0 on today's
build. These are that session's numbers; the coordinator did not re-run them.

Measured here on 2026-11-24 while checking pull request 93: the Playwright step of `task ux-check`
ran 119.6 s at 2 workers against its 120 s limit, 86 cases, slowest 6.6 s.

`AGENTS.md` says no code in this repository runs in anything but Rust; `ux/` is TypeScript.

## Outcome

`task ux-check` takes the five layout checks from a pinned `ux-browser` release instead of
`ux/core/layout.ts`. axe and the screen-only user stay as they are. The ratchet reads the evidence
`ux-browser` writes, and `ux/baseline.json` holds values measured on the current build.

## Acceptance

- The gate pins `ux-browser` by release tag and SHA-256, locally and in the "Docs UX gate" CI job.
- On one build, every value of the five checks equals the value the removed code produced, or the
  difference is listed with its cause.
- A planted defect per check turns the gate red.
- The UX step's wall time is reported before and after, and stays within 120 s at the worker count
  CI uses.
- `ux/core/layout.ts` and the cases that only it served are removed.

## Open

Whether a uxui release that carries `ux-browser` exists; this story waits for its tag.

## Verification

`task ux-check` on a local build and the "Docs UX gate" job. No network beyond the pinned download.

## Scope

Derived 2026-11-25 by `story-scoper` at `87bc16f16`. Every line is **cited** (read from the story or the tree) or **inferred** (a reading that could be wrong).

- **Primary surface:** `ux/` (the docs UX harness) — cited
- **Files:** `ux/core/layout.ts` — cited, high: the five checks are removed (story names the file)
- **Files:** `ux/tests/l2-layout.spec.ts:14` — cited, high: drops the five checks, keeps axe; imports `layout.ts`
- **Files:** `ux/tests/l1-core.spec.ts:8` — cited, high: the break-tests of `checkAlwaysVisible`, `keyboardWalk`, `measureOverflow` go
- **Files:** `ux/core/results.ts:5` — cited, high: `toMeasured` takes the five values from ux-browser's output, not from `layout.ts` types
- **Files:** `ux/cli.ts` (`ratchet()`) — cited, high: the ratchet reads what ux-browser wrote
- **Files:** `ux/baseline.json` — cited, high: layout-shift values re-measured (lines 494, 570, 646 hold 0.0006, 0.0006, 0.0013)
- **Files:** `Taskfile.yml:20-48` — cited, high: `ux-install`, `ux-check` gain the pinned download and the ux-browser step
- **Files:** `.github/workflows/docs-ux.yml` — cited, high: the "Docs UX gate" job installs the pinned binary and runs the step
- **Files:** `AGENTS.md:173-181` — cited, high: the paragraph that describes the gate and the L2 layout contract
- **Also likely:** a new pin file under `ux/` (tag and SHA-256) and a new ux-browser checks config under `ux/` — inferred, medium: the story asks for a pin, names no file
- **Also likely:** `ux/adapter.ts:26,41` and `ux/core/types.ts:36,50` — inferred, medium: `alwaysVisible` and `keyboard` declarations serve only the removed code
- **Also likely:** `ux/core/round.ts:33-41,142,168` — inferred, medium: reads `overflow.scrollContainers` and the five values from L2 results
- **Also likely:** `ux/findings/UXF-001.md`, `UXF-002.md`, `UXF-004.md`, `UXF-005.md` line 12 — inferred, low: `regression_asset` names an L2 layout case
- **Documents:** `CHANGELOG.md` (Unreleased) — inferred, medium
- **Symbols:** `measureOverflow`, `checkAlwaysVisible`, `keyboardWalk`, `readShift`, `CLS_INIT`, `readyTime` — cited
- **Confidence:** high — the story names `ux/core/layout.ts`, `task ux-check`, `ux/baseline.json` and the CI job; file names of pin and config are guessed
- **Would collide with:** any unit touching `ux/`, the `ux-*` tasks or a top-level `vars:` of `Taskfile.yml`, `docs-ux.yml`, the docs paragraph of `AGENTS.md`, `CHANGELOG.md` Unreleased
- **Safety fact:** `layout.ts` is imported only by `results.ts:5`, `l1-core.spec.ts:8`, `l2-layout.spec.ts:14`, but `round.ts` reads its scroll containers through `PageResult`, and the ratchet compares the key names of `ux/baseline.json` with main's, so both must survive the swap — unproven
