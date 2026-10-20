# Wave 2026-10-20-w09: design review before dispatch

Requested by the owner on 2026-10-20, after approving the wave and before any unit was dispatched:
review the three layers, validate their impact, and propose an option for each open decision. Three
read-only reviews, one per layer, read llm at `f2da03d4` (tag 0.7.0 plus the opening store commit)
and the consumers at Loom `baea5e70`, llm-gateway `bc872f2`, inference `5b64301` (its
`story:llm-over-b10x-llm` merged during the review) and the Harness spike `8989d138`. Nothing was
built. **V** = verified by reading or command output, with the source; **I** = inferred.

## Summary

| Layer | Story | Recommendation | Consumer code that breaks |
| --- | --- | --- | --- |
| 1 | `story:capabilities-declare-streaming` | two bools, written only when not default; one protocol check; `streaming: false` refused | 6 full struct literals: Loom 4, llm-gateway 1, Harness spike 1 (V) |
| 1 | `story:harness-cutover-on-llm-0-7` | cite tag 0.7.0; re-anchor by name with a recorded command; hand-check about 43 cells | none |
| 2 | `story:text-deltas-are-final` | the shown deltas are the outcome's text; a fixed `Warning` when a server's text was replaced; Messages start text emitted as a delta | none; llm-gateway's refusal after streamed text goes away (V by reading) |
| 3 | `story:tool-result-rejected-or-failed` | no `Item` change: document the convention, fix Chat ingress reading every failure as success | none |

## Layer 1: `story:capabilities-declare-streaming`

**1.1 Shape.** A: two bools `output_streaming`, `input_during_output`. B: one enum. C: only
`output_streaming` now. inference's node reads two separate claims, `Capability::OutputStreaming`
and `Capability::ConcurrentInputOutput` (`inference-core/src/model.rs:33-37`, read at
`inference-llm/src/lib.rs:639`, `:418`) (V). **A**: it matches what the consumer reads, and consumers
break once, not twice.

**1.2 Serialization.** A: serde default plus `skip_serializing_if` when default. B: always written.
C: new `llm.binding/2` and `llm.catalog/2`. Under A a pre-change document serializes to the same
bytes, so `binding_revision` (`crates/llm-providers/src/binding.rs:86-97`) and `config_digest`
(`crates/llm-routing/src/catalog.rs:176-194`) do not move (V, read in serde_derive 1.0.229 and
serde_json 1.0.151; not run). Under B every revision and digest moves, opaque state stored before
the upgrade is refused (`crates/llm-core/src/turn.rs:226-230`), and 0.7.0 readers refuse what the new
version writes; 7 routing scenarios, 21 Messages scenarios and `website/docs/guides/explain-a-route.md`
move (V, grep). Precedents for A: `crates/llm-core/src/error.rs:47`, `turn.rs:89`. **A.**

**1.3 Where `input_during_output: true` is refused.** A: `Capabilities::validate` (ignores
protocol, also refuses Models built in code). B: `BindingDocument::bind` only (a routes refusal
would not name the route, `crates/llm-models/src/routes.rs:655-663`). C: one llm-core function
taking the protocol, called from `bind` and from `Routes::parse` at `routes.rs:730-732`. **C**: the
rule depends on the protocol and the routes module promises refusals name the route
(`routes.rs:31-35`).

**1.4 A document declaring `output_streaming: false`.** A: refuse it, all three clients always
stream (`crates/llm-responses/src/request.rs:153`, `crates/llm-messages/src/codec.rs:148`,
`crates/llm-chat/src/client.rs:61`) (V). B: accept it as the caller's claim (untrue; nothing
enforces it). C: make the clients buffer. **A**: with 1.2 A, no accepted document then carries a
non-default value, so no hash moves and 0.7.0 readers read everything llm writes. Lifting the
refusal later breaks nothing.

**1.5 Consumer break.** A: plain public fields. B: `#[non_exhaustive]` plus setters (forbids struct
expressions outside the crate, functional update included: 21 consumer sites and 18 in llm,
including a quoted guide program). C: plain fields now plus additive `with_*` setters; the
CHANGELOG points literal writers to `..Capabilities::text(…)`. Full literals that break under A or C:
Loom `loom-intake-router/tests/adversary_classify.rs:49`, `classify.rs:42`,
`loom-plugin/tests/support/mod.rs:277`, `loom-plugin-slack/tests/support/mod.rs:266`; llm-gateway
`llm-gateway-cli/src/translate.rs:226`; Harness spike `harness-cli/src/llm_port.rs:286`; 17 in llm.
inference has 0 full literals (3 use `..Capabilities::text`) (V). **C.**

**1.6 Corrections to the story.** The string-route default is documented as "every boolean true"
(`routes.rs:72-73`, `spec/domains/routes.yaml:65`); taken literally it would set
`input_during_output` to true, so the default is stated per field. `Capabilities::text`'s doc
(`crates/llm-core/src/vocabulary.rs:61`) says it refuses every optional setting; it will set
`output_streaming` true and the line changes. `website/docs/guides/configure-model-routes.md:61`
("all seven fields") changes (V).

## Layer 1: `story:harness-cutover-on-llm-0-7`

**1.7 What to cite against.** A: tag 0.7.0, plus a list of what the next release changes. B: the
wave's tree (unreleased; Harness pins tags). C: wait for the release carrying this wave. **A**:
Harness rebases onto 0.7.0 (Atlas ADR 0097). Next-release changes for the list: the full
`Capabilities` literal at `llm_port.rs:286-296` (layer 1); with layer 3 as recommended, no
`Item::ToolResult` change, so `:483-487` and `:568-572` hold (V).

**1.8 How to re-cite.** A: every citation by hand (about 390). B: pin 0.7.0 in the header,
re-anchor each cell by its test or symbol name with a recorded command, hand-check only the cells
that changed or did not resolve, plus a status check per row. C: drop line numbers. Measured: of
151 test anchors 111 sit on the same line at 0.7.0 and 40 moved, none gone; of 170 llm anchors 10
changed text and 33 were cited against trees after `aaf21d41` (V). **B.**

## Layer 2: `story:text-deltas-are-final`

**2.1 How the caller learns the outcome's text differs from what it was shown.**

| | What it does | Cost | Impact |
| --- | --- | --- | --- |
| A | keep "terminal text outranks deltas", add a `Warning` | small | llm-gateway still refuses at `close` after text was streamed (`crates/llm-responses/src/ingress.rs:466-491`, `crates/llm-messages/src/ingress.rs:416-448`, `llm-gateway-cli/src/translate.rs:1034-1046`) (V); a caller's history differs from what it spoke |
| B | a field on `TurnOutcome` | 30 llm files and 13 Loom files build `TurnOutcome`; serialized format change | same gateway refusal (V) |
| C | the shown deltas are the outcome's text; a `Warning` with a fixed code (`shown-text-kept`) when a server's own text was replaced | one post-pass in the Responses decoder's `terminate` | outcome equals what was shown; the gateway's `close` succeeds; Harness's R36 kept text is already the deltas (`crates/llm-responses/src/stream.rs:309-328`) (V) |
| D | refuse divergence | small | a turn already spoken becomes an error (I) |

**C.** Every consumer already forwards or drops `Warning` (Loom `loom-sdk/src/llm.rs:214-215`, Harness
spike `llm_port.rs:542-543`, the three ingress encoders) (V), so no consumer changes. One scenario
moves: `contracts/responses/scenarios/kept-text-a-terminal-message-outranks-the-shown-deltas.yaml`
(V).

**2.2 The Responses fix site.** Two routes let the outcome outrank the deltas: `keep_shown_text`
(`stream.rs:312-318`) and an empty terminal `output` taking the `output_item.done` message
(`:201-217`, `:366-387`). One post-pass in `terminate`, pairing by id where both sides have one and
by order otherwise, covers both (**B** of the review). When a real server sends terminal text that
differs from its deltas: **I don't know**; no recorded Responses stream exists in any repository read
(V).

**2.3 Messages `content_block_start` with text.** A: emit the start text as a `TextDelta`. B: refuse
it. C: warn. Anthropic's streaming documentation shows `"text": ""` in every text-block start and
does not say it is always empty; no fixture in any repository sends non-empty start text (V). **A**:
the text is model output, `thinking_delta` already works this way (`crates/llm-messages/src/decode.rs:494-501`),
and no test moves.

**2.4 The guarantee and its tests.** In `StreamEvent` and `Model::turn` docs: every event is final;
on success each assistant item's text is the concatenation of the `TextDelta`s shown for it; where a
server's text differed, the shown text is kept and `shown-text-kept` says so. Tests per protocol:
deltas equal outcome text; the warning appears exactly when text was replaced; feeding the same
events to the protocol's ingress encoder, `close(outcome)` succeeds.

## Layer 3: `story:tool-result-rejected-or-failed`

**3.1 Can a wire carry "rejected"?** No provider documents a rejected or denied tool-result state:
Messages has only `is_error`; Responses `function_call_output` and Chat tool messages carry text only
(V, provider documentation fetched). Any new key breaks a 0.7.0 reader: Responses reads an envelope
with an extra key as a *successful* plain string (`crates/llm-responses/src/request.rs:793-801`,
pinned by `contracts/responses/scenarios/ingress-reads-an-envelope-with-an-extra-field-as-a-plain-string.yaml`),
and Messages ingress refuses an unknown key (`codec.rs:648`) (V).

**3.2 The loops already keep the typed fact.** inference's `ToolOutcome` (`crates/inference-llm/src/tools.rs:59-88`,
reasons `SchemaInvalid | Denied | Superseded | Exhausted` in `inference-tools/src/lib.rs:89-98`);
Harness `refusal` beside the item (`harness-wire/src/item.rs:86-97`); Loom's copy
(`loom-executor/src/harness/wire/item.rs:92-100`) (V).

**3.3 Options for the story.**

| | What it does | Cost | What the model sees |
| --- | --- | --- | --- |
| A | the story as written: typed three-way state, per-wire encoding, ingress | breaks 8 source and 22 test sites in llm, Loom 2, inference 5-6, Harness 2; `llm.turn/5`; spec, contracts, four pages (V) | the caller's text plus `is_error` on Messages, as today |
| B | no `Item` change: document "a rejected call is `failed: true` with the reason in `output`" in `docs/design.md` and the protocol pages; fix Chat ingress, which reads every tool message as succeeded (`crates/llm-chat/src/ingress.rs:241-242`, contradicting `docs/chat.md:47-51`) so llm's own `{"error":…,"ok":false}` reads back as failed | no Rust API change; inference's `{"rejected":…}` convention becomes the documented one | unchanged |
| C | type only: an optional kind beside `failed` (serde default, skip when absent), encoded as failed on every wire | the same Rust break as A; `llm.turn/5` | unchanged |

**B**: no wire can carry the distinction, the loops already hold it, and the one defect found (Chat
ingress turning a failure into success) is fixed without touching `Item`. If the owner wants the
typed state anyway, C is the shape that keeps every existing byte (`failed` stays; old turns decode
unchanged; `llm.turn/3` and `/4` keep their bytes).

## What changes in the wave if the recommendations are taken

- Layer 3 shrinks to documentation and one Chat ingress fix; `story:tool-result-rejected-or-failed`
  is rewritten to that outcome before dispatch.
- The next release breaks only the 6 full `Capabilities` literals listed in 1.5.
- Layers stay in the same order: the deltas and tool-result units still share
  `docs/{responses,messages,chat}.md` and `docs/design.md`.
