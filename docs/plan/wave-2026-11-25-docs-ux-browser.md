# Wave 2026-11-25-docs-ux-browser

Status: **open**. Wave 7 of the [wave plan](trajectory.md). Approval:
`approval-record:wave-2026-11-25-docs-ux-browser` (the owner's order of 2026-11-24 to run the next
wave from the active stories, relayed by the operator session). Opened on the head of wave 6,
`8f3abb719`, while pull request 105 waits for its merge; the integration branch takes `main` before
the unit merges. Integration branch `wave/2026-11-25-docs-ux-browser`, tree `llm-1125-ux-int`.
Skill: AEP implementing 0.26.0.

Consumer gap moved: none. The story was proposed by the uxui repository on 2026-11-24: its
`ux-browser` runs the five page checks this repository's docs UX gate runs in TypeScript.

## Plan

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-25, at `8f3abb719`:

```
wave 1
  story:anthropic-access (inferred)
  story:docs-ux-layout-checks-from-ux-browser (inferred)
unassessed: story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason
1 wave(s), 0 collision(s), 1 unassessed
```

N is one. `story:anthropic-access` is held by `credential-blocker:anthropic-live-qualification`.
`story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason` is wave 8 and has no Scope
section yet; it gets one before its wave. The three properties: implementable in this tree (no
credential, no live call; one pinned download from a public release), no overlap (one unit), and
it builds on wave 6, whose resolver it reuses.

The move to `implemented` needs the acceptance evidence: the gate pins `ux-browser` by release tag
and SHA-256, locally and in the "Docs UX gate" job; on one build every value of the five checks
equals the value the removed code produced, or the difference is listed with its cause; a planted
defect per check turns the gate red; the UX step's wall time before and after, within 120 s at the
worker count CI uses; `ux/core/layout.ts` and the cases that only it served are removed.

## Decisions before dispatch

| Question | Decision |
| --- | --- |
| The release | uxui 0.8.1, published 2026-11-25; asset `ux-browser-0.8.1-x86_64-unknown-linux-gnu.tar.gz`, SHA-256 `2080904f1e34dfab904a1d35dabf6b50b30b58bb0ee0b0340f2aec447ea2029e` (the release's digest field). The offered patch names 0.3.0 |
| Where the pin lives | a step "Install ux-browser" in the job "Docs UX gate" of `docs-ux.yml`, version and SHA-256 in the step's `env`, like "Install ess"; the local task reads it through the resolver of `checks/conformance/src/ess.rs`, made general over the tool |
| Proof of the version | `ux-browser` 0.8.1 has no `--version`: a fetched asset is proved by its digest, and after each run the gate compares the version the evidence names with the pin. A `ux-browser` on `PATH` is not used locally. The uxui session was asked for a `--version` |
| One bound of 120 s | `ux-browser` runs inside the Playwright run when the first measurement shows the run's `globalTimeout` covers it; otherwise the implementor reports before another bound is built |
| The round | takes its five values from `ux-browser` too, with a second configuration file; `ux/rounds/R-01/` is not regenerated |
| The Chrome build | pinned in both configuration files to the build the harness's Playwright installs; a test holds the two equal |
| The offered patch | a starting point; every number in the report is measured in this wave |

## Pre-flight

- Free disk on `/`: 60 GB (`df -h`, 2026-11-25).
- Compile cache: the host's shared sccache server through `~/.cargo/config.toml`.
- Builds and tests run as jobs of the build pool on its build node.
- An adversary's mutant or base copy builds in a managed worktree (`AGENTS.md`).

## Units

Sub-agents: implementor `aep:implementor`, adversary `aep:adversary`. The unit runs its own packages'
tests and the docs UX gate; the whole gate runs once on the integration branch.

| Unit | Story | Branch | Tree | Build directory | Head | Stage |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | `story:docs-ux-layout-checks-from-ux-browser` | `impl/docs-ux-browser` | `llm-1125-ux` | `llm-1125-ux/target` | | dispatched |

Each tree is a managed worktree under the worktree tool's tree root (`llm/<tree>`). Scratch lives
outside the repository in the coordinator's scratch directory, `w34/u1`.

## Commits this wave makes

The opening plan commit, one unit commit and its corrections, the merges into the integration branch,
the closing store commit, and the merge of the integration branch into `main` through one pull
request. A release is a separate change after the wave closes.
