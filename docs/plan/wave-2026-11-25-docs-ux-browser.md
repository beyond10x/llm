# Wave 2026-11-25-docs-ux-browser

Status: **delivered** at `7536c2c89` on `wave/2026-11-25-docs-ux-browser`; one story
`implemented`. Wave 7 of the [wave plan](trajectory.md). Approval:
`approval-record:wave-2026-11-25-docs-ux-browser` (the owner's order of 2026-11-24 to run the next
wave from the active stories, relayed by the operator session). Opened on the head of wave 6,
`8f3abb719`, while pull request 105 waited for its merge; the integration branch took `main` before
the unit merged. Integration branch `wave/2026-11-25-docs-ux-browser`, tree `llm-1125-ux-int`.
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
| 1 | `story:docs-ux-layout-checks-from-ux-browser` | `impl/docs-ux-browser` | `llm-1125-ux` | `llm-1125-ux/target` | `af878b306` | merged at `218b38634`, correction 3 (`af878b306`) at `7536c2c89`, after two adversary passes |

Each tree is a managed worktree under the worktree tool's tree root (`llm/<tree>`). Scratch lives
outside the repository in the coordinator's scratch directory, `w34/u1`.

## Commits this wave makes

The opening plan commit, one unit commit and its corrections, the merges into the integration branch,
the closing store commit, and the merge of the integration branch into `main` through one pull
request. A release is a separate change after the wave closes.

## Close

The gate on `wave/2026-11-25-docs-ux-browser` at `218b38634`, Rust 1.98.0, each step run alone with
its own exit status, as a job of the build pool on its build node:

| Step | Exit | What it printed |
| --- | --- | --- |
| `task rust` | 0 | the three `cargo check` lines, `cargo fmt --all --check`, the clippy lines |
| `task test` (`cargo xtask test`) | 0 | 2445 tests in 5 runs, all passed (the workspace run 2131, one of them marked leaky by the runner and 1 skipped); the runs took 16.6 s of the 120 s bound; slowest 5.2 s (`llm-bedrock::process_credentials callers_after_expiry_wait_for_the_refresh_already_stalled_in_flight`), a test this wave did not touch, as the leaky one (`llm-bedrock::process_credentials a_process_the_command_leaves_holding_its_output_does_not_hold_the_answer`) |
| `task docs` | 0 | the generated pages current, example blocks match |
| `llm-conformance ess specify validate --path spec` | 0 | `llm v1 — 18 file(s), valid` |
| `llm-conformance check` | 0 | `{"total":1080,"passed":1080,"failed":0,"error":0,"unsupported":0,"skipped":0}`; `foundation conformance and deterministic ESS projections passed` |
| `aep plan artifact validate` | 0 | `valid` |

The docs UX gate at the same commit, one pool job on a site built in that job (`npm ci`, the site
build, `task ux-install`, `task ux-check`):

- `task ux-check` exit 0, wall 86.0 s: `npx tsc --noEmit`, `node cli.ts findings`,
  `npx playwright test --workers=2`, `node cli.ts ratchet`;
- the Playwright step: `113 passed (1.4m)`, wall time 83.0 s of its 120 s bound at 2 workers, the
  build node at load 15 to 20; the slowest case 8.1 s (the planted-defect case), the next 4.4 s;
- `ratchet: 32 scenarios; main baseline compared` and
  `ratchet: no value worse, no improvement left untaken`;
- no `ux-browser` and no Chromium left running after the run.

A first run at the same commit, while the gate's test job compiled in the same tree on the node,
also ended exit 0: 113 expected, 0 unexpected, 109.6 s by Playwright's own record. The copy of its
log that came back stops at case 13, so the run was repeated alone; the lines above are the second
run's.

The step "Install ux-browser" of `docs-ux.yml` runs on a forge for the first time in the "Docs UX
gate" job of this wave's pull request; the adversary ran its body once in a probe, exit 0, with the
binary the local resolver stores.

The story is `implemented` on:

- the pin by release tag and SHA-256 in one place, the step "Install ux-browser" of
  `docs-ux.yml`; locally `llm-conformance ux-browser` reads it, and
  `checks/xtask/tests/gate/ux_browser_pin.rs` holds the two readers equal;
- 135 of 135 values equal between the removed harness and the 0.8.1 evidence on one build, in five
  compared runs of the implementor; three `layout_shift.after_ready` values of `ux/baseline.json`
  moved to 0 (`explain-a-route` and `price-recorded-usage` from 0.0006, `run-a-local-turn` from
  0.0013), as the uxui repository's session had reported;
- a planted defect per check on a fixture site turning its case red
  (`ux/tests/l1-page-checks.spec.ts`); that case runs 7.6 to 8.9 s and is the one added case
  over 5 s;
- the step's wall time: before the unit 52.8 s at 4 workers and 115.2 s at 2 workers, 86 cases;
  at the unit's last commit 38.1 s at 4 workers and 90.5 s at 2 workers on the build node at load
  21 to 32, 111 cases (the adversary's pass 2); the run above is the gate's own;
- `ux/core/layout.ts` removed with the four L1 cases only it served.

## Reviews

| Pass | Covers | Verdict | Cases | Findings | Outcome |
| --- | --- | --- | --- | --- | --- |
| 1 | `bf6991be7` | red | 222 → 229, 3 red | 6 introduced (2 warnings, 4 notes), 1 pre-existing note | `review-result:adversary-w34-docs-ux-browser-pass-1`: fixed in correction 1 (`cadcf074c`); the pre-existing one is the draft below |
| 2 | `cadcf074c` | red | 183 → 187, 1 red | 4 introduced (1 warning, 3 notes) | `review-result:adversary-w34-docs-ux-browser-pass-2`: fixed in correction 2 (`37cc90f42`) |

Two passes ran, the most the wave rules allow. The coordinator read the diff of correction 2
(`cadcf074c..37cc90f42`) and ran the gate above.

What the passes found:

- Pass 1: the workflow guards missed a second step that downloads another release, a later step
  that writes `UX_BROWSER` to `GITHUB_ENV`, and a second hand-over line in the install step;
  `UX_BROWSER` naming a directory was taken; the exit file was read while it was written in place;
  eight mutants of the start, stop, signal, exit, teardown, clear and path code passed the step;
  one finding file pointed to result files that no longer carry its failure.
- Pass 2: every pass-1 finding closed, each held by a case. New: a step that uses an action of this
  repository passed the guards; the words of `AGENTS.md` ("no workflow step starts or fetches one
  of its own") said more than a text search holds; four more mutants passed; one parenthesis named
  a pool job at its limit as a runner ended by SIGKILL, and a pool job sends SIGTERM first.

## Deviations

1. The wave opened on the head of wave 6 before pull request 105 merged, and took `main` with
   wave 6 and release 0.32.0 before its unit merged. The automatic merge of the unit put its five
   CHANGELOG lines under 0.32.0; the merge commit moves them under `## [Unreleased]`.
2. The scope grew past the story's section: the resolver of `checks/conformance/src/ess.rs` made
   general over a tool, and guard tests in `checks/xtask/tests/gate/`.
3. `ux-browser` 0.8.1 answers no `--version`. A fetched asset is proved by its digest, a stored
   binary is taken on its place alone, and after each run the gate compares the version the
   evidence names with the pin. The uxui repository's release 0.9.0 carries `--version`; the pin
   move is not part of this wave.
4. The guard is a text search. `AGENTS.md` says what it reads: a name built from parts, a script
   file a step runs and an action of another repository are not found; the evidence's version check
   and the digest of the install step hold where a step hides the name.
5. The round's `ux-browser` run (`cli.ts round`) has no bound of its own, and `ux/rounds/R-01/` is
   not regenerated.
6. A runner ended by SIGKILL leaves the `ux-browser` it started running until its own run ends;
   `AGENTS.md` says so.
7. The fetch exists for `x86_64-unknown-linux-gnu` only.
8. Wave 8 opened beside this wave although `aep plan artifact waves` reports a collision of the two
   stories on `AGENTS.md`; the second of the two to merge takes `main` first.
9. The correction-1 run of the implementor ended once with an error of the harness's API and was
   resumed; its cost line is what the harness reported at the end.

## Backlog

Filed by this wave as a draft (accepting it is the owner's act):

- `story:xtask-list-check-cases-start-no-file-open-for-writing`:
  `list::a_binary_listing_its_tests_as_libtest_does_passes` failed once in 4 runs with
  `Text file busy`; the wave did not touch its file.
- `story:llm-cli-closed-stdout-case-closes-the-pipe-before-the-answer`:
  `a_reader_that_closes_stdout_before_the_first_delta_stops_the_turn_with_0` failed once in the
  gate after correction 3 and 2 of 40 times alone; the wave did not touch its file.

Not filed, because each is a pin move by the rule this repository already has: `ux-browser` 0.9.1
(with `--version`) and ESS 0.99.2.

## Cost

As the harness reported each agent run (tokens, tool uses, seconds):

| Agent | Run | Tokens | Tool uses | Seconds |
| --- | --- | --- | --- | --- |
| planning read | before the wave opened, read-only | 228,633 | 41 | 522 |
| implementor | first green | 122,605 | 314 | 3,609 |
| adversary | pass 1 | 98,885 | 96 | 2,145 |
| implementor | correction 1 | 255,882 | 36 | 396 |
| adversary | pass 2 | 250,013 | 78 | 1,098 |
| implementor | correction 2 | 87,462 | 46 | 399 |

## After the forge's first run

The pull request's first run at `587275343` (the close commit on `218b38634`) went red in the job
"Docs UX gate" after the green local gate above: the step "Install ux-browser" passed, and both
cases that start `ux-browser` failed with
`ux-browser: Chromium exited before it listened: signal: 6 (SIGABRT) (core dumped)`, 111 of 113
passed in 49.8 s at 4 workers. The six other checks passed. Read from the tool's source at 0.8.1:
`ux-browser` starts Chromium with its sandbox and prints nothing of Chromium's output when it ends
that early. That the runner's image refuses the sandbox is inferred and was not reproduced: on the
build node Chromium starts with the sandbox and without it.

Correction 3 (`af878b306`, merged at `7536c2c89`): both checks configurations carry
`chrome_args: [--no-sandbox]`, so a run here and a run on the forge start the same command line.
What makes the local gate fail on the same failure: `chromeArgs` of `ux/core/pin.ts` reads the one
line, the case "every checks configuration starts Chromium with --no-sandbox and adds no other
switch" holds each `*.checks.yaml` of `ux/`, and the planted-defect case holds its fixture's copy
and the switches its own evidence names. Not chosen: a workflow step that changes the runner's
kernel setting, and a switch given on the forge only. The uxui repository's session was told that
the tool hides the cause.

The gate again at `7536c2c89`, the same six steps and the docs UX gate:

- `task rust` exit 0; `task docs` exit 0; `llm-conformance -- ess specify validate --path spec`
  exit 0; `llm-conformance -- check` exit 0, 1080 of 1080 passed three times, failed 0, skipped 0;
  `aep plan artifact validate` exit 0;
- `task test`, first run: exit 201, `2131 tests run: 2130 passed (1 leaky), 1 failed, 1 skipped` in
  the workspace run; the four other runs passed. The failed case is
  `llm-cli::command a_reader_that_closes_stdout_before_the_first_delta_stops_the_turn_with_0`, with
  the usage line of a finished turn on stderr where it expects the closed-stdout line;
- `task test`, second run at the same commit: exit 0, 2445 tests in 5 runs
  (2131, 129, 23, 80, 82), `the runs took 13.4 s of their bound of 120 s`, the slowest case 5.2 s
  (`llm-bedrock::process_credentials an_expired_answer_makes_every_caller_wait_for_the_one_run`);
- that one case 40 times on the build node (`cargo nextest run --locked -p llm-cli --test command`
  with `--stress-count 40`): `40/40 stress run iterations: 38 passed, 2 failed`, the same stderr.
  The wave changed no file of `crates/llm-cli`; the case is at the wave's base as it is here. It
  is filed as the draft `story:llm-cli-closed-stdout-case-closes-the-pipe-before-the-answer`;
- `task ux-check` exit 0, wall 80.1 s: `npx tsc --noEmit`, `node cli.ts findings`,
  `npx playwright test --workers=2`, `node cli.ts ratchet`;
- the Playwright step: `115 passed (1.3m)`, wall time 76.2 s of its 120 s bound at 2 workers, the
  build node at load 17 to 29; the slowest case 9.7 s (the planted-defect case), the next 3.1 s;
- `ratchet: 32 scenarios; main baseline compared` and
  `ratchet: no value worse, no improvement left untaken`;
- no `ux-browser` and no Chromium left running after the run.

No adversary pass covers correction 3: the two passes the wave rules allow were spent before it.
The coordinator read its diff (5 files, 78 lines added). The forge's run on this pull request is
the check of the inferred cause.

| Agent | Run | Tokens | Tool uses | Seconds |
| --- | --- | --- | --- | --- |
| implementor | correction 3 | 165,958 | 47 | 464 |
