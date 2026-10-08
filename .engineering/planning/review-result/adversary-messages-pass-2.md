---
format: aep.planning-md/3
id: review-result:adversary-messages-pass-2
kind: review-result
status: archived
title: 'Adversary, messages projection, pass 2: the turn deadline stops at the HTTP exchange, and nine of twelve mutations survived'
relations:
- reviews: story:messages-projection
revision: 2
transitions:
- {from: "active", to: "archived", at: "2026-10-08T10:43:35Z", actor: "human:timo", revision: 2}
---
## The pass

Second and final adversarial pass over `story:messages-projection`, wave 1, against `wave1-messages`
over base `dbc45f8`. The attack budget is now spent.

Verdict: **needs-change**. Eleven cases added, three red. Seven findings, all `origin: introduced`;
one blocker, four warnings, two notes.

The returned header line said `introduced 4`, which disagrees with its own findings block of seven.
The block is authoritative and is recorded verbatim below; the count in the header is wrong.

Executed cases 52 → 63. The before figure was measured by deselecting the three new files by name:
52 passed, 0 failed, so every one of the unit's own cases stays green and all three reds are the
adversary's. `git diff --stat` is byte-identical to the stat taken before the pass began — no tracked
file was modified — and every pre-existing test file still carries its pre-session mtime. The four
source digests recorded in `docs/verification/messages-falsification.json` still match the files on
disk.

## The blocker: the turn deadline does not reach the HTTP exchange

`crates/llm-messages/src/client.rs:61`. The turn's absolute instant reaches exactly two waits —
`bounded(...)` at `:90` for credential resolution and `with_deadline` at `:129` for the sink.
`post_sse` at `:119` and `stream.next()` at `:132` are never told it, and `crates/llm-http/src/transport.rs:96`
takes a **fresh** `Instant::now()` and runs against the `HttpClient`'s own `Limits`. So a 300 ms turn
limit on a 10 s transport leaves the turn running at 3 s.

Measured: `tests/adversary2_deadline.rs:124` and `:154`, both failing on `Elapsed(())` from the outer
bound rather than on the turn's own limit; whole-crate exit 101.

**What reaches it**: `with_turn_limit` is public, its own documentation at `:64` says "the shorter of
the two ends the turn", and the unit's own `tests/client.rs:371-375` builds exactly this configuration —
a 10 s transport with a 300 ms turn. A corollary the adversary read but did not run: even with `new`
alone, `transport.rs:96` restarts the clock, so a turn can run past its stated absolute instant by the
credential-resolution time.

The adversary named the fix and did not apply it: thread the turn's deadline through `post_sse` and
`SseStream::next`, or say plainly in all three documents that the HTTP exchange is bounded by the
transport's clock alone.

## Nine of twelve mutations survived the shipped gate

Twelve deliberate defects, each applied to a copy outside the worktree, the copy's gate run, the file
restored and the SHA-256 verified against the pre-mutation digest. Against the suite the unit shipped,
**nine survived**:

| mutation | file |
|---|---|
| bounded-name check on a terminal reason → `if false` | `decode.rs` |
| role check on ingress → `if false` | `codec.rs` |
| `blocks.len() == 1` → `!blocks.is_empty()` on the system prompt | `codec.rs` |
| unknown tool choice → `ToolChoice::Auto` | `codec.rs` |
| `append` missing field → `unwrap_or("")` | `decode.rs` |
| half an argument object → `unwrap_or_else(json!({}))` | `decode.rs` |
| non-integer usage counter → `Ok(n.as_u64())` | `usage.rs` |
| usage metadata type check → `if false` | `usage.rs` |
| `MAX_ITEMS` in-flight bound → `if false` | `decode.rs` |

Three of those delete refusals `docs/messages.md:38-40` names by name: a `none` tool choice, a system
prompt that is not one text block, and a role outside user/assistant. Every one of the twelve left the
conformance lane at `39 passed, 0 failed` — no guard in the list is reachable by a scenario.

The adversary's seven new cases in `tests/adversary2_documented_refusals.rs`, each written from a
sentence in `docs/messages.md`, raise that to **ten of twelve killed**. The two survivors are findings
6 and 7. One of the seven initially passed for a reason it did not name — a truncated stream refuses
whatever else is wrong with it — which the adversary found by the mutant surviving anyway, and rewrote
the case to carry a terminal event.

`docs/verification/messages-falsification.json` records 27 mutations, every one killable by a scenario.
That is the wave's recurring class in its exact form: a record listing kills says nothing about the
guards no mutation was aimed at.

## The wire-name check cannot reach two names it claims to cover

`crates/llm-messages/tests/wire_names.rs:154`. `is_wire_shaped` admits only `[a-z0-9_]`. It selected 79
literals and classified 79, unclassified 0. Widening the scan by one character class finds 81 spoken on
the wire; the two extra are `anthropic-version` and `2023-06-01`, sent by `src/client.rs:23` and `:25`
on **every** request, and in neither the confirmed nor the carried list. `docs/messages.md:99` and
`docs/verification/messages.md:132` both claim the check fails on any name in neither list.

The pass's own guard case — that the names under test are really spoken by the projection — passes, so
these are literals the source sends rather than invented strings.

## Two documents describe a tree that no longer exists

`docs/verification/messages.md:49-53` states three things that are false of the tree it documents: that
`new` does not derive the instant (it does, `client.rs:56`), that `HttpClient` has no accessor (it does,
`transport.rs:59`), and that the two adversarial cases still fail (52 passed, 0 failed). Line `:17` says
"two of the fifty fail" where `:15` says 52.

`docs/messages.md:125-130` tells a caller that a client built with `new` alone bounds the HTTP exchange
only, because `HttpClient` does not expose its limits, and to state the bound until that lands. The
accessor has landed and the behaviour is now the inverse of the sentence.

## What could not be broken

Every pass-1 finding routed back is closed, re-established from the artefacts rather than from the
unit's account: ingress refuses thinking it cannot attribute (`codec.rs:400`) and the adapter renders
the binding (`checks/conformance/src/messages.rs:299`); credential resolution is bounded
(`client.rs:90`); the sink is bounded (`decode.rs:596`); items assemble by index into a `BTreeMap`
(`decode.rs:209`); `optional()` reads an explicit null as absent (`lib.rs:39`); `start_message` runs the
full `assistant_message` check (`decode.rs:301`).

All four restore digests in the falsification record match the files on disk. All 30 scenario files on
disk are selected — checked file by file against the synthesized suite's ids, `NOT SELECTED: []` — and
39 of 39 pass three times with identical counts. Removing the derived deadline from `client.rs:56` is
caught, by two cases. `normalized()` still yields `None` whenever any component is absent, and a
non-integer counter is refused rather than downgraded to unknown.

## The pass's own error, recorded

The adversary pointed its mutation probe at the assigned `CARGO_TARGET_DIR`, which the worktree shares.
A mutated build leaked back and made two of the unit's deadline tests fail in isolation. The worktree's
sources were never touched, the digests were verified, and `cargo clean -p` over the six llm packages
restored the assigned directory to 52 of 52 green. Every mutation verdict above was then re-measured in
a dedicated build directory and came out identical. A probe needs its own build directory; this brief's
triple did not say so.

## For the coordinator

`contracts/ess-inputs.yaml` lists 177 scenarios across five domains and **zero** messages scenarios, so
the committed `contracts/suite.json` will not carry `llm.messages` and the `check` lane exits 0 without
mentioning it. The unit was right not to edit that file.

```findings
- file: crates/llm-messages/src/client.rs
  line: 61
  category: acceptance
  severity: blocker
  verdict: CONFIRMED
  origin: introduced
  message: "the absolute turn deadline is passed only to credential resolution and the sink, never to post_sse or SseStream::next, so with a turn limit shorter than the transport's the HTTP exchange runs on the transport's own clock and a 300 ms turn is still running at 3 s, contradicting the implementation contract and three documents that say it covers HTTP."
- file: crates/llm-messages/tests/wire_names.rs
  line: 154
  category: contract-drift
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "is_wire_shaped admits only [a-z0-9_], so the check that both documents claim fails on any literal in neither list cannot reach anthropic-version or 2023-06-01, the two producer names sent on every request and classified nowhere."
- file: docs/verification/messages.md
  line: 49
  category: contract-drift
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: "the verification record still says new does not derive the turn instant, that HttpClient has no accessor and that two adversarial cases still fail, and all three are false of the tree it documents at client.rs:56, transport.rs:59 and 52 passed 0 failed."
- file: docs/messages.md
  line: 125
  category: contract-drift
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: "the contract page says a client built with new alone bounds the HTTP exchange only and tells the caller to state the bound until the accessor lands, which is the inverse of the shipped behaviour now that the accessor has landed."
- file: docs/verification/messages-falsification.json
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "the record's 27 mutations are all killable by a scenario; 9 of 12 aimed at guards outside the scenarios' reach survived the whole 52-case and 39-scenario gate, three of them deleting refusals docs/messages.md names by name, and seven are closed by the cases this pass added."
- file: crates/llm-messages/src/decode.rs
  line: 320
  category: boundary
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "deleting the in-flight MAX_ITEMS content-block bound leaves the entire gate green because no fixture streams that many blocks, and the overrun is then caught only after the stream ends by core's late check."
- file: crates/llm-messages/src/usage.rs
  line: 70
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "deleting the service_tier and inference_geo type check leaves the entire gate green, and no document promises that refusal either way."
```
