---
format: aep.planning-md/3
id: review-result:adversary-w07-routes-document-pass-2
kind: review-result
status: active
title: Wave 2026-10-16-w07 adversary, story:routes-document, pass 2
relations:
- reviews: story:routes-document
revision: 1
---
```
unit: story:routes-document
verdict: red
cases: executed 25→28 (default features), 24→27 (--all-features), red 3
origin: introduced 3, pre-existing 0, undecided 0
wrote-outside-worktree: 9 log files in ~/.cache/llm-wave-w07/routes-document/scratch/ (part 6), plus a session lease I took and released
needs-coordinator: no
```

This covers tree `llm-w07-routes-document` at commit 3aeaed39, plus one new test file that is not tracked by git. Pass 2 found three failing cases.

**1. What I changed.** `git --no-pager diff --stat` prints nothing, because the only change is a new file. `git status --short` shows `?? crates/llm-models/tests/routes_adversary_pass2.rs`. That is a test file. No implementation file was edited.

**2. Cases I added** (`crates/llm-models/tests/routes_adversary_pass2.rs`). Each one was red the first time it ran alone, before any suite run.

| Case | What it asserts | Red output, first run alone |
|---|---|---|
| `aliases_to_a_sequence_cannot_make_a_small_document_cost_more_than_a_small_multiple_of_its_bound` | Pass 1's contract (a document under 1/16 of the byte bound raises peak memory by at most 8 × `MAX_CONFIG_BYTES`), tested with a different alias shape: `r: [&x [0 ×2000], *x ×2000]` | `parsing a 10144 byte document raised peak RSS by 131432448 bytes (125 MiB); outcome Err((InvalidRequest, "invalid llm.routes/1 YAML document at line 5 column 3"))` |
| `an_unknown_provider_field_is_refused_as_invalid_request_whatever_its_name` | A provider field named `llm.routes expansion bound` is refused as `invalid-request` | `left: (TooLarge, false)` / `right: (InvalidRequest, false)` |
| `a_direct_name_resolves_for_every_model_id_a_route_accepts` | A route to `p/<255-byte model id>` loads, so the direct name `p/<same id>` should resolve too | `left: Err((InvalidRequest, "the name is neither a route nor <declared provider>/<model-id>"))` |

After the first runs I applied rustfmt to my file (layout only, no assertion changed). I re-ran it and all three cases were still red for the same reasons. `cargo fmt --all --check` and Clippy with `--all-targets --all-features -D warnings` both exit 0.

**3. Suite runs** (after my cases existed):
- `RUSTUP_TOOLCHAIN=1.98.0 cargo test -p b10x-llm-models --locked`: EXIT 101. `construction` 8 ok, `routes` 15 ok, `routes_adversary` 2 ok, `routes_adversary_pass2` 0 passed / 3 failed.
- The same command with `--all-features`: EXIT 101. 8 ok, 14 ok, 2 ok, 0 passed / 3 failed.
- The "before" counts come from the implementor's `r1-gate-test.log` and `r1-gate-test-all.log`.

**4. Findings**
- `crates/llm-models/src/routes.rs:183`: the alias budget limits the expanded node count, not the memory used (NEEDS-CHANGE, warning, introduced).
  - Each node is charged 1 byte. But the `#[serde(untagged)]` route value (`:115`) is buffered into serde's `Content` before its type is checked, and each buffered node costs about 32 bytes.
  - A 10,144-byte document reaches 125 MiB and takes about 5.6 s before it is refused. The commit message says "so a small document cannot cost hundreds of MiB", and this is close to that.
  - Not measured: a 2 MiB document without aliases has about 1M nodes, so roughly 32 MiB.
  - What reaches it: any `Routes::parse` caller given an operator-written document.
  - Suggested fix: make the budget relative to the source, e.g. `2 × source.len()`, instead of `2 × MAX_CONFIG_BYTES`. Or refuse aliases outright.
- `crates/llm-models/src/routes.rs:164`: `malformed()` decides the error kind by searching serde_yaml's message for `EXPANSION`. That message quotes the name of an unknown field, so an unknown provider field with that name is refused as `too-large` with the alias message. The Acceptance line requires `invalid-request` (INFEASIBLE, note, introduced). The same thing happens with `DUPLICATE_KEY`. What reaches it: nothing found. Nobody names a field after the marker.
- `crates/llm-models/src/routes.rs:754`: a direct name has to be a valid `Id` of at most 256 bytes as a whole. So a model id of 250–256 bytes, which a route accepts, cannot be reached directly. This contradicts the module doc (INFEASIBLE, note, introduced). What reaches it: nothing found.

**5. Checked and not broken**
- Pass-1 corrections:
  - Case 1 is unchanged: 2048 aliases, the 8× bound, under 1/16 of the byte bound.
  - Case 2 was changed to expect a refusal that names the document. That matches the decided limit, and the assertion is an exact `(code, message)` equality.
  - The billing gap I noted in pass 1 is now pinned for each auth kind and for direct names.
- `BaseUrl` refuses userinfo and query, so no secret can enter through `Debug` that way.
- The implementor's "dense" test does assert something: it fails if the bound fires on a document without aliases.

**6. Paths written outside the worktree**, all in `~/.cache/llm-wave-w07/routes-document/scratch/`:
- `p2-case1.log`
- `p2-an_unknown_provider_field_is_refused_as_invalid_request_whatever_its_name.log`
- `p2-a_direct_name_resolves_for_every_model_id_a_route_accepts.log`
- `p2-suite-default.log`
- `p2-suite-all.log`
- `p2-clippy.log`
- `p2-rerun.log`

That is 7 logs. I also took the session lease `adversary-w07-routes-document-p2` and have released it.

```findings
- file: crates/llm-models/src/routes.rs
  line: 183
  category: boundary
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the alias budget charges one byte per node while the untagged route value buffers each as a ~32-byte Content node, so a 10 KB document aliasing one sequence 2000 times costs 125 MiB peak before refusal"
- file: crates/llm-models/src/routes.rs
  line: 164
  category: acceptance
  severity: note
  verdict: needs-revision
  origin: introduced
  message: "malformed() matches the EXPANSION marker inside serde_yaml's message, which quotes unknown field names, so an unknown provider field named after the marker is refused too-large instead of invalid-request"
- file: crates/llm-models/src/routes.rs
  line: 754
  category: boundary
  severity: note
  verdict: needs-revision
  origin: introduced
  message: "a direct name must itself be a valid Id of at most 256 bytes, so a model id a route accepts (250-256 bytes) is refused as neither a route nor a direct name"
```
