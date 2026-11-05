# Wave 2026-11-05-llm-seams

Status: **open 2026-11-05** (`approval-record:wave-2026-11-05-llm-seams-go`, stories accepted in
`approval-record:seam-stories-2026-11-05`). Base: `main` at `5e34b4026` (llm 0.22.0, ESS 0.86.0).
Integration branch `wave-2026-11-05-llm-seams`, tree `llm-seams-int`. Release: 0.23.0 (additive).
Skill: AEP implementing 0.26.0.

## Units

One unit tree at a time, in this order; each tree is finished when its unit merges.

| Order | Story | Branch | Tree | Depends on | Stage |
| --- | --- | --- | --- | --- | --- |
| 1 | `story:call-records` (and the ESS 0.87.0 pin) | `impl/call-records` | `llm-seams-u1` | | planned |
| 2 | `story:closed-label-classifier` | `impl/closed-label-classifier` | `llm-seams-u2` | 1 | planned |
| 3 | `story:shadow-calls` | `impl/shadow-calls` | `llm-seams-u4` | 1 | planned |
| 4 | `story:embed-route-kind` | `impl/embed-route-kind` | `llm-seams-u3` | 1 | planned |

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
