# Wave 1 — protocol projections, gateway authentication, hosting lifecycle, public surface

Six stories implemented at once, each on its own branch in its own worktree, merged into
`wave/1` and closed on one run of the whole gate. Coordinator: an interactive session running the
`aep-drive:wave` skill, version 0.9.3.

## Why these six

The store computed the set. `aep plan artifact waves --kind story --status active` returns one wave
holding all six, with zero collisions and zero unassessed stories. Every one is implementable in this
checkout: no credential, no paid call, no third party. Each lands on its own crate and its own
specification domain.

Three stories were left out on purpose:

- `story:connectors-secret-resolver` is blocked by `dependency-blocker:connectors-arbitrary-secrets`.
  A blocked story leaves the set. Its exclusion is also what admits `story:hosting-contract` here: the
  two share `spec/domains/catalog.yaml`.
- `story:openai-access` and `story:anthropic-access` are blocked by
  `decision-blocker:subscription-access-contract`, and each needs a live credential and a paid call,
  so neither could be finished in this tree even unblocked.

`story:runtime-contracts` appears in the computed wave and is not dispatched. It is the umbrella
contract story and stays active until this wave's evidence is recorded against it. Before this wave it
also claimed `AGENTS.md`, `README.md` and `.github/workflows`; those moved to `story:public-surface`,
which is what removed the three collisions the verb reported.

## How the surfaces were established

Six `aep-drive:story-scoper` agents were dispatched, one per unit, read-only. The run was
interrupted by an account usage limit and resumed after it reset; all six returned. Their sections
are now the `## Scope` section of each artifact, and their typed entries are what
`aep plan artifact waves` reads.

The scopers reached the same partition the coordinator had, and independently named the reason it
would not hold as written: `spec/system.yaml`, `checks/conformance/src/main.rs`,
`checks/conformance/src/target.rs` and `checks/conformance/Cargo.toml` are structurally required by
every new specification domain, so four of the six units would have met in them. Their own
recommendation was that the coordinator take those four up front. The opening commit does exactly
that, which is why the wave computes with zero collisions.

Three findings from that pass are carried into the unit briefs rather than left in a report:
`README.md` and `AGENTS.md` belong to `story:public-surface` and are off limits to the other five,
which four of them have a reason to edit; `.github/workflows/gate.yml` is the same case for a unit
that adds a feature-gated test; and the licence file is a decision rather than a copy, because the
workspace declares `LicenseRef-B10x-Proprietary` while the sibling repositories ship Apache-2.0.

## The units

| Unit | Branch | Worktree (managed id `wave1-*`) | Build directory | Stage |
|---|---|---|---|---|
| `story:messages-projection` | `impl/messages-projection` | `wave1-messages` | `~/.cache/b10x-target/llm-wave1/messages` | merged into `wave/1` at `f9b4ba5`, after its correction round |
| `story:responses-projection` | `impl/responses-projection` | `wave1-responses` | `~/.cache/b10x-target/llm-wave1/responses` | merged into `wave/1` at `d10a2a8` |
| `story:chat-projection` | `impl/chat-projection` | `wave1-chat` | `~/.cache/b10x-target/llm-wave1/chat` | merged into `wave/1` at `f9ec6dd` |
| `story:public-surface` | `impl/public-surface` | `wave1-public-surface` | none — not a Rust unit | merged into `wave/1` at `4f8be99` |
| `story:gateway-auth` | `impl/gateway-auth` | `wave1-gateway` | `~/.cache/b10x-target/llm-wave1/gateway` | merged into `wave/1` at `1ffba7b` |
| `story:hosting-contract` | `impl/hosting-contract` | `wave1-hosting` | `~/.cache/b10x-target/llm-wave1/hosting` | merged into `wave/1` at `d937f9b`, after its correction round |

Every worktree is under `/home/timo/.local/state/worktree/trees/b10x/llm/`. Each unit's scratch
directory is `~/.cache/llm-wave-1/<unit>/` and holds its brief. The briefs share
`~/.cache/llm-wave-1/invariants.md`, which is written once and referenced rather than retyped.

Four units run at once. The limit is the operator's answer, not a measurement: one unit's build was
measured at 659 MB, and the disk had 19 GB free when the wave opened, against a floor of 10 GB.
Re-read `df -h /` as each unit returns.

The unit branches do not exist until the coordinator commits each unit's work; the worktrees are
detached checkouts of the commit each unit forks from.

`impl/messages-projection` already carries one commit made before the wave opened: the previous
session's in-flight Messages work, moved onto that branch rather than left uncommitted in the tree
every other unit forks from.

## Where the wave stands, 2026-09-21

The coordinating session was interrupted and restarted. State was reconciled from Git, the planning
store and the worktrees rather than from the previous session's account of itself; the six stages in
the table above are what that reconciliation found.

Four units are merged. Each unit's work was uncommitted in its own worktree, was committed to its own
branch by the coordinator, and was merged into `wave/1` with `--no-ff`:

| Unit | Unit commit | Merge |
|---|---|---|
| `story:responses-projection` | `0fe4b0c` | `d10a2a8` |
| `story:chat-projection` | `ebe8d34` | `f9ec6dd` |
| `story:gateway-auth` | `680e08c` | `1ffba7b` |
| `story:public-surface` | `0884b0a` | `4f8be99` |

All four applied with no conflict, including `Cargo.lock`, which two of them had changed.

Two units are under their second and final adversarial pass, dispatched against their own worktrees
with a brief file each:

- `/home/timo/.cache/llm-wave-1/hosting/brief-pass-2.md`
- `/home/timo/.cache/llm-wave-1/messages/brief-pass-2.md`

Their work is still uncommitted in `wave1-hosting` and `wave1-messages`, which is the only copy of it.

Three coordinator steps are outstanding and are named here so they are not lost to another
interruption:

1. `contracts/ess-inputs.yaml` must list every `contracts/*/scenarios/*.yaml` the six units added.
   `checks/conformance/src/gate.rs` on this branch refuses a suite that omits one, so the whole gate
   cannot pass until this is done. It happens once, after the last unit merges.
2. The sibling stub claims that `story:public-surface` pass 2 found are closed against this merged
   tree, not in the unit's worktree: the claim is about a set that only exists here.
3. `contracts/suite.json` and `contracts/schema/**` are regenerated by the coordinator after the
   last merge.

Disk at the four merges: 27 GB free, against a floor of 10 GB.

### Both second passes came back red, and what that changed

| Unit | Pass 2 | Findings | Cases |
|---|---|---|---|
| `story:hosting-contract` | needs-change | 11, all introduced: 1 blocker, 6 warnings, 4 notes | 146 → 151, red 5 |
| `story:messages-projection` | needs-change | 7, all introduced: 1 blocker, 4 warnings, 2 notes | 52 → 63, red 3 |

Recorded as `review-result:adversary-hosting-pass-2` and `review-result:adversary-messages-pass-2`.
Both are the wave's recurring class in a harder form: not fixtures that cannot reach a guard, but
guards no mutation was ever aimed at. Six such guards in hosting, nine surviving mutations of twelve
in messages.

Hosting's blocker is pass 1's blocker reached through a second slot, which is the case the wave rules
send to a fresh implementor rather than back to the same one. Both units' sessions are gone in any
case, so both correction rounds went to fresh implementors, each handed its brief file, both review
records and what had already been tried.

**The attack budget is spent for both.** The coordinator verifies each correction by reading the diff.
There is no third pass.

Three decisions the coordinator took rather than handing back:

1. Messages' blocker is fixed in code, not by writing the gap into three documents. The contract
   promises a bounded turn and `with_turn_limit` is public, so the documents are not the thing that is
   wrong.
2. For that round only, `crates/llm-http/` is the messages implementor's to edit directly. It is
   coordinator-owned and no other unit in this wave touches it, and the patch route was tried in the
   previous round with half of it silently not applying.
3. The hosting adversary's two test files and the messages adversary's three are the specification for
   their correction rounds. Neither implementor may delete, ignore or weaken one; a case believed wrong
   stays red and is argued.

`contracts/ess-inputs.yaml` lists 177 scenarios and **zero** for any of the four new domains —
responses, chat, messages and hosting — so the coordinator's manifest step is load-bearing for the
whole gate rather than tidying.

### Hosting's correction round, and what the coordinator checked

Unit commit `9b5f844`, merged at `d937f9b`. 93 Rust cases and 65 scenarios pass, three runs with
identical counts; the falsification record holds 55 entries, 13 of them added this round, each aimed
at a named guard rather than counted as a kill.

The attack budget was spent, so the coordinator verified the correction instead of opening a third
pass. Checked, in order: no `#[ignore]` anywhere in the crate; both of the adversary's artifacts —
`tests/adversary_pass2.rs` and its scenario — byte-identical to how the pass left them; and every
assertion the round removed replaced by a stronger one. Those removals were three `all()` calls over
a collection the fixture leaves empty, which is an assertion that passes whether or not the behaviour
exists. Two are now `Err(ForeignResource)` with `stopped_keys` asserted empty, and the third pins the
refusal count at seven before checking what each refusal is.

The round answered two findings as classes rather than at their sites, which is the part worth
carrying to the next wave. A hand-maintained table whose completeness nothing checks became a grid
derived from `Phase::ALL` crossed with the three unconditional writers, so a phase added to the
vocabulary fails until its rows exist. And the vacuous-`all()` class was swept: the adversary named
one site, the round found and fixed three.

### Messages' correction round, and the shared crate it needed

Unit commit `f4be1db`, merged at `f9b4ba5`. 70 Rust cases in the projection, 13 in `llm-http`, and
41 scenarios in three identical runs. The falsification record holds 39 mutations with no survivors,
11 of them killed by a Rust case that no scenario reaches.

The blocker needed a change in `crates/llm-http`, which belongs to no unit. The coordinator gave that
crate to the messages implementor for the round rather than taking a patch: the patch route was tried
in the previous round and half of it silently did not apply. The result is one additive method —
`post_sse_until` takes a caller's absolute instant and takes the minimum of it and the client's own
total — with `post_sse` left as a one-line delegate, so no existing caller changed. Both properties
that make it general are cases in that crate's own tests.

Coordinator verification: no case ignored; the adversary's three files carry their pass-2 mtimes
untouched; every pre-existing test file byte-identical; and the only removed lines are the two
narrowing predicates in the wire-name scan and a duplicate type check replaced by a stricter one
covering three fields across every accept list. The provenance the wire-name classification cites was
read back from `beyond10x/harness` at `709a2ebadcc14602b82b6f3c240350e4ddc1c88c`:
`ANTHROPIC_VERSION` and `VERSION_HEADER` are literals there.

One limit the round stated rather than hid: the falsification checker bounds a mutation's claim to
the file it mutated and no further, so a mutation could still claim a different refusal in that same
file. Falsifying the checker kills three of four probes; the fourth is that hole.

### The coordinator's own four files

| File | Before | After |
|---|---|---|
| `contracts/ess-inputs.yaml` | 177 scenarios | 374, rebuilt from the tree with no removals |
| `contracts/suite.json` | 183 selected | 391 selected, 374 authored, 0 refusals |
| `contracts/schema` | — | 137 artifacts regenerated; the only change to an untouched domain's file is `source_digest`, which covers the whole specification |
| `contracts/baseline.json` | floors 183 | floors 391, ceiling 0 |

### The gate

Run once on `wave/1`, step by step, one exit status captured per step
(`~/.cache/llm-wave-1/wave-gate.log`). **Ten steps, all exit 0. No step was skipped.**

| Step | Result |
|---|---|
| `cargo test --workspace --locked` | 452 passed, 0 failed, 0 ignored, across 77 lanes |
| `cargo test -p b10x-llm-credentials --all-features` | exit 0 |
| `cargo check -p b10x-llm-credentials --no-default-features` | exit 0 |
| `cargo test -p b10x-llm-cost --all-features` | exit 0 |
| `cargo check -p b10x-llm-cost --no-default-features` | exit 0 |
| `cargo fmt --all --check` | exit 0 |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | exit 0 |
| `ess specify validate --path spec` | `llm v1 — 11 file(s), valid` |
| `cargo run -p b10x-llm-conformance -- check` | 391 of 391 passed three times, identical counts, `sources-sha256:ee6f05fe…` |
| `aep plan artifact validate` | `valid`, 61 artifacts |

`aep plan artifact validate` also reports six plan-time reviews that recorded no findings block. All
six are the pre-wave critic rounds, not this wave's adversary passes, and none of them blocks.

### The stub claims, closed against the merged tree

`story:public-surface` pass 2 found that the pages described the sibling units as unimplemented
five-line stubs, true in its own worktree and false once the wave merged. Checked here, on the merged
branch: exactly three crates still have a five-line `lib.rs` — `llm-cli`, `llm-modal`, `llm-runpod` —
and every remaining stub claim in the site, the README and the changelog names those three and no
others. The crate page's own count agrees: eleven implemented, which is fourteen minus those three,
and its implemented table lists all six of this wave's crates.

## The interruption

All four dispatched units were killed part-way through by an account rate limit, within seconds of
each other. None had committed anything, because units in this wave do not commit: each one's work
was sitting in its own worktree, and all of it survived.

What each had reached, read from `git status` in its worktree rather than from its report:

| Unit | Work in the tree when it was killed |
|---|---|
| messages | `crates/llm-messages/tests/`, its first test files |
| responses | `crates/llm-responses/{Cargo.toml,src/lib.rs,src/binding.rs,tests/}` and the lock file |
| chat | `crates/llm-chat/{Cargo.toml,fixtures/,tests/}` and the lock file |
| public-surface | `website/`, part-written |

Each was resumed in place, with its own context, told what its tree already held and to check it
before writing. None was re-dispatched from scratch: a re-dispatched unit pays twice for work that
is already on disk, and may write a second copy of it.


## The pattern across three units: a record that claims more than it checks

Three units have now been attacked, and the same defect beat all three. It is worth naming once
here rather than three times in three review records.

**A suite whose fixtures are all the complete, in-subset case cannot reach a guard that fires
outside it.** Each unit wrote fixtures that report every counter and stay inside the pinned
subset, so every guard for a missing counter or an out-of-subset field was unreachable — and a
mutation removing that guard passed the whole suite. One unit had six such mutations survive. One
declared in its verification record that it had closed the class when a targeted variant still
survived 89 of its own cases.

The falsification record is what makes this dangerous rather than merely incomplete. A record
listing eleven or twelve killed mutations reads as a suite that falsifies. It says nothing about
the guards no mutation was ever aimed at, and that is where all three units' real defects were.

Two answers are in the tree rather than in prose. The responses unit's harness now drives both the
scenario and the Rust lane and **fails outright** if any mutation is killed by neither, so its
record cannot overstate again. And the rule-driving case — each counter absent in turn while the
others are present — replaces a fixture per instance with one case that closes the class. Both are
worth copying into the next wave's briefs rather than rediscovering.


## A whole domain's evidence could go missing and the gate would still pass

`story:chat-projection` found this and it is the most useful thing the wave has produced.

`ess verify conform synthesize --scenarios contracts` selects the scenarios that
`contracts/ess-inputs.yaml` **lists**, not the ones the tree holds. The manifest is a hand-kept list
of 177 paths. A scenario file that exists and is unlisted is never selected, the run does not
mention it, and the suite exits 0.

Measured on the chat unit's tree: 36 authored scenarios on disk, 0 of them in the manifest, whole
repository lane 183 → **187**. The four are its generated adapter scenarios. Not one of its 36
authored scenarios ran, and the gate was green.

Both sibling projection units are in the same state, so three domains' evidence would have merged
unread. The unit's own lane uses `--scenarios contracts/<domain>/scenarios` directly, which is why
each unit measured real coverage while the repository gate did not.

The instance is fixed per unit by adding the entries at merge. **The class is fixed on this
branch:** `checks/conformance/src/gate.rs` now refuses when any `contracts/*/scenarios/*.yaml` is
absent from the manifest, so a future domain cannot repeat it. The chat unit wrote that check,
compiled and linted it, confirmed it names all 36 missing files, then restored `gate.rs` to a
byte-identical hash rather than leaving a change in a file it did not own. The coordinator applied
it, and the wave branch still passes 183 in three runs.

## One gate lane is empty, in every unit and in both states

`cargo test -p b10x-llm-conformance --locked` reports `0 passed` before a unit's work and `0 passed`
after it. That is not a filter dropping cases: `checks/conformance` holds no `#[test]` at all, so the
lane is a compile-and-link check and nothing more. Its value is that an adapter which stops
compiling is caught early; its executed count is not evidence about anything and must not be read as
a unit whose cases did not move.

The lane that carries a projection unit's real coverage is the executable-specification run, and the
Rust lane is its own crate's `#[test]` count. Both move.

## What each unit owns

Everything not listed for a unit is another unit's or the coordinator's.

| Unit | Owns |
|---|---|
| messages | `crates/llm-messages`, `spec/domains/messages.yaml`, `contracts/messages/scenarios`, `checks/conformance/src/messages.rs`, `docs/messages.md`, `docs/verification/messages*` |
| responses | the same shape, for `responses` |
| chat | the same shape, for `chat` |
| gateway | `crates/llm-gateway`, `docs/gateway.md`, `docs/verification/gateway*`. Authentication and read-only route inspection only; translation is `story:gateway-translation` and is not in this wave |
| hosting | `crates/llm-provision`, `spec/domains/hosting.yaml`, `spec/domains/catalog.yaml`, `contracts/hosting/scenarios`, `checks/conformance/src/hosting.rs`, `docs/hosting.md`, `docs/verification/hosting*` |
| public-surface | `CHANGELOG.md`, `LICENSE`, `README.md`, `AGENTS.md`, `website`, `b10x.docs.yaml`, `.github/workflows`, `changes`; and in the Atlas worktree, the catalog subjects, the independent-source roster and the objective map |

Coordinator-owned, and no unit's to edit: the workspace `Cargo.toml`, `Cargo.lock`,
`contracts/ess-inputs.yaml`, `contracts/suite.json`, `contracts/baseline.json`, `contracts/schema`,
`checks/conformance/src/target.rs` and `main.rs`, `docs/implementation-status.md`, `Taskfile.yml`,
and everything under `.engineering`.

## What the opening commit pre-wired

So that no two units need the same file:

- `spec/system.yaml` lists `llm.responses`, `llm.chat` and `llm.hosting`, and
  `spec/domains/{responses,chat,hosting}.yaml` each declare their domain and nothing else. A
  header-only domain generates no scenarios, so the suite count is unchanged at 183.
- `checks/conformance/src/{messages,responses,chat,hosting}.rs` are stub modules, each exposing
  `VIEWS` and an `observe` that returns `None`. Each unit replaces its own file.
- `checks/conformance/src/target.rs` tries those four hooks before its existing command arms, and
  builds its view allowlist from their `VIEWS`. A new domain is now a new file, not an edit here.
- `checks/conformance/Cargo.toml` depends on the four crates.

## The gate

Each unit runs a package-scoped gate in its own worktree and quotes it. The whole gate runs once,
later, on `wave/1`, step by step with one exit status captured per step. One test result recorded
against that merge commit closes every story in the wave.

## The commits this wave is authorised to make

The opening commit on `wave/1`; the carry commit already on `impl/messages-projection`; one commit
per unit; six merges into `wave/1`; the closing commit; the merge of `wave/1` into
`plan/llm-foundation`; the push of `plan/llm-foundation` to origin; and one commit in the Atlas
worktree. No tag, no release, no merge into `main`, and no push of Atlas or Harness, whose existing
history a source gate still refuses for reasons that predate this work.
