---
format: aep.planning-md/3
id: story:docs-ux-layout-checks-from-ux-browser
kind: story
status: draft
title: The docs UX gate takes its five layout checks from a pinned ux-browser release
tags:
- from-consumer-proposal
relations:
- decomposes: epic:contracts
revision: 1
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
