---
format: aep.planning-md/3
id: review-result:adversary-w07-responses-ingress-encoder-pass-1
kind: review-result
status: active
title: Wave 2026-10-16-w07 adversary, story:responses-ingress-encoder, pass 1
relations:
- reviews: story:responses-ingress-encoder
revision: 1
---
unit: story:responses-ingress-encoder, tree `~/.local/state/worktree/trees/b10x/llm/llm-w07-responses-ingress-encoder` at head 6fd6b34c plus one untracked test file
verdict: red
cases: executed 166→170, red 4
origin: introduced 4, pre-existing 0, undecided 0
wrote-outside-worktree: ~/.cache/llm-wave-w07/responses-ingress-encoder/scratch/adversary-suite.log
needs-coordinator: yes

**1. What I touched.** `git --no-pager diff --stat` is empty because my file is new and untracked. `git status --short` shows one path, and it is a test file:
```
?? crates/llm-responses/tests/adversary_w07_ingress.rs
```

**2. Cases added**, in `crates/llm-responses/tests/adversary_w07_ingress.rs`. All four are red now. Each outcome is checked with `TurnOutcome::validate_for` against a request read in with `ingest_request`.

The first run of this file alone had 2 of the 4 cases fail for the wrong reason. My request body lacked the `include` field, so `ingest_request` refused it ("the client body ingests: … a field this projection fixes is absent"). That was my fixture error, not a finding. I added the field and reran the file alone; this is that run, verbatim:

| case | asserts | red output |
|---|---|---|
| `a_turn_with_several_messages_reads_back_as_the_events_that_were_encoded` | the story's streamed round trip gives back the same events | `left: [TextDelta "First.", ToolCallStarted call-1, ToolArgumentsDelta "{}", TextDelta "Second.", TextDelta "Second."]` / `right: [… TextDelta "Second."]` |
| `every_streamed_message_finishes_with_the_text_its_deltas_showed` | for each message item, the deltas add up to its `output_text.done` text | `left: {"msg_resp_gateway_1_0": "First.Second.", …}` / `right: {"msg_resp_gateway_1_0": "First.", …}` |
| `a_streamed_response_keeps_one_id_from_created_to_completed` | `response.created` and the terminal payload carry the same `id` | `left: String("resp_gateway_1")` / `right: String("resp_upstream_1")` |
| `arguments_the_decoder_refuses_are_refused_by_the_encoder_not_emitted` | the decoder accepts whatever the encoder emits, or the encoder refuses | `non-streamed [1]: the encoder answered, the decoder refused: … "function call arguments are not a JSON object"` |

**3. Suite run, after the cases existed.** Command: `RUSTUP_TOOLCHAIN=1.98.0 cargo test -p b10x-llm-responses --locked --no-fail-fast`. Exit 101. My binary: `test result: FAILED. 0 passed; 4 failed`. Every other binary: `ok`, 166 cases in total. The before count of 166 comes from this same run with my binary taken out; it is not a separate run.

**4. Findings.** `crates/llm-responses/src/ingress.rs` does not exist at base 6d889e7a (`git show` exit 128), so all four are `introduced`.

- **`ingress.rs:239` — text shown twice (blocker).**
  - What breaks: `chunk` appends every text delta to a single message item, even when a call comes between them. `close` then ends that item with the first `AssistantText` only, and sends each later `AssistantText` again as a new message. The caller sees "First.Second.Second.", and the first item's deltas disagree with its finished text.
  - Tests: the first two cases above.
  - What reaches it: any outcome with more than one text item. A Messages upstream produces one `AssistantText` per text block (`llm-messages/src/codec.rs:278,428`), so text, tool call, text is enough. A Responses upstream with two `message` output items reaches it too. Whether llm-gateway calls this encoder yet: I don't know.
- **`ingress.rs:375` — the stream changes `id` mid-stream (warning).**
  - What breaks: `response.created` and `response.in_progress` carry the gateway `id`, but the terminal object carries the outcome's `response_id`. A client that keeps the id from `response.created` holds one that the finished response does not have.
  - Test: the third case.
  - What reaches it: every streamed outcome whose upstream reported an id. The Responses decoder always reads one.
  - Needs coordinator: the round-trip rule (equal `response_id`) and a constant stream id cannot both hold.
- **`ingress.rs:450` — tool arguments the decoder refuses (warning).**
  - What breaks: `check_outcome` accepts call arguments that are valid JSON but not an object (`[1]`, `"noon"`, `null`, `5`). The encoder emits them, and this repository's decoder then refuses its own encoder's output.
  - Test: the fourth case.
  - What reaches it: `validate_for` accepts such an outcome, and Chat's `parse_arguments` (`llm-chat/src/incoming.rs:477`) produces it. It needs a model that actually emits non-object arguments; I did not observe one.
  - Fix to name: refuse non-object arguments in `check_outcome`.
- **`ingress.rs:610` — `fail` reports the wrong code for most errors (note, no test).**
  - What breaks: 9 of the 12 error codes decode back as `refused`. The story's Acceptance says the decoder turns the failure into "the error's code". The unit's test `fail_ends_the_stream_with_a_failure_the_decoder_reads_as_the_same_code` asserts `Transport`→`Refused`, which contradicts its own name.
  - What reaches it: no routing behaviour changes, because a decoded failure has `Dispatch::Accepted` and fallback needs `NotSent` or `Rejected` (`llm-routing/src/fallback.rs:231`).
  - Needs coordinator: narrow the Acceptance wording, or teach the decoder more codes.

**5. Attacked and could not break:**
- Usage: an unknown counter is omitted; `total_tokens` only appears when both parts are known; an empty report becomes `{}`; the cache-write extension mirrors Chat.
- Stop reasons: `max_output_tokens` as an incomplete reason, and reasons not shaped like a code, are refused.
- Opaque state: a foreign binding, non-reasoning state and unattributed state are all refused, not dropped.
- Caller-owned items are refused.
- `fail` never relays the error message.
- A refused `close` leaves the stream open for `fail`.
- Nothing is emitted after the stream ends.
- `sequence_number` order holds.
- An empty text delta works.
- Reasoning shown without carried state adds no item.
- A call that was never announced is emitted whole at close.
- The 12 new scenarios agree with the code. `outcome_preserved: false` for the caller's id is documented in the scenario.

Outside this unit: `tests/adversary_codex_stream.rs:525` has a `#[ignore]` at base, which breaks the repository's no-`#[ignore]` rule. It is pre-existing and not a finding against this unit.

**6. Paths written outside the worktree:** `~/.cache/llm-wave-w07/responses-ingress-encoder/scratch/adversary-suite.log`, the suite output, in the assigned scratch dir. I acquired a session lease on the tree (`adversary-w07-responses-ingress-encoder`) and released it.

**7. Findings block**
```findings
- file: crates/llm-responses/src/ingress.rs
  line: 239
  category: acceptance
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "a turn with more than one AssistantText re-sends every later message's text at close after already streaming it into the first message item, so the caller sees text twice, the first item's deltas disagree with its done text, and decoded events differ from the encoded ones"
- file: crates/llm-responses/src/ingress.rs
  line: 375
  category: contract-drift
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the streamed response opens under the caller's id and ends under the outcome's response_id, so one stream carries two response ids"
- file: crates/llm-responses/src/ingress.rs
  line: 450
  category: boundary
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "check_outcome accepts tool-call arguments that are JSON but not an object, which this crate's decoder then refuses, so the encoder answers with output its own decoder rejects instead of refusing"
- file: crates/llm-responses/src/ingress.rs
  line: 610
  category: judgement
  severity: note
  verdict: needs-revision
  origin: introduced
  message: "fail encodes 9 of 12 error codes as wire codes the decoder reads back as refused, short of the Acceptance line that the decoder turns the failure into the error's code"
```
