---
format: aep.planning-md/3
id: review-result:adversary-w31-classifier-whole-answer-pass-1
kind: review-result
status: active
title: 'Adversary, pass 1 (story:classifier-takes-only-a-whole-answer, wave 2026-11-25-classifier-whole-answer): green, 3 notes, 0 red cases'
relations:
- reviews: story:classifier-takes-only-a-whole-answer
revision: 1
---
Recorded by the coordinator as the adversary returned it. Covers head `6cdaf1655` of
`impl/classifier-whole-answer` (base `0fa28f936`).

unit: story:classifier-takes-only-a-whole-answer
verdict: green
cases: executed 98→103, red 0
origin: introduced 3, pre-existing 0, undecided 0
wrote-outside-worktree: the scratch directory `adv/` only (logs, `mut/` mutant copies and three throwaway scripts, `whole_answer-with-probe-cases.rs`)
needs-coordinator: no

`git --no-pager diff --stat` in the unit worktree: empty. `git status --short` is empty in both
worktrees; the mutants worktree is at the head and its `target/` is removed. No permission check
refused anything. The conformance check was not run (no scenario added).

The adversary could not make the unit red. Five probe cases (`adversary_w31_*`, in
`crates/llm-decide/tests/classify/whole_answer.rs`) passed on their first run
(`adv/01-cases-alone.log`). They killed no mutant the unit's own tests miss, so the adversary
withdrew them; their text is in `adv/whole_answer-with-probe-cases.rs`. The 103 is the
`-p llm-decide` run with them present (`adv/05-suite-llm-decide.log`); the tree as left runs 98. Two
tests failed in that run, both the known wall-clock ones:
`judge::an_answer_of_any_make_is_read_in_time_proportional_to_its_length` and
`adversary_w27_judge_pass2::a_long_answer_of_numbers_is_read_in_time_proportional_to_its_length`.

Findings:

1. One mutant survives: `call` taking a call from a turn that says `EndTurn`.
   `crates/llm-decide/src/answer.rs:62`. With the check loosened to accept `EndTurn` as well, the
   lib test and all of `--test classify` stay green (`adv/mut-m09-call-endturn.log`). What reaches
   it: nothing found. The routed call refuses `EndTurn` with a call
   (`crates/llm-core/src/turn.rs:455`) before `call` reads it. The name check has the same standing
   and is held by the unit test at `answer.rs:210`; the stop reason has none. The guard is one
   assertion in that test; measured in the mutants worktree: green on the head, red on the mutant
   (`adv/04-guard-m09.log`).
2. Claim 8 stays unproven, but two decoders can emit the pair.
   `crates/llm-decide/src/answer.rs:38`. A whole answer beside an empty text item abstains now and
   decided at the base; the unit's own test at `whole_answer.rs:242` pins this. By reading: Messages
   emits one item per text block, an empty block included (`crates/llm-messages/src/codec.rs:964`);
   Responses emits one item per output message, its parts joined, and a message with no parts is an
   empty item (`crates/llm-responses/src/stream.rs:949`); Bedrock Converse emits one item per
   non-empty text block (`crates/llm-bedrock/src/converse.rs:1146`, `:1371`); Chat emits at most one
   text, an empty one dropped (`crates/llm-chat/src/incoming.rs:326`). What reaches it: nothing
   found. No fixture in the repository records such a pair for one answer, and no live call was
   allowed. A preamble text beside the answer abstained at the base too, since the join was not
   JSON. The new abstentions are only an empty or whitespace-only neighbour and one JSON text split
   over items.
3. A malformed `tool_calls` entry is not refused as a program.
   `checks/conformance/src/decisions.rs:415`. Read, not run: an entry that is not JSON leaves
   `valid_program` true and fails inside the scripted turn, so a scenario typo reads as a provider
   error of the library. `tool_arguments` did this before; `tool_calls` inherits it. `answers` and
   `stop_reason` are typed and refused at the program parse. What reaches it: a scenario author
   only.

Attacked and not broken:

- Base build (`adv/mut-m00-base.log`): 12 of the unit's 19 new tests fail at the base, among them
  each acceptance case (repeated `label`, cut at the output limit, two text items) and the replay
  row. The 34 older classifier tests are unchanged in the diff.
- Mutants killed by the unit's own tests (`adv/mut-*.log`): the stop check removed in `call`, and in
  `whole`; the old joining reader in the classifier only (11 tests red); first call wins, and last
  call wins; the refusal arm of `call` dropped; the name check of `call` dropped (killed only by the
  unit test in `answer.rs`); `Item::Opaque` refused in `call`, and assistant text refused in `call`;
  `call` refusing only the output limit; `whole` passing over an empty text, keeping the last of a
  repeated key, and passing over a refusal.
- Probe cases, all green: every stop reason on both paths with a decodable answer, including the
  schema path with `ToolCalls` (refused by the call, record failed) and nine `Incomplete` reasons; a
  forced-tool turn without a call under each stop reason; a refusal with no text on both paths;
  spellings that still decide (escaped single key, member order, whitespace, 0 and 10000);
  spellings that still abstain (byte order mark, form feed, exponent, fraction, `-0`, an integer no
  `u64` holds, a third member); three calls, and two calls with text and state between them.
- Claim 4: every attempt's outcome passes `validate_for` in
  `crates/llm-routing/src/fallback.rs:500`, and a refused one halts without fallback. The chain,
  `Routed::label` and the conformance adapter all go through `record_turn`. No outcome with a call
  to another tool was found that reaches `answer::call`.
- Claim 2: reasoning arrives as `Item::Opaque` on Messages, Responses and Bedrock, and as no item on
  Chat. Only Chat emits `AssistantRefusal`, never an empty one.
- Replay: the recorded answer is a label and a candidate `None` is `differs`
  (`crates/llm-calls/src/replay.rs:1042`). The judge path is untouched: the diff of `answer.rs` only
  adds `call`.
- Documents: `docs/design.md`, the prose of `spec/domains/decisions.yaml`, the doc comments, the
  CHANGELOG lines and the floor row agree with the code. `docs/implementation-status.md:159`, the
  crate documentation and `website/docs/reference/crates.md` state no old reading.
- Scenarios: read, not run. Each scripts an answer that would decode and asserts `OffLabel` with a
  succeeded record; the five hold five different cases.

```findings
- file: crates/llm-decide/src/answer.rs
  line: 62
  category: mutant
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: a mutant of call that also takes a call from a turn that says EndTurn survives the lib and classify tests; the routed call refuses that outcome first, and the guard is one assertion in the unit test of answer.rs
- file: crates/llm-decide/src/answer.rs
  line: 38
  category: boundary
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: a whole answer beside an empty text item decided at the base and abstains now; the Messages and Responses decoders can emit that pair, and no fixture or live call shows a provider sending it
- file: checks/conformance/src/decisions.rs
  line: 415
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: a tool_calls entry that is not JSON leaves valid_program true and fails inside the scripted turn, as tool_arguments did before, while answers and stop_reason are refused at the program parse
```

## What the coordinator decided

1. The guard assertion goes into the unit test of `answer.rs`. Correction round 1.
2. An empty text item beside the answer stays an abstention: the judge's reader reads it so and one
   reader serves both. The documents and the CHANGELOG name the case. Correction round 1.
3. A malformed `tool_calls` entry is refused at the program parse, with a test. Correction round 1.

No second pass: pass 1 was green, and the coordinator reads the diff of correction round 1.
