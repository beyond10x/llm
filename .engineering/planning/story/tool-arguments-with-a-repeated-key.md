---
format: aep.planning-md/3
id: story:tool-arguments-with-a-repeated-key
kind: story
status: active
title: Tool-call arguments that repeat a key do not reach a caller with one reading chosen
tags:
- from-finding
relations:
- informed_by: review-result:adversary-w27-rubric-judge-pass-1
- decomposes: epic:inference
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: AGENTS.md
- confidence: inferred
  path: CHANGELOG.md
- confidence: inferred
  path: checks/conformance/src/decisions.rs
- confidence: inferred
  path: checks/conformance/src/messages.rs
- confidence: inferred
  path: contracts/baseline.json
- confidence: inferred
  path: contracts/bedrock/scenarios
- confidence: inferred
  path: contracts/chat/scenarios
- confidence: inferred
  path: contracts/decisions/scenarios
- confidence: inferred
  path: contracts/ess-inputs.yaml
- confidence: inferred
  path: contracts/messages/scenarios
- confidence: inferred
  path: contracts/responses/scenarios
- confidence: inferred
  path: contracts/suite.json
- confidence: cited
  path: crates/llm-bedrock/src/converse.rs
- confidence: cited
  path: crates/llm-bedrock/src/invoke.rs
- confidence: inferred
  path: crates/llm-bedrock/tests
- confidence: cited
  path: crates/llm-chat/src/incoming.rs
- confidence: inferred
  path: crates/llm-chat/tests
- confidence: inferred
  path: crates/llm-core/Cargo.toml
- confidence: inferred
  path: crates/llm-core/src/error.rs
- confidence: cited
  path: crates/llm-core/src/item.rs
- confidence: inferred
  path: crates/llm-core/src/lib.rs
- confidence: inferred
  path: crates/llm-core/src/turn.rs
- confidence: inferred
  path: crates/llm-core/tests
- confidence: cited
  path: crates/llm-decide/src/answer.rs
- confidence: inferred
  path: crates/llm-decide/src/classify.rs
- confidence: inferred
  path: crates/llm-decide/tests/classify/whole_answer.rs
- confidence: cited
  path: crates/llm-messages/src/codec.rs
- confidence: cited
  path: crates/llm-messages/src/decode.rs
- confidence: inferred
  path: crates/llm-messages/src/lib.rs
- confidence: inferred
  path: crates/llm-messages/tests
- confidence: cited
  path: crates/llm-openai-decisions/src/strict.rs
- confidence: cited
  path: crates/llm-responses/src/stream.rs
- confidence: inferred
  path: crates/llm-responses/tests
- confidence: inferred
  path: crates/llm-tool-call/tests
- confidence: inferred
  path: docs/bedrock.md
- confidence: inferred
  path: docs/chat.md
- confidence: inferred
  path: docs/design.md
- confidence: inferred
  path: docs/messages.md
- confidence: inferred
  path: docs/responses.md
- confidence: inferred
  path: spec/domains/decisions.yaml
- confidence: inferred
  path: website/docs/guides/call-a-model-with-one-forced-tool.md
revision: 46
transitions:
- {from: "draft", to: "proposed", at: "2026-11-25T14:06:40Z", actor: "human:operator", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-25T14:06:41Z", actor: "human:operator", revision: 3}
---
## Context

`story:classifier-takes-only-a-whole-answer` makes the classifier abstain on an answer that gives a
key twice, on its JSON-schema path, where the answer is text and `llm-decide` reads its bytes.

On the forced-tool path the answer is the arguments of a tool call. `ToolCall.arguments` is an
already-parsed `serde_json::Value` (`crates/llm-core/src/item.rs`), and the three protocol decoders
keep the last value of a repeated key when they parse it (`crates/llm-messages/src/decode.rs`,
`crates/llm-responses/src/stream.rs`, `crates/llm-chat/src/incoming.rs`; read by the `story-scoper`
of 2026-11-25 at `87bc16f16`). So a model that answers `{"label":"a","label":"b"}` as tool arguments
is read as `b`, and no caller of the library can see that the key came twice. Whether `llm-bedrock`
parses tool arguments the same way is not established.

The defect is in the protocol crates and was there before wave 2026-11-25-classifier-whole-answer.

## Outcome

Tool-call arguments that give a key twice, at any depth, do not reach a caller as a call with one
reading chosen for it. A classifier on the forced-tool path abstains `OffLabel` on such an answer.

## Open

Decided on 2026-11-25. The question was what a protocol decoder does with such arguments: refuse
the turn as a protocol error, carry the call marked so that the caller decides, or a control that
chooses between the two. The operator session that acts with the owner's permissions answered:
"D35 = A: a repeated key in tool-call arguments is a protocol error, like arguments that are not
JSON. Name the break in the changelog of the release that carries it."

What follows from it:

- A decoder refuses the turn as a protocol error, as it does for arguments that are not JSON. No
  type of `llm-core` gains a field or a variant for it.
- It changes every caller of tools on every wire: an `Err` where the answer was `Ok`. The
  CHANGELOG line of the release that carries it is marked breaking.
- The classifier on the forced-tool path abstains `OffLabel` on this one error, as the Outcome
  says, and not `ProviderError`; how it tells this error from another protocol error is the
  unit's to settle with the Scope section's reading of `Error`.
- Not chosen: a new `Item` variant that keeps the raw text, and a `Controls` flag.

## Acceptance

- For Messages, Responses, Chat Completions and each Bedrock path that carries a tool call, a
  streamed and a whole answer whose tool arguments repeat a key has a test that fails before the
  change.
- `llm-decide` has a forced-tool test for the repeated `label` key that abstains `OffLabel`.
- The strict reader is one function; `crates/llm-decide/src/answer.rs` and
  `crates/llm-openai-decisions/src/strict.rs` do not each keep a copy.

## Verification

Targeted: the named test targets of the protocol crates and of `llm-decide`, and the conformance
check. No live call.

## Scope

Derived 2026-11-25 by `story-scoper`, read at `main` 831c21d95. Every line is **cited** (read from
the story or the tree; the source is named in brackets) or **inferred** (a reading that could be
wrong).

- **Primary surface:** the answer decoders of `llm-messages`, `llm-responses`, `llm-chat` and `llm-bedrock`, at the lines where tool arguments become `ToolCall.arguments` (`serde_json::Value`, `crates/llm-core/src/item.rs:11`) — cited (story, tree)
- **Primary surface, second:** one strict JSON reader in `llm-core`, called by those decoders and by the two decision crates — inferred
- **Where arguments become a value, per wire.** Every site below reads into `Value` with `serde_json::from_str` or `from_slice`; a repeated key keeps its last value and nothing is reported (measured for this reader by `review-result:adversary-w27-rubric-judge-pass-1`, finding 1).
- Messages, streamed: `input_json_delta.partial_json` fragments are kept as raw text (`crates/llm-messages/src/decode.rs:809-817`) and parsed once in `stop_block` at `decode.rs:859`. Last value wins — cited (story, tree)
- Messages, streamed, a `tool_use` block that opens with a non-empty `input` and gets no delta: the object was parsed with the whole SSE payload at `crates/llm-http/src/sse.rs:130`; `stop_block` keeps it (`decode.rs:858`) and `decode_block` clones it (`crates/llm-messages/src/codec.rs:981-1004`). Last value wins, and the text is gone before this crate runs — cited (tree)
- Messages, whole: `decode_message(message: &Value, …)` (`decode.rs:242`) receives a parsed value and `decode_block` clones `input` (`decode.rs:268`, `codec.rs:981-1004`). The repeated key is gone before the crate is called. The Messages client is stream-only (`crates/llm-messages/src/client.rs:334`); the callers that hold bytes are InvokeModel and the conformance driver — cited (tree)
- Responses, streamed and terminal: `function_call` (`crates/llm-responses/src/stream.rs:840`) takes `arguments` as a JSON string and parses it at `:855`. It is reached from `response.output_item.done` (`:388`) and from the terminal object's `output` (`:668`). Last value wins. There is no non-streamed entry: `decode_stream` (`:94`) is the one decoder and the client is stream-only (`crates/llm-responses/src/client.rs:263`) — cited (story, tree)
- Chat Completions, streamed: fragments accumulate as text (`crates/llm-chat/src/incoming.rs:258`) and `parse_arguments` (`:727-732`) reads them in `finish` (`:351`). Whole: `decode_completion` (`:393`) takes `function.arguments` as a string and calls the same function (`:442`). Last value wins on both — cited (story, tree)
- Bedrock Converse, streamed: `toolUse.input` fragments accumulate as text (`crates/llm-bedrock/src/converse.rs:805-811`); `tool_input` (`:1299-1310`) parses them at `:1303` when the block closes (`:1211`). Last value wins — cited (tree)
- Bedrock Converse, whole: `read_answer` parses the body at `converse.rs:1373`; `whole_block` clones `toolUse.input` (`:1442-1454`) and shows it re-serialised as the one `ToolArgumentsDelta` (`:1463`). Last value wins, in the delta too — cited (tree)
- Bedrock InvokeModel, streamed: `chunk` parses each event at `crates/llm-bedrock/src/invoke.rs:355` and hands it to `llm_messages::StreamDecoder` (`:147`), so the arguments are read at `decode.rs:859` — cited (tree)
- Bedrock InvokeModel, whole: `decode_response` parses the body at `invoke.rs:378` and calls `llm_messages::decode_message` (`:381`). The bytes exist only in this function — cited (tree)
- **What a caller gets today:** `Ok(TurnOutcome)` with `Item::ToolCall(ToolCall { arguments })` holding the last value: no error, no warning, no mark. On every streamed path the sink has already been shown the raw text with both keys as `ToolArgumentsDelta`. `call_tool` returns the value (`crates/llm-tool-call/src/lib.rs:341`) and `answer::call` hands it to the classifier (`crates/llm-decide/src/answer.rs:62-76`, `crates/llm-decide/src/classify.rs:783`) — cited (tree)
- **What the Outcome asks:** no call with one reading chosen, and `OffLabel` on the forced-tool path; which of the two forms below is `## Open` — cited (story)
- **Form "refuse the turn":** `Error::protocol` in `crates/llm-core/src/error.rs` carries it with no type change. Every caller of tools gets `Err` where it got `Ok`: not a compile break, a behaviour change this CHANGELOG marks `**Breaking:**` (`CHANGELOG.md:11`, `:31`) — cited (tree)
- That form does not give `OffLabel` as the code stands: a failed turn abstains `ProviderError(error.code)` with the record written failed (`classify.rs:846`, `:861`; `docs/design.md:1001-1004`; `crates/llm-decide/tests/classify/whole_answer.rs:425`) — cited (tree)
- For `OffLabel` the classifier would have to tell this refusal from every other `protocol` error. `Error` (`error.rs:64`) has public fields and `deny_unknown_fields` and is not `non_exhaustive`, so a new field breaks struct literals and older readers; `RefusalCause` is `non_exhaustive` but is documented as the far side's cause — inferred
- **Form "carry the call marked", a field on `ToolCall`:** `ToolCall` (`item.rs:6-12`) has three public fields, `deny_unknown_fields`, no `non_exhaustive`. A new field is a compile break for every struct literal: 13 sites in 11 `src` files, 63 sites in 48 test files, and the guide programs `crates/llm-docs/examples/forced_tool_call.rs:35` and `interrupt_turn.rs:272` with their pages (`git grep -c 'ToolCall {'`), plus every consumer. A newer serialised turn is refused by an older reader — cited (tree)
- **Form "carry the call marked", a new `Item` variant kept as raw text:** `Item` is `non_exhaustive` (`item.rs:61`), so a caller outside the workspace compiles and its wildcard arm refuses the item (`item.rs:16-18`). Inside the workspace `kind` (`item.rs:117-127`), `bind_unattributed` (`:168-174`) and `validate_for` (`crates/llm-core/src/turn.rs:404-445`) each need an arm, and so does every encoder that matches `Item`. `validate_for` refuses a `ToolCalls` stop with no `ToolCall` (`turn.rs:454`). The last new variant was marked `**Breaking:**` (`CHANGELOG.md:231`) — cited (tree)
- **A `StreamEvent::Warning` alone** needs no type change (`crates/llm-core/src/port.rs:37`, `code: String`) but leaves the chosen reading in the outcome, so it does not meet the Outcome — inferred
- **A `Controls` flag beside `return_unpublished_calls`** (`crates/llm-core/src/controls.rs:403`) would let a caller opt in to the carried call while the default refuses; that field's addition was marked `**Breaking:**` (`CHANGELOG.md:288`) — inferred
- **Also likely, only if the call is carried marked:** the encoders and relays that read `call.arguments`: `crates/llm-messages/src/codec.rs:59-62`, `crates/llm-messages/src/ingress.rs:537-539`, `crates/llm-responses/src/request.rs:763`, `crates/llm-responses/src/ingress.rs:575-580`, `:958-961`, `crates/llm-chat/src/outgoing.rs:213`, `crates/llm-chat/src/ingress.rs:846-861`, `crates/llm-bedrock/src/converse.rs:232-238`, `crates/llm-tool-call/src/lib.rs:336-342`. Messages and Converse send `input` as an object and have no place for raw text — inferred
- **Strict reader, copy 1:** `crates/llm-decide/src/answer.rs`. `mod answer` is private (`crates/llm-decide/src/lib.rs:28`). `pub(crate) fn whole` (`:32`) is called by `classify.rs:785` and `judge.rs:312`; `pub(crate) fn call` (`:62`) by `classify.rs:783`. Private: `Strict` (`:84-158`), `MAX_DEPTH = 128` (`:82`), `integers_are_exact` (`:162`). It reads with the JSON reader's depth limit switched off (`:46`) — cited (story, tree)
- **Strict reader, copy 2:** `crates/llm-openai-decisions/src/strict.rs`. `pub const MAX_DEPTH: usize = 16` (`:14`) is public API (`crates/llm-openai-decisions/src/lib.rs:44`, asserted at `crates/llm-openai-decisions/tests/client.rs:737`). `pub(crate) fn document` (`:20`) is called by `request.rs:358` and `response.rs:117`. `Strict` (`:31-99`) is the same visitor with other error text — cited (story, tree)
- **The copies differ in:** the bound (128 and 16), the reader's own depth limit (off and on), and the integer check, which only `llm-decide` has — cited (tree)
- **Where one function can live:** `llm-core` has no workspace dependency and already depends on `serde` and `serde_json` (`crates/llm-core/Cargo.toml:17-22`). `llm-messages`, `llm-responses`, `llm-chat`, `llm-bedrock`, `llm-http`, `llm-decide` and `llm-openai-decisions` all depend on it, so there is no cycle. It is the only crate both decision crates and all four wire crates already depend on: `llm-decide` does not depend on `llm-http` (`crates/llm-decide/Cargo.toml:17-23`) — cited (tree)
- For the two copies alone no new edge is needed: `llm-openai-decisions` already depends on `llm-decide` (`crates/llm-openai-decisions/Cargo.toml`), which could export the reader. The decoders cannot use it from there — cited (tree)
- `disable_recursion_limit` needs `serde_json` feature `unbounded_depth`, which only `llm-decide` declares. `float_roundtrip` is declared by `llm-decide`, `llm-openai-decisions`, `llm-messages` and `llm-calls`, not by `llm-core`, `llm-responses`, `llm-chat` or `llm-bedrock`. A reader in `llm-core` either gets both features in `crates/llm-core/Cargo.toml` or takes the caller's deserializer and bound — inferred
- **ESS, commands that read a tool-call answer today:** `llm.messages.DecodeStream` and `DecodeMessage` (`spec/domains/messages.yaml:247`, `:258`), `llm.responses.DecodeStream` (`spec/domains/responses.yaml:338`), `llm.chat.ProjectResponse` (`spec/domains/chat.yaml:164`), `llm.bedrock.RunTurn` (`spec/domains/bedrock.yaml:297`, `api` `Converse` or `AnthropicInvoke`), `llm.decisions.Classify` (`spec/domains/decisions.yaml:448`), `llm.inference.InspectResult` (`spec/domains/inference.yaml:139`) — cited (tree)
- **ESS, tool-call scenarios today (by file name; three opened):**
- `contracts/messages/scenarios/`: `stream-announces-a-tool-call-by-name-before-its-arguments.yaml`, `stream-preserves-text-thinking-tools-and-terminal-usage.yaml`, `a-streamed-answer-with-reasoning-text-and-two-calls-reads-back-whole.yaml` — cited (tree)
- `contracts/responses/scenarios/`: `a-tool-call-stream-names-the-call-its-arguments-belong-to.yaml`, `a-finished-call-renaming-its-announcement-is-refused-with-its-counters.yaml`, `the-text-and-tool-subset-round-trips.yaml` — cited (tree)
- `contracts/chat/scenarios/`: `incoming-tool-arguments-survive-the-split-that-delivered-them.yaml`, `incoming-a-streamed-call-is-announced-by-name-before-its-arguments.yaml`, and the two `incoming-*-tool-stream-preserves-its-call-and-counters.yaml` — cited (tree)
- `contracts/bedrock/scenarios/`: `turn-a-converse-stream-is-signed-replayed-and-carries-text-and-a-tool-call.yaml` — cited (tree)
- `contracts/decisions/scenarios/`: `a-forced-tool-classifier-returns-one-label-with-a-confidence-per-label.yaml`, `two-tool-calls-abstain-off-label.yaml`, `a-tool-call-of-a-turn-cut-at-the-output-limit-abstains-off-label.yaml`, and the JSON-schema sibling `an-answer-that-repeats-label-abstains-off-label.yaml` — cited (tree)
- `contracts/inference/scenarios/`: `model-output-calling-an-unpublished-tool-*.yaml`, `a-tool-calls-stop-without-a-call-is-refused.yaml` — cited (tree)
- **ESS, where the new scenarios go:** one streamed and one whole per wire in `contracts/messages/scenarios/`, `contracts/responses/scenarios/`, `contracts/chat/scenarios/` and `contracts/bedrock/scenarios/` (Converse and `AnthropicInvoke`), and one forced-tool scenario in `contracts/decisions/scenarios/`. Each is listed in `contracts/ess-inputs.yaml`; `contracts/suite.json` is regenerated; the floor in `contracts/baseline.json` (1090) and in `AGENTS.md:80` rises by their number — inferred
- **ESS, what cannot be authored as the drivers stand:**
- A whole Messages answer: the driver parses `message_json` before `decode_message` (`checks/conformance/src/messages.rs:281`). It needs a bytes entry in `llm-messages` and in the driver, or the case is authored as `llm.bedrock.RunTurn` with `AnthropicInvoke`, `stream: false` — cited (tree)
- A forced-tool classification: the scripted model parses the authored arguments itself (`checks/conformance/src/decisions.rs:343-355`) and builds the `ToolCall` (`:429-433`); the program comment says so (`spec/domains/decisions.yaml:453-456`) — cited (tree)
- A whole Bedrock answer needs a new `.json` under `crates/llm-bedrock/tests/data/frames/`, served as raw bytes (`checks/conformance/src/bedrock.rs:707-715`). Converse stream frames (`frames_json`), Responses `stream_json` and Chat `response` carry the arguments as strings and need no driver change — cited (tree)
- A view field is needed only if the call is carried marked (`spec/domains/*.yaml` entities, then `contracts/schema/`); a refusal shows in the existing `accepted` and `error_code` — inferred
- **Documents that state today's behaviour and would change:**
- `docs/design.md:993-1004` ("a key the model gave twice in them is not seen here; that reading belongs to the protocol decoders") — cited (tree)
- `spec/domains/decisions.yaml:24-30` (comment: "A key given twice in a call's arguments is not seen") — cited (tree)
- `crates/llm-decide/src/answer.rs:52-61` and `classify.rs:344`, `:711-714` (doc comments) — cited (tree)
- `docs/chat.md:90-95` (the refusal list), `docs/messages.md:250-251` ("parsed once when the block closes"), `docs/responses.md` (the stream table at `:309` and its refusals), `docs/bedrock.md` (no sentence on tool input found) — inferred
- `website/docs/guides/call-a-model-with-one-forced-tool.md:127-136` (the results table of `call_tool`), `docs/implementation-status.md:158` with the generated `website/data/status.json` only if the capability line changes — inferred
- `AGENTS.md`: the floor row (`:80`) and a new row in the invariants table beside the two tool-call rows (`:71-72`) — inferred
- `CHANGELOG.md` under Unreleased (`:7`): a `**Breaking:**` line and an `ESS:` line with the new floor — inferred
- `docs/contract-v1.md:56-68` and `website/docs/concepts/neutral-boundary.md:34-40` describe tool calls and state nothing about a repeated key — cited (tree)
- **Confidence:** medium — every parse site above was read at its line, but how much of `llm-core`, the encoders and `llm-tool-call` changes depends on `## Open`, and two parts of the story cannot be placed as written (a whole Messages answer, and `OffLabel` under a refusal)
- **Would collide with:** any unit that edits `crates/llm-decide/src/answer.rs` or `crates/llm-openai-decisions/src/strict.rs`; any unit that edits `crates/llm-core/src/item.rs`, `turn.rs` or `error.rs`; any unit that edits the answer decoders `crates/llm-messages/src/decode.rs`, `codec.rs`, `crates/llm-responses/src/stream.rs`, `crates/llm-chat/src/incoming.rs`, `crates/llm-bedrock/src/converse.rs`, `invoke.rs`; any unit that adds, edits or moves a test under `tests/` of those crates or of `llm-decide`; any unit that adds a scenario (`contracts/ess-inputs.yaml`, `contracts/suite.json`, `contracts/baseline.json`, `AGENTS.md:80`); any unit that edits the invariants table of `AGENTS.md`, `CHANGELOG.md` Unreleased or `docs/design.md` — inferred
- **Overlap with `story:llm-decide-tests-read-no-clock-and-build-one-binary`:**
- `crates/llm-decide/src/answer.rs`: this story takes `Strict` (`:84-158`) out of the file or replaces it with a call; that story's scope puts a `#[cfg(test)]` counter into `Strict` and `integers_are_exact` there and moves two cases into the unit-test module at `:198`. Either order is a textual conflict — cited (tree, that story's Scope)
- If `Strict` moves to `llm-core` first, a `#[cfg(test)]` counter there is compiled only into `llm-core`'s own unit tests, so that story's count of reader steps cannot be read from `llm-decide` — inferred
- `crates/llm-decide/tests/classify/whole_answer.rs` (this story adds the forced-tool case; that story moves the file and rewrites its imports) and `crates/llm-decide/tests/classify.rs` if the fake needs a new reply — inferred
- **Overlap with `story:test-binaries-one-per-crate`:**
- Every test this story adds under `crates/llm-messages/tests`, `crates/llm-responses/tests`, `crates/llm-chat/tests`, `crates/llm-bedrock/tests`, `crates/llm-core/tests`, `crates/llm-decide/tests`, `crates/llm-tool-call/tests`: a new `tests/<file>.rs` merges cleanly and is a second binary again; a case added to an existing file conflicts with its move. Its parts 1 to 5 each meet one part of this story — cited (tree, that story's Scope)
- `AGENTS.md` invariants table, `CHANGELOG.md`, `docs/design.md`, `docs/messages.md`, `docs/responses.md`, `docs/bedrock.md`, `website/docs/guides/call-a-model-with-one-forced-tool.md` (its `source:` line 6, this story's table), `spec/domains/messages.yaml` (its comment at `:246`, this story's `DecodeMessage` at `:258` if the command changes): its part 6 — inferred
- **Safety fact:** on five of the nine paths the decoder still holds the arguments as text at the line that parses them (`decode.rs:859`, `stream.rs:855` for both Responses paths, `incoming.rs:731` for both Chat paths, `converse.rs:1303`), so a strict read at that line returns the same `Value` for text that repeats no key and changes no other answer. The limits of that: the shared reader must not apply `llm-decide`'s integer check or a bound under the JSON reader's own to tool arguments, or it refuses arguments that are read today. Step 2 for the two copies (`answer.rs:84-158`, `strict.rs:31-99`), step 1 for `serde_json`'s own reader, unproven — inferred
- **Safety fact, second:** three functions on these paths have other callers. `parse_arguments` is also the Chat ingress relay check (`crates/llm-chat/src/ingress.rs:987`). `decode_block` is also the ingress request reader and three relay checks (`codec.rs:687`, `crates/llm-messages/src/ingress.rs:405`, `:471`, `:693`). `decode_message` is also InvokeModel's whole path (`invoke.rs:381`). A gateway built on the ingress encoders has relayed the raw fragments to its client before a block closes, as it has today for arguments that are not JSON (`decode.rs:860`). Step 2, unproven — cited (tree)
- **Proposed split, seven parts with disjoint paths.** Part 1 is first and settles `## Open`; parts 2 to 4 are disjoint from each other; part 5 follows part 2; parts 6 and 7 are last.
- Part 1: `crates/llm-core/src/` (the one reader, and the carrier if the call is carried marked), `crates/llm-core/Cargo.toml`, `crates/llm-core/tests/`, `crates/llm-decide/src/answer.rs`, `crates/llm-decide/src/classify.rs`, `crates/llm-decide/tests/classify/whole_answer.rs`, `crates/llm-openai-decisions/src/strict.rs`. It closes the second and third acceptance items; the forced-tool case runs on the fake model and needs no wire — inferred
- Part 2: `crates/llm-messages/src/decode.rs`, `codec.rs`, `lib.rs` (a bytes entry beside `decode_message`), `crates/llm-messages/tests/` — inferred
- Part 3: `crates/llm-responses/src/stream.rs`, `crates/llm-responses/tests/`, `crates/llm-tool-call/tests/` — inferred
- Part 4: `crates/llm-chat/src/incoming.rs`, `crates/llm-chat/tests/` — inferred
- Part 5: `crates/llm-bedrock/src/converse.rs`, `invoke.rs`, `crates/llm-bedrock/tests/` with new recordings under `tests/data/frames/`. After part 2, because both InvokeModel paths read through `llm-messages` — inferred
- Part 6: every new scenario, `contracts/ess-inputs.yaml`, `contracts/suite.json`, `contracts/baseline.json`, `checks/conformance/src/messages.rs`, `checks/conformance/src/decisions.rs`, `spec/domains/decisions.yaml`. One part, because the conformance check refuses an unlisted scenario (`AGENTS.md:200-201`) and the suite, the floor and the list are one file each — inferred
- Part 7: `docs/`, `website/docs/`, `AGENTS.md`, `CHANGELOG.md` — inferred
- With parts 2 to 5 carrying no scenario, "scenarios move with the code" holds at the wave's merge into `main`, not per part. Read per part, each of parts 2 to 5 carries its scenarios, regenerates `contracts/suite.json` and raises the floor, and they merge one after another — inferred
