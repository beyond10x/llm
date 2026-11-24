# Wave 2026-11-24-rubric-judge

Status: **delivered** on the integration branch at `bcad20939`; `story:rubric-judge` is `implemented`.
Wave 1 of 3 of the [wave plan of 2026-11-24](trajectory.md). Approval:
`approval-record:wave-2026-11-24-rubric-judge` (the owner's order of 2026-11-24 to run the next wave
from the active stories, relayed by the operator session). Base: `main` at `c7c2a62cb` (llm 0.26.0).
Integration branch `wave/2026-11-24-rubric-judge`, tree `llm-1124-int`. Skill: AEP implementing 0.26.0.

## Plan

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-24, the lines naming
this wave's story:

```
wave 4
  story:rubric-judge (inferred)
collision: story:anthropic-access story:rubric-judge checks/conformance/src/decisions.rs (inferred)
collision: story:anthropic-access story:rubric-judge spec/domains/decisions.yaml (inferred)
collision: story:offline-replay-eval story:rubric-judge docs/design.md (inferred)
collision: story:openai-decisions-client story:rubric-judge checks/conformance/src/decisions.rs (inferred)
collision: story:openai-decisions-client story:rubric-judge contracts/decisions/scenarios (inferred)
collision: story:openai-decisions-client story:rubric-judge docs/design.md (inferred)
collision: story:openai-decisions-client story:rubric-judge spec/domains/decisions.yaml (inferred)
```

N is one. The story's scope is `inferred`; with one unit no pair runs at once. The three properties:
implementable in this tree (no credential, no live call), no overlap (one unit), one package
(`llm-decide`, with its conformance adapter and scenarios).

The move to `implemented` needs the acceptance evidence: the six scenarios answered by
`llm-conformance check` and the raised `contracts/baseline.json`.

## Pre-flight

- Free disk on `/`: 50 GB (`df -h`, 2026-11-24); the build pool reports its 100 GB floor as not met.
  The unit builds one tree.
- Compile cache: the host's shared sccache server through `~/.cargo/config.toml`.
- No tree of an earlier wave holds a build directory (13 trees, 25 to 29 MB each).

## Units

Sub-agents: implementor `aep:implementor`, adversary `aep:adversary`. The unit runs its own package's
tests and the conformance check; the whole `task check` runs once on the integration branch.

| Unit | Story | Branch | Tree | Build directory | Head | Stage |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | `story:rubric-judge` | `impl/rubric-judge` | `llm-1124-judge` | `llm-1124-judge/target` | `20f06fa6` | implementor green at `966bf1e1` (llm-decide tests 15 to 31, conformance 1049 to 1055); adversary pass 1 red, 8 introduced, 6 red cases (`review-result:adversary-w27-rubric-judge-pass-1`); correction round 1 by the same implementor green at `bf58d42a` (llm-decide tests 39 to 48); adversary pass 2 red, 4 introduced, 2 red cases (`review-result:adversary-w27-rubric-judge-pass-2`); correction round 2 by the same implementor green at `20f06fa6` (llm-decide tests 52 to 55), read by the coordinator; merged as `bcad2093` |

Each tree is a managed worktree under the worktree tool's tree root (`llm/<tree>`). Scratch lives
outside the repository in the coordinator's scratch directory, `w27/u1`.

## Commits this wave makes

The opening plan commit, one unit commit and its corrections, the merges into the integration branch,
the closing store commit, and the merge of the integration branch into `main` through one pull
request. The release 0.27.0 is a separate change after the wave closes.

## Close

The gate on `wave/2026-11-24-rubric-judge` at `bcad20939`, Rust 1.98.0, each step run alone with its
own exit status:

| Step | Exit | What it printed |
| --- | --- | --- |
| `task rust` | 0 | the three `cargo check` lines, `cargo fmt --all --check`, the four clippy lines |
| `task test` (`cargo xtask test`) | 0 | 1839 tests passed, 1 skipped, 1 leaky, 6.0 s of the 120 s bound; slowest 5.2 s (`llm-bedrock::process_credentials`); the slowest test of this wave 2.2 s (`llm-decide::judge an_answer_of_any_make_is_read_in_time_proportional_to_its_length`) |
| `task docs` | 0 | the three generated pages current, example blocks match |
| `ess specify validate --path spec` | 0 | `llm v1 — 18 file(s), valid` (ess 0.87.0) |
| `task conformance` | 201, then 0 | first run: `schema drift: regenerate contracts/schema with ESS`; rerun: 1055 of 1055, failed 0, skipped 0, three runs |
| `aep plan artifact validate` | 0 | `valid` |

No step skipped itself.

## Reviews

`review-result:adversary-w27-rubric-judge-pass-1` (8 findings, 6 red cases) and
`review-result:adversary-w27-rubric-judge-pass-2` (4 findings, 2 red cases), each with a
`review_outcome` record `fixed` against the story. After the second pass the coordinator read the
diff of correction round 2: no assertion was dropped; the 100 and 200 level assertions and the 64 and
65 level schema cases moved into two cases that also test both sides of each bound; the adversary's
second file is as delivered plus one comment line and one line that prints the timing pair.

Findings that were there before the wave went to the backlog as drafts, and no wave of this plan
takes them: `story:classifier-takes-only-a-whole-answer` (the class the judge was corrected for, in
the classifier's JSON-schema path) and `story:ess-pin-current-and-local-gate-finds-it`.

## Deviations

- The first conformance run of the gate exited 201. The build pool ran the step on its build node,
  whose `ess` is 0.98.0; `contracts/schema` is generated with 0.87.0, the version CI pins. The rerun
  carried the 0.87.0 binary in the tree for that one job and removed it after.
  `story:ess-pin-current-and-local-gate-finds-it` is the draft for this.
- Two cases assert a ratio of wall times ("four times the answer under eight times the time", the
  fastest of three and of five runs). The adversary's case measured ratios of 2.9 to 4.5 in five runs. A loaded
  runner can make such a case fail without a defect; the first such failure is a finding against
  these two cases, not a reason to rerun.
- The adversary's reports were recorded with workstation paths replaced by names relative to the
  repository and the scratch directory.
- `serde_json` gained the features `float_roundtrip` and `unbounded_depth` in `llm-decide`. Cargo
  unifies features, so every crate of a workspace build reads numbers with `float_roundtrip`
  (`cargo tree --workspace -e features -i serde_json`).

## Cost

Sub-agent usage as the harness reported it at each stop (tokens, tool uses, wall time):

| Agent | Stop | Tokens | Tool uses | Wall time |
| --- | --- | --- | --- | --- |
| implementor | first green | 101,089 | 177 | 2,836 s |
| implementor | correction round 1 | 227,154 | 74 | 2,916 s |
| implementor | correction round 2 | 81,547 | 54 | 658 s |
| adversary | pass 1 | 160,096 | 58 | 601 s |
| adversary | pass 2 | 236,011 | 26 | 3,660 s |

Executed cases in `llm-decide`: 15 before the wave, 55 after. Conformance: 1049 to 1055.
