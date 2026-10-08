---
format: aep.planning-md/3
id: review-result:adversary-w16-llm-responses-client-pass-1
kind: review-result
status: archived
title: Wave 2026-10-04-w16 adversary, llm story:responses-client, pass 1
relations:
- reviews: story:responses-client
revision: 2
transitions:
- {from: "active", to: "archived", at: "2026-10-08T10:43:37Z", actor: "human:timo", revision: 2}
---
```
unit: llm/responses-client, working tree on 4a4dcc61 + uncommitted phase 2 (worktree llm-w16-responses-client)
verdict: NEEDS-CHANGE
cases: executed 553→567, red 4
origin: introduced 5 / pre-existing 0 / undecided 0
wrote-outside-worktree: 6 paths (part 6)
needs-coordinator: whether F3/F4 (no request/outcome validation, unlike Messages and Chat) hold the unit
```

The client fails 4 of my 14 new cases. All 4 come from checks that `MessagesClient` and the Chat client make but `ResponsesClient` does not. Everything else on the attack list held, and M5 is still caught on the `while let` shape.

**1. Diff.** `git --no-pager diff --stat` shows only the implementor's tracked files: README.md, Cargo.toml, lib.rs, implementation-status.md, responses.md. My only change is one untracked test file, `?? crates/llm-responses/tests/adversary_client.rs`. No non-test path is mine.

**2. Red cases**, run alone before the suite (log: `scratch/adversary-p1-red.log`):

| Case | Red output |
|---|---|
| `an_oversized_projected_body_is_refused_as_a_valid_unsent_failure` | `left: Err(Error { code: Protocol, message: "unsent failure carries upstream observations", dispatch: NotSent, .. })` |
| `an_oversized_projected_body_is_refused_before_the_credential_is_resolved` | `the credential was resolved for a body that was never going to be sent`, `left: 1 right: 0` |
| `a_request_beyond_the_binding_capabilities_is_refused_before_sending` | `a request beyond the binding's declared capabilities was sent` |
| `a_forced_tool_answered_with_another_tool_is_refused` | `the client returned a call to ["shell_exec"] for a turn that forced file_read` |

Mutants, run in one scratch copy with its own target dir, deleted afterwards:
- **Removing `if last { break; }`:** the unit's `tests/client.rs` stays 4/4 green. My `a_completed_response_on_a_connection_left_open…` and `an_error_event_mid_stream…` go red.
- **M5 on the `while let` shape** (a completion made up on a clean close): the unit's `a_stream_closed_cleanly_before_completion_is_refused_not_completed` goes red, so it still catches it.

**3. Gate**, on `+1.98.0` with the unit build dir:

| Step | Exit | Summary |
|---|---|---|
| `cargo fmt --all --check` | 0 | the first run failed on my file only; I reformatted it, no assertions changed |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 | `Finished`, re-run after touching the new file |
| `cargo test -p b10x-llm-responses --locked --no-fail-fast` | 101 | `adversary_client: FAILED. 10 passed; 4 failed`; the other 8 targets are ok (74 passed) |
| `cargo test --workspace --locked --no-fail-fast` | 101 | 89 summary lines: passed=563 failed=4 ignored=1; `1 target failed: -p b10x-llm-responses --test adversary_client` |

`--list` in this tree shows 14 tests.

**4. Findings**

| # | Where | Verdict | What reaches it |
|---|---|---|---|
| F1 | `crates/llm-responses/src/client.rs:124` | NEEDS-CHANGE | `attach` adds an observation to the transport's `NotSent` refusals (body over the limit, cancelled before sending). That breaks `Error::validate_for`, so routing's fallback (`fallback.rs:267`) records `AmbiguousDispatch` and halts. Reached by a neutral request under 16 MiB whose projected body is over it, or by a cancel racing the send. Fix: don't attach when the error is `NotSent`. |
| F2 | `crates/llm-responses/src/client.rs:81` | CONFIRMED | The encoded body is never checked against `MAX_REQUEST_BYTES` before the credential is resolved. `MessagesClient` checks it in `encode_request`. |
| F3 | `crates/llm-responses/src/client.rs:81` | NEEDS-CHANGE | `TurnRequest::validate_for(provenance, capabilities)` is never called, so `max_output_tokens` above the binding's declared limit went out on the wire. Routing callers are protected (`selection.rs:159`); direct callers such as intake are not. |
| F4 | `crates/llm-responses/src/client.rs:101` | NEEDS-CHANGE | `TurnOutcome::validate_for(request, provenance)` is never called, so a turn that forced `file_read` returns `Ok` with a `shell_exec` call. A model that ignores a forced tool reaches this; `fallback.rs:249` covers routing callers only. |
| F5 | `crates/llm-responses/src/client.rs:135` | CONFIRMED | No unit test holds a connection open after the terminal event, so removing the stop-reading `break` survives the unit's own suite. My new case now covers it. |

**5. Attacked and not broken** (10 green cases):
- **Framing:** CRLF, comment keep-alives, and the stream sent in 7-byte writes.
- **Stream events:**
  - an `error` event mid-stream gives `RateLimited`/`Accepted` after the events before it, and no provider text is relayed;
  - `response.incomplete` gives `MaxOutputTokens`;
  - `response.failed` gives `Unavailable` and keeps its counters.
- **HTTP status:** 401, 429 (with `Retry-After`) and 500 come back typed. An endless error body is not read, and neither the body nor the bearer appears in `Debug` or `Display`.
- **Bounds:**
  - a stream that never ends stops on `total`;
  - `total` includes credential resolution;
  - a resolver that never answers gives `Deadline`/`NotSent` with no connection.
- **Concurrency:** two turns at once on one client stay independent.
- **Headers:** the only headers sent are accept, authorization, content-length, content-type and host.
- **Docs:** `docs/responses.md` steps 1–4 and the README claim match the code, apart from F1–F4.

**6. Written outside the worktree**
- `~/.cache/ga-wave-2026-10-04-w16/llm-responses-client/scratch/adversary-p1-red.log`
- `~/.cache/ga-wave-2026-10-04-w16/llm-responses-client/scratch/adversary-mutants.log`
- `~/.cache/ga-wave-2026-10-04-w16/llm-responses-client/scratch/adversary-pkg-test.log`
- `~/.cache/ga-wave-2026-10-04-w16/llm-responses-client/scratch/adversary-ws-test.log`
- `~/.cache/ga-wave-2026-10-04-w16/llm-responses-client/scratch/mut` and `scratch/mut-target`: the mutant copy and its build dir, both deleted.
- `~/.cache/b10x-target/llm-w16-responses-client`: the existing unit build dir, reused.

Free disk is now 13G.

```findings
- file: crates/llm-responses/src/client.rs
  line: 124
  category: contract-drift
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: "attach puts an observation on the transport's NotSent refusals, producing an error Error::validate_for refuses, which routing fallback records as AmbiguousDispatch"
- file: crates/llm-responses/src/client.rs
  line: 81
  category: boundary
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "a projected body over MAX_REQUEST_BYTES is only refused by the transport, after the credential has already been resolved"
- file: crates/llm-responses/src/client.rs
  line: 81
  category: acceptance
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: "the client never runs TurnRequest::validate_for against the binding's capabilities and sends requests the binding does not support, unlike the Messages and Chat clients"
- file: crates/llm-responses/src/client.rs
  line: 101
  category: acceptance
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: "the client never runs TurnOutcome::validate_for, so a model answering a forced file_read with a shell_exec call is returned as Ok"
- file: crates/llm-responses/src/client.rs
  line: 135
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "removing the stop-at-final-event break leaves the unit's tests/client.rs green because no fixture keeps the connection open"
```
# responses-client adversary pass 1: coordinator decisions

| # | finding | decision |
|---|---|---|
| F1 | observation attached to NotSent refusals | accept, fix: never attach to a NotSent error |
| F2 | body size checked only after the credential | accept, fix: check MAX_REQUEST_BYTES before resolving |
| F3 | no TurnRequest::validate_for | accept, fix: validate against the binding before anything, as Messages does |
| F4 | no TurnOutcome::validate_for | accept, fix: validate the outcome, as Messages does |
| F5 | stop-at-final-event unpinned | accept; the adversary case pins it; no code change |
