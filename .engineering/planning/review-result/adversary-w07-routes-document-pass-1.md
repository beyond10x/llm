---
format: aep.planning-md/3
id: review-result:adversary-w07-routes-document-pass-1
kind: review-result
status: active
title: Wave 2026-10-16-w07 adversary, story:routes-document, pass 1
relations:
- reviews: story:routes-document
revision: 1
---
```
unit: story:routes-document, tree llm-w07-routes-document at 89290256 plus one new untracked test file
verdict: red (NEEDS-CHANGE warning ×1, INFEASIBLE note ×1)
cases: executed 21→23 (default features), 20→22 (--all-features), red 2
origin: introduced 2 / pre-existing 0 / undecided 0
wrote-outside-worktree: 7 paths (part 6)
needs-coordinator: none
```

**1. Proof of what I touched.** `git --no-pager diff --stat` prints nothing because the only change is a new file. `git status --short` shows:
```
?? crates/llm-models/tests/routes_adversary.rs
```
It is the only path, and it is a test file. No implementation file was edited.

**2. Cases added** (`crates/llm-models/tests/routes_adversary.rs`). Both are red now, and both were red on their first solo run, before any suite run.

| Case | What it asserts | Red output on its first solo run |
|---|---|---|
| `yaml_aliases_cannot_expand_a_document_past_its_byte_bound_while_it_is_parsed` (`:75`). Linux only; it measures in a child run of the test binary. | Parsing a document smaller than 1/16 of `MAX_CONFIG_BYTES` raises peak RSS by no more than 8 × `MAX_CONFIG_BYTES` | `parsing a 74073 byte document raised peak RSS by 269213696 bytes (256 MiB); outcome Err((TooLarge, "catalog declarations exceed their byte bound"))` |
| `a_document_well_within_its_byte_bound_is_not_refused_as_too_large` (`:102`) | A document of 3500 plain routes with 32-byte names is accepted. It is under 1/8 of `MAX_CONFIG_BYTES` and under the 4096-declaration bound | `called Result::unwrap() on an Err value: (TooLarge, "catalog declarations exceed their byte bound")` |

**3. Suite runs** (after the cases existed):
- `RUSTUP_TOOLCHAIN=1.98.0 cargo test -p b10x-llm-models --locked`: EXIT 101. `construction` 8 ok, `routes` 13 ok, `routes_adversary` 0 passed / 2 failed.
- Same command with `--all-features`: EXIT 101. `construction` 8 ok, `routes` 12 ok, `routes_adversary` 0 passed / 2 failed.
- The "before" counts come from the implementor's `gate-test.log` (8+13) and `gate-test-all.log` (8+12).
- My file passes `rustfmt --check` and Clippy with `-D warnings`, with and without features. I made two lint-only edits after the first red run, re-ran, and both cases were still red for the same reasons.

**4. Findings** (they cover 89290256 plus the test file):

- **`crates/llm-models/src/routes.rs:546`: YAML aliases are expanded before any size check (NEEDS-CHANGE, warning, introduced).**
  - **What happened:** a 74,073-byte document cost 256 MiB of peak memory. One 64 KiB anchored reasoning-effort name was repeated 2048 times through aliases. serde_yaml 0.9.34 limits alias jumps to 100 × the number of parse events, so a list of aliases always fits under that limit. The 2 MiB check runs on the source text, and the catalog's own size check (`exceeds`) only runs after everything has been expanded and copied.
  - **Not measured, extrapolated:** a full 2 MiB document (1 MiB anchor, about 256k aliases) would try to allocate roughly 256 GiB and kill the process (a gateway, CLI or SDK) instead of returning `too-large`.
  - **What reaches it:** any caller of `Routes::parse` given an operator-written document. It needs write access to the config, which is why this is a warning and not a blocker.
  - **Suggested fix:** refuse anchors and aliases, or put a cap on the expanded size during parsing.
- **`crates/llm-models/src/routes.rs:597`: documents well under the byte bound are refused as `too-large` (INFEASIBLE, note, introduced).**
  - **What happened:** each route name is copied about 8 times into the expanded catalog, so the catalog's 2 MiB check (`catalog.rs:91`) refuses a document of about 136 KB. The doc comment at `routes.rs:537` promises `too-large` only "above MAX_CONFIG_BYTES", and the error message names catalog internals rather than the document.
  - **What reaches it:** I found nothing. It takes about 3200 or more routes, which nobody writes by hand. The fix could be a doc line, or a separate check on the expanded size.

**5. Attacked without breaking it:**
- Strictness:
  - Duplicate keys are refused at every level, including inside derived structs, flow mappings and untagged route values.
  - Unknown fields are refused, including inside `Capabilities` (it has `deny_unknown_fields`).
  - A YAML `<<` merge key is refused as an unknown field.
- No value echoed in errors:
  - The errors from the base URL, API-key header and capability checks are fixed strings.
  - serde_yaml errors are reduced to a fixed message plus location.
- `file:` paths:
  - Paths starting with `~/../` or `~//`, an empty `~/`, and paths ending in `..` are refused.
  - A `.` in the middle of a path is accepted at parse time and by `FileResolver`, which is consistent.
- Lookup and expansion:
  - Route names and direct names cannot collide, because route names contain no `/`.
  - A secret reference id is the same as a valid provider name, so that check cannot refuse one.
- Secret material:
  - The `Debug` output of `Routes`, `SecretSources` and `RouteResolvers` carries no variable name, path or value.
  - The turn-time refusal for an unset variable or a missing file is a fixed string.
- Contract against code:
  - All 24 scenarios match their Acceptance lines.
  - The conformance adapter's field formats match the comments in `spec/domains/routes.yaml`.
- One gap in the unit's own tests: no test or scenario checks the billing kind (metered or self-hosted). Swapping them would leave every test green. I did not demonstrate this with a mutant, so it is not in the findings block.

**6. Paths written outside the worktree** (all logs in the assigned scratch directory `~/.cache/llm-wave-w07/routes-document/scratch/`):
- `adversary-case1.log`
- `adversary-case2.log`
- `adversary-suite-default.log`
- `adversary-suite-all.log`
- `adversary-clippy.log`
- `adversary-rerun.log`

The seventh is a worktree session lease, `adversary-w07-routes-document-p1`. I acquired it and have already released it.

**7. Findings block**
```findings
- file: crates/llm-models/src/routes.rs
  line: 546
  category: boundary
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: "YAML aliases expand before any size check, so a 74 KB routes document costs 256 MiB of peak memory and a 2 MiB one can exhaust memory instead of being refused too-large"
- file: crates/llm-models/src/routes.rs
  line: 597
  category: boundary
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: "the expanded catalog's 2 MiB check refuses as too-large a 136 KB document of 3500 routes, contradicting the parse doc that promises too-large only above MAX_CONFIG_BYTES"
```
