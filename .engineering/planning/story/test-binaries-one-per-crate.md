---
format: aep.planning-md/3
id: story:test-binaries-one-per-crate
kind: story
status: active
title: Each crate links one integration-test binary
summary: Consolidate 242 test files into one tests/main.rs per crate
relations:
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: AGENTS.md
- confidence: inferred
  path: CHANGELOG.md
- confidence: inferred
  path: checks/conformance/tests
- confidence: inferred
  path: crates/llm-bedrock/Cargo.toml
- confidence: cited
  path: crates/llm-bedrock/tests
- confidence: inferred
  path: crates/llm-blocking/tests
- confidence: cited
  path: crates/llm-calls/tests
- confidence: cited
  path: crates/llm-chat/tests
- confidence: inferred
  path: crates/llm-cli/Cargo.toml
- confidence: cited
  path: crates/llm-cli/tests
- confidence: cited
  path: crates/llm-core/tests
- confidence: cited
  path: crates/llm-cost/tests
- confidence: inferred
  path: crates/llm-credentials/examples/live_subscription_turn/turn.rs
- confidence: cited
  path: crates/llm-credentials/tests
- confidence: cited
  path: crates/llm-decide/tests
- confidence: cited
  path: crates/llm-docs/tests
- confidence: cited
  path: crates/llm-embed/tests
- confidence: cited
  path: crates/llm-http/tests
- confidence: inferred
  path: crates/llm-messages/src/client/tests.rs
- confidence: cited
  path: crates/llm-messages/tests
- confidence: inferred
  path: crates/llm-models/Cargo.toml
- confidence: cited
  path: crates/llm-models/tests
- confidence: inferred
  path: crates/llm-openai-decisions/tests
- confidence: cited
  path: crates/llm-providers/tests
- confidence: cited
  path: crates/llm-responses/tests
- confidence: cited
  path: crates/llm-routing/tests
- confidence: cited
  path: crates/llm-tool-call/tests
- confidence: inferred
  path: docs/bedrock.md
- confidence: inferred
  path: docs/design.md
- confidence: inferred
  path: docs/implementation-status.md
- confidence: inferred
  path: docs/messages.md
- confidence: inferred
  path: docs/responses.md
- confidence: inferred
  path: spec/domains/messages.yaml
- confidence: inferred
  path: website/data/status.json
- confidence: inferred
  path: website/docs/guides/call-a-model-with-one-forced-tool.md
- confidence: inferred
  path: website/docs/guides/configure-model-routes.md
- confidence: inferred
  path: website/docs/guides/use-llm-from-a-synchronous-loop.md
- confidence: inferred
  path: website/docs/guides/use-the-llm-command.md
revision: 44
transitions:
- {from: "draft", to: "proposed", at: "2026-11-25T14:06:39Z", actor: "human:operator", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-25T14:06:39Z", actor: "human:operator", revision: 3}
---
## Outcome

Each crate of the workspace links one integration-test binary, `tests/main.rs` with one module
per former file, as the workspace rule "Tests stay fast" asks.

## Why

On 0.25.0 the workspace holds 242 integration-test files, each its own binary: `llm-messages` 45,
`llm-responses` 43, `llm-chat` 24, `llm-models` 19, `llm-bedrock` 18, `llm-core` 17, `llm-http`
16, `llm-credentials` 15, `llm-routing` 14, and fewer in the rest. Every binary links the crate
and its dependencies again, which is most of the test phase's build time and disk.
`story:gate-test-budget-and-planning-skip` bounded the test phase and removed debug information
from test builds; it left the consolidation to this story because it moves every test file.

## Acceptance

- Every crate with integration tests has exactly one test binary: `tests/main.rs` with modules
  under `tests/main/` (or one directory per crate), and no other `tests/*.rs` file. Excepted are
  the three test targets with `harness = false`, which stay binaries of their own (section Open):
  `process_credentials` of `llm-bedrock`, `process_routes` of `llm-models` and `credential_process`
  of `llm-cli`. Those three crates link two test binaries, every other crate one.
- The number of tests `cargo nextest list --workspace` reports is unchanged, and every test name
  the invariants table of `AGENTS.md` cites is updated to its new path.
- The test phase's build of the test binaries (`cargo nextest run --workspace --no-run`, cold
  cache) is measured before and after, and both times are recorded as evidence.

## ESS first

None: test layout only.

## Scope

Derived 2026-11-25 by `story-scoper`, read at `main` 831c21d95. Every line is **cited** (read from the story or the tree; the source is named in brackets) or **inferred** (a reading that could be wrong).

- **Primary surface:** the `tests/` directory of every workspace crate: 247 top-level `tests/*.rs` files in 20 crates today (the story counts 242 at 0.25.0) — cited (story, tree)
- **Files, crates with more than one `tests/*.rs` file (17 crates, 244 files):**
- `crates/llm-messages/tests`: 45 — cited (story, tree)
- `crates/llm-responses/tests`: 43 — cited (story, tree)
- `crates/llm-chat/tests`: 24 — cited (story, tree)
- `crates/llm-models/tests`: 19 — cited (story, tree)
- `crates/llm-bedrock/tests`: 18 — cited (story, tree)
- `crates/llm-core/tests`: 17 — cited (story, tree)
- `crates/llm-http/tests`: 16 — cited (story, tree)
- `crates/llm-credentials/tests`: 15 — cited (story, tree)
- `crates/llm-routing/tests`: 14 — cited (story, tree)
- `crates/llm-calls/tests`: 7 — cited (tree)
- `crates/llm-providers/tests`: 7 — cited (tree)
- `crates/llm-decide/tests`: 5, with 4 more modules in `tests/classify/` — cited (tree)
- `crates/llm-tool-call/tests`: 4 — cited (tree)
- `crates/llm-cli/tests`: 3 — cited (tree)
- `crates/llm-cost/tests`: 3 — cited (tree)
- `crates/llm-docs/tests`: 2 — cited (tree)
- `crates/llm-embed/tests`: 2 — cited (tree)
- **Files, crates that already link one integration-test binary:**
- `crates/llm-blocking/tests/blocking.rs`, `crates/llm-openai-decisions/tests/client.rs` (with `tests/client/adversary_w28_wire.rs`) and `checks/conformance/tests/client_turns.rs` (with `tests/pinned_ess/mod.rs`): one file each, none named `main.rs` — cited (tree)
- `checks/xtask/tests/gate/main.rs` with 7 modules beside it: one binary, and no `tests/*.rs` file — cited (tree)
- These four change only if "no other `tests/*.rs` file" is read as "the file is named `main.rs`" — inferred
- **Files, targets that cannot become a module:** the three `[[test]]` entries with `harness = false`. Each file has its own `main` and is run as a `credential_process` command whose stdout must be exactly its answer.
- `crates/llm-bedrock/Cargo.toml:39` `process_credentials`, `required-features = ["process"]` — cited (tree)
- `crates/llm-models/Cargo.toml:49` `process_routes`, `required-features = ["process"]` — cited (tree)
- `crates/llm-cli/Cargo.toml:46` `credential_process` — cited (tree)
- With them, `llm-bedrock`, `llm-models` and `llm-cli` keep two test binaries each; the acceptance needs an exception or a rewording for them — inferred
- **Helper modules shared between test files:**
- `crates/llm-chat/tests/common/mod.rs`: `mod common;` in 21 of 24 files, `#![allow(dead_code)]` — cited (tree)
- `crates/llm-messages/tests/support/mod.rs`: `mod support;` in 42 of 45 files, `#![allow(dead_code)]` — cited (tree)
- `crates/llm-responses/tests/wire_support/mod.rs`: `mod wire_support;` in 7 of 43 files, `#![allow(dead_code)]` — cited (tree)
- `crates/llm-decide/tests/classify.rs:1035-1045`: four `#[path = "classify/…"]` modules — cited (tree)
- `crates/llm-openai-decisions/tests/client.rs:2397`: one `#[path = "client/…"]` module — cited (tree)
- `crates/llm-credentials/tests/live_subscription_turn.rs:11`: `#[path = "../examples/live_subscription_turn/turn.rs"]` — cited (tree)
- `checks/conformance/tests/client_turns.rs:11`: `mod pinned_ess;` — cited (tree)
- No other crate shares a module; there is no `tests/common/` outside `llm-chat` — cited (tree)
- Each helper has to be declared once in the crate's `main.rs` and reached as `crate::<helper>`; loading one file from several modules trips Clippy's `duplicate_mod`, which the workspace denies — inferred
- **Files, fixture and data directories read by relative path:**
- `tests/fixtures/` in `llm-bedrock`, `llm-core`, `llm-messages` and `llm-responses`, and `crates/llm-bedrock/tests/data/` — cited (tree)
- About 100 `include_str!`/`include_bytes!` calls in test files are relative to the including file, so each gains one `../` if its file moves one directory down — cited (tree)
- `checks/conformance/tests/client_turns.rs:15,18` includes fixtures of `llm-messages` and `llm-responses`; the fixture and data directories should stay where they are — inferred
- **Symbols, crate-level `#![cfg(feature = …)]` at the top of a test file:** 13 files in `llm-credentials`, `crates/llm-calls/tests/adversary_w2026_11_09_u6.rs:4` and `crates/llm-models/tests/adversary_w10_routes.rs:1`. As a module each keeps its gate; `main.rs` itself must need no feature — cited (tree)
- **Symbols, tests that run their own binary again with `--exact <bare test name>`** (the name gains a module prefix after the move):
- `crates/llm-bedrock/tests/web_identity.rs:903` and `crates/llm-bedrock/tests/security_w25_web_identity.rs:666` — cited (tree)
- `crates/llm-http/tests/ambient_proxy.rs:37` and `crates/llm-http/tests/adversary_w25_form_exchange.rs:59` — cited (tree)
- `crates/llm-credentials/tests/environment.rs:29`, with the names passed at `:58` and `:82` — cited (tree)
- `crates/llm-cost/tests/budget.rs:547` — cited (tree)
- `crates/llm-models/tests/routes.rs:1031`, `crates/llm-models/tests/routes_adversary.rs:37` and `crates/llm-models/tests/routes_adversary_pass2.rs:41` — cited (tree)
- **Documents the story names:** `AGENTS.md`, the invariants table and the "Gate" and "Never" sections: 140 distinct `path::test` citations over 56 test files on 30 lines; the table lines for "unknown usage" and "retry only before output" each cite six crates on one line — cited (story, tree)
- **Documents, live pages that cite a test path:**
- `docs/design.md` (13 lines), `docs/messages.md` (13, crate-relative `tests/<file>.rs`), `docs/bedrock.md` (21), `docs/responses.md:586` — inferred
- `docs/implementation-status.md:123`, and through it the generated `website/data/status.json:319` (`task docs-generate`) — inferred
- The `source:` line 6 of four guides under `website/docs/guides/`: `call-a-model-with-one-forced-tool.md`, `configure-model-routes.md`, `use-llm-from-a-synchronous-loop.md`, `use-the-llm-command.md` — inferred
- Comments in `crates/llm-messages/src/client/tests.rs`, `crates/llm-credentials/examples/live_subscription_turn/turn.rs` and `spec/domains/messages.yaml:246` — inferred
- `CHANGELOG.md`, one Unreleased line — inferred
- **Documents read as history and left alone:** `docs/harness-parity.md` (172 lines, cited against tag 0.7.0), `docs/plan/`, `docs/verification/` and the planning store — inferred
- **Places outside the test files that could name a test binary or path, and what each holds:**
- `.config/nextest.toml`: no filter and no per-binary override — cited (tree)
- `Taskfile.yml`: no test path and no binary name; `task test` is `cargo xtask test` — cited (tree)
- `.github/workflows/gate.yml:85,148,150` and `pages.yml:37`: run by package (`cargo xtask test`, `cargo test -p llm-credentials --all-features`, `cargo test -p llm-cost --all-features`, `cargo test --locked -p llm-docs`); none names a test target — cited (tree)
- `checks/xtask/src/main.rs:38-42` runs by package and `:240-264` reads each manifest's `[[test]]` entries with `harness = false`; no target name is written there — cited (tree)
- `checks/xtask/tests/gate/list.rs:65,72` uses the string `process_credentials` as the file name of a fake binary; `checks/xtask/tests/gate/paths.rs:79` uses `crates/llm-bedrock/tests/fixtures/README.md` as a sample prose path; `checks/xtask/tests/gate/debug_info.rs:10` reads its own binary — cited (tree)
- `[[test]]` entries: the three above and no other; `required-features` on a test target: `process` on two of them and no other; `crates/llm-docs/Cargo.toml:29` gives the example `interrupt_turn` `test = true`, a further test executable outside `tests/` — cited (tree)
- No gate step checks that a test path cited in a document exists, so a citation left behind fails nothing — inferred
- **Also likely:** the layout decides how many lines change. `mod x;` in `tests/main.rs` resolves to `tests/x.rs`, which Cargo also builds as a target of its own. "Modules under `tests/main/`" therefore needs `tests/main/main.rs` (the form `checks/xtask/tests/gate/main.rs` has), a `#[path]` per module, or `autotests = false` with the files left in place — inferred
- **Confidence:** high for where the work lands, because every count and line number above was read in the tree; medium for the three custom-harness targets and the single-file crates, which the acceptance does not settle — cited (tree)
- **Would collide with:** any unit that adds, edits or moves a file under a crate's `tests/` directory (a file added as `tests/<new>.rs` merges cleanly and is a second binary again); any unit that edits the invariants table of `AGENTS.md`; any unit that edits the `[[test]]` entries of `crates/llm-bedrock/Cargo.toml`, `crates/llm-models/Cargo.toml` or `crates/llm-cli/Cargo.toml`; any unit that consolidates `crates/llm-decide/tests` — inferred
- **Safety fact:** the move is safe only if each re-run names its test by the new module path. With a bare name `--exact` selects nothing and libtest exits 0 (`crates/llm-credentials/tests/environment.rs:44` says so). `crates/llm-bedrock/tests/web_identity.rs:903`, `crates/llm-bedrock/tests/security_w25_web_identity.rs:666` and `crates/llm-http/tests/ambient_proxy.rs:37` then pass because the proxy heard nothing, with an unchanged test count, and the invariant "Credentials never reach a redirect target or an ambient proxy" is held by nothing. The other six sites fail loudly: a `1 passed` check, a child marker, a proxied assertion, or a ledger file the child never wrote. Step 3, walked by reading, unproven — inferred
- **Safety fact, second:** under cargo-nextest every test is its own process, so merged files share nothing. The macOS and Windows job (`gate.yml:148,150`) and `pages.yml:37` run `cargo test`, where all tests of one binary share a process. `crates/llm-credentials/tests/adversary_codex.rs:83` calls `set_current_dir` and would then share a process with the other 14 files' tests on macOS. Step 2, unproven — inferred
- **Proposed split, six parts with disjoint paths.** No part moves a fixture or data directory, and parts 1 to 5 touch no document.
- Part 1: `crates/llm-messages/tests` (45 files, `support/`) — inferred
- Part 2: `crates/llm-responses/tests` (43 files, `wire_support/`) — inferred
- Part 3: `crates/llm-chat/tests` (24 files, `common/`), `crates/llm-bedrock/tests` (18 files, two re-run sites, `process_credentials`) and `crates/llm-bedrock/Cargo.toml` — inferred
- Part 4: `crates/llm-core/tests` (17), `crates/llm-http/tests` (16, two re-run sites), `crates/llm-credentials/tests` (15, feature gates, one re-run site, the `#[path]` into `examples/`), `crates/llm-routing/tests` (14), `crates/llm-providers/tests` (7) — inferred
- Part 5: `crates/llm-models/tests` (19, three re-run sites, `process_routes`) with `crates/llm-models/Cargo.toml`; `crates/llm-calls/tests` (7); `crates/llm-tool-call/tests` (4); `crates/llm-cli/tests` (3) with `crates/llm-cli/Cargo.toml`; `crates/llm-cost/tests` (3, one re-run site); `crates/llm-docs/tests` (2); `crates/llm-embed/tests` (2); the renames in `crates/llm-blocking/tests`, `crates/llm-openai-decisions/tests` and `checks/conformance/tests` if they are wanted; `crates/llm-decide/tests` (5) only if no other unit holds it — inferred
- Part 6, after the other five are merged: `AGENTS.md`, the live documents above, `task docs-generate`, `CHANGELOG.md`, the total test count and the "after" build measurement; the "before" measurement and the per-run test counts are taken on the base commit before part 1 lands — inferred
- Between the first merged part and part 6, `AGENTS.md` cites paths that no longer exist; the acceptance's second bullet closes with part 6 — inferred

## Open

Decided on 2026-11-25. The scoper found three test targets that cannot become modules of one test
binary: `process_credentials` (`crates/llm-bedrock/Cargo.toml`), `process_routes`
(`crates/llm-models/Cargo.toml`) and `credential_process` (`crates/llm-cli/Cargo.toml`), each
declared with `harness = false`. Each is its own harness and also the command its cases start as a
credential process. libtest writes `running N tests` to stdout before a test runs, and the stdout
of a credential-process command must be its answer and nothing else
(`crates/llm-bedrock/tests/process_credentials.rs`, the module comment).

The question had three options: A, the three stay binaries of their own; B, each is split into a
helper program without cases and cases inside the one binary; C, the three crates leave the story.
The operator session that acts with the owner's permissions answered: "D36 = A. The 3
`harness = false` targets stay their own binaries; every other test file becomes a module. Reason:
B links the same number of targets per crate (helper plus test binary) and rewrites about 25 cases
for no gain."

What follows from it:

- The first acceptance line names the exception; `llm-bedrock`, `llm-models` and `llm-cli` link two
  test binaries, every other crate with integration tests one.
- The three files keep their place, their `[[test]]` entries and their cases.
- The layout of the one binary is the acceptance's second form, one directory per crate:
  `tests/main/main.rs` with the modules beside it, as `checks/xtask/tests/gate/main.rs`. Cargo finds
  it without a manifest entry, and no module needs a `#[path]`.
