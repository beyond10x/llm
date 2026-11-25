---
format: aep.planning-md/3
id: story:docs-ux-layout-checks-from-ux-browser
kind: story
status: implemented
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
revision: 8
transitions:
- {from: "draft", to: "proposed", at: "2026-11-24T21:59:01Z", actor: "human:operator", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-24T21:59:02Z", actor: "human:operator", revision: 3}
- {from: "active", to: "implemented", at: "2026-11-25T10:44:12Z", actor: "human:operator", revision: 8, decided_on: {"recorded":{"test_result":1,"review_outcome":2,"verification":1}}}
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

Read by the coordinator from `git diff --stat` of the unit branch `impl/docs-ux-browser` (head
`37cc90f42`) against the integration branch before the unit merged (`aecef6e94`), at the close of
wave 2026-11-25-docs-ux-browser: 31 files. No `story-scoper` ran for this story; the section before
this one named `docs-ux.yml`, `Taskfile.yml`, `ux`, `AGENTS.md` and `CHANGELOG.md`, and did not
foresee the resolver of `checks/conformance` or the guard tests.

| Path | Scoped | What the unit did |
| --- | --- | --- |
| `.github/workflows/docs-ux.yml` | cited | the step "Install ux-browser" with `UX_BROWSER_VERSION` "0.8.1" and its `UX_BROWSER_SHA256`; the step hands the binary to the harness as `UX_BROWSER` |
| `Taskfile.yml` | cited | `ux-install`, `ux-check` and `ux-round` take the path from `llm-conformance ux-browser` and pass it as `UX_BROWSER` |
| `checks/conformance/src/ess.rs`, `src/main.rs`, `tests/pinned_ess/mod.rs` | not scoped | the resolver of the ESS pin made general over a tool; the subcommand `ux-browser` prints the pinned binary's path; process-level cases for the second tool |
| `checks/xtask/tests/gate/ux_browser_pin.rs`, `main.rs` | not scoped | new guards: one pin, both readers agree, no task and no workflow step with a `ux-browser` of its own in the forms the guard reads |
| `ux/core/layout.ts`, `ux/core/types.ts` | cited (`ux`) | `layout.ts` removed with the five page checks; `types.ts` loses the types only they used |
| `ux/core/page-checks.ts`, `evidence.ts`, `pin.ts`, `scroll-containers.ts` | cited (`ux`) | new: start and stop of `ux-browser`, the parser of `ratchet-evidence/1`, the pin's reader, what the round kept of `layout.ts` |
| `ux/core/results.ts`, `round.ts`, `ux/cli.ts`, `ux/adapter.ts` | cited (`ux`) | the ratchet takes the five numbers of a page from the evidence; the round runs `ux-browser` before its specs |
| `ux/tests/global-setup.ts`, `l2-layout.spec.ts`, `l1-core.spec.ts`, `l1-page-checks.spec.ts`, `page-checks-fixture.ts` | cited (`ux`) | `ux-browser` starts in the global setup; the last L2 case awaits its exit and its evidence; a planted defect per check on a fixture site |
| `ux/ux-browser.checks.yaml`, `ux/ux-browser.round.checks.yaml` | cited (`ux`) | new: the two checks configurations, Chrome build 1254 in both |
| `ux/baseline.json` | cited (`ux`) | three `layout_shift.after_ready` values moved to 0 |
| `ux/findings/UXF-001.md`, `UXF-002.md`, `UXF-004.md`, `UXF-005.md` | cited (`ux`) | each names where its value is measured now |
| `AGENTS.md` | cited | the invariants row, two paragraphs of "Gate", the `ux/` paragraph of "Documentation", the pin-move rule for `ux-browser` |
| `CHANGELOG.md` | inferred | five lines under `### Changed` |

Not touched: every library crate, `spec/`, `contracts/`, `website/docs/` and `README.md`.
