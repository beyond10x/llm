# Wave 2026-11-05-llm-seams

Status: **closed 2026-11-06: four stories implemented, release 0.23.0**
(`approval-record:wave-2026-11-05-llm-seams-go`, stories accepted in
`approval-record:seam-stories-2026-11-05`, corrections in `approval-record:seams-u2-correction`
and `approval-record:seams-u3-correction`). Base: `main` at `5e34b4026` (llm 0.22.0, ESS 0.86.0).
Integration branch `wave-2026-11-05-llm-seams`, tree `llm-seams-int`. Release: 0.23.0, with one
**Breaking** line: `llm_routing::Route` gains a public `kind`. Skill: AEP implementing 0.26.0.

## Units

One unit tree at a time, in this order; each tree was finished when its unit merged.

| Order | Story | Head | Tree | Depends on | Stage |
| --- | --- | --- | --- | --- | --- |
| 1 | `story:call-records` (and the ESS 0.87.0 pin) | `b0bd124c` | `llm-seams-u1` | | merged `b0de4037`; adversary red, fixed `972064d7` (merged `4df8fc4a`) |
| 2 | `story:closed-label-classifier` | `247fdadb` | `llm-seams-u2` | 1 | merged `ce859d2b`; adversary 4 red, fixed `4c8ad3a8` (merged `bbdee8d1`) |
| 3 | `story:shadow-calls` | `aebec66e` | `llm-seams-u4` | 1 | merged `5065e6da`; adversary and security review red, fixed `09bc2cc0` (merged `0f887ca6`) |
| 4 | `story:embed-route-kind` | `8c6dfdc6` | `llm-seams-u3` | 1 | merged `926524f2`; adversary red, fixed `e48fa571` (merged `160b9b85`) |

Each tree is a managed worktree under the worktree tool's tree root (`llm/<tree>`), building into
its own `target/`. Scratch lives outside the repository in the coordinator's scratch directory.

## Collisions

One line each in `spec/system.yaml`, the workspace `Cargo.toml` members and
`checks/conformance/Cargo.toml` / `src/target.rs`; separate sections of `docs/design.md`. Units 1
and 3 share `crates/llm-calls/src/lib.rs` and run in sequence. `contracts/suite.json`,
`contracts/schema/`, `contracts/ess-inputs.yaml` and `contracts/baseline.json` are regenerated or
merged by the coordinator at each merge.

## Build terms

Every cargo call through the shared build slot with the line-tables-only and non-incremental
profile; pre-check per unit (free disk at least 25G, trees budget under 20G). Units tell at 8G or 20
cargo minutes and stop at 10G or 30; unit 4 stops at 9G. Gate S1 tests, S2 Clippy, S3 docs and
specification, S4 conformance three times; two slots, cache discarded between stages.

## Reviews

An adversary pass on each unit; a security-review pass on `story:shadow-calls` (the money cap).

## Close

- All four stories implemented on `test_result`, `verification` and `review_outcome` (fixed) records.
- Every unit review found something; each was fixed in its unit before the next unit started. U2 and U3
  corrections are recorded in their approval-records. The shadow cap is per process and documented as
  such; durable persistence is the backlog draft `story:durable-shadow-cap`, which stays a draft.
- `llm_routing::Route` gains a public `kind`; a `Route` literal sets `kind: RouteKind::Turn`. No known
  consumer builds a `Route` literal; the CHANGELOG carries the **Breaking** line.
- Local gate at `84fb8353`, Rust 1.98.0, four stages, cache discarded between them. S1 tests: 1,710
  passed, 1 red (`feature_declarations` read the new embed scenarios that declare `embed_batch_limit`;
  fixed in `84fb8353`, 3/3). S2: fmt and Clippy `-D warnings`. S3: docs drift, `ess specify validate`
  (0.87.0), store valid. S4: conformance 1049/1049 three times. Peaks 12.2G, 0.6G, 0.1G and 1.3G;
  cargo seconds 407, 76, 9 and 446.
