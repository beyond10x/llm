---
format: aep.planning-md/3
id: review-result:adversary-w09-tool-result-pass-1
kind: review-result
status: active
title: 'Wave 2026-10-20-w09 adversary, story:tool-result-rejected-or-failed, pass 1: five introduced'
relations:
- reviews: story:tool-result-rejected-or-failed
revision: 1
---
## Pass

Wave 2026-10-20-w09, `aep:adversary` pass 1 on `story:tool-result-rejected-or-failed`, head
`29f5bc84` (base `51a8f5b4`). The report as returned, paths outside the repository shortened.

```
unit: story:tool-result-rejected-or-failed, head 29f5bc84 plus one uncommitted test file
verdict: CONFIRMED
cases: executed 120→123, red 3
origin: introduced 5 / pre-existing 0 / undecided 0
wrote-outside-worktree: the assigned scratch
needs-coordinator: none
```

## Cases (`crates/llm-chat/tests/adversary_tool_result.rs`, all red)

- `adv_an_unusable_call_id_is_still_refused_before_the_content_is_read`: an empty `tool_call_id`
  with an unsupported content part is now refused `Unsupported` instead of `InvalidRequest`.
- `adv_a_deeply_nested_failed_output_the_chat_encoder_sent_decodes_back_failed`: a failed output
  nested 200 arrays deep reads back as a success (serde_json's 128-level limit).
- `adv_content_with_a_repeated_key_stays_a_successful_string`: `{"error":"x","ok":true,"ok":false}`
  reads back failed (last value wins) and loses its bytes.

`cargo test --locked --no-fail-fast -p b10x-llm-chat`: 120 passed, 3 failed, exit 101. Conformance
883/883 and `llm-docs generate --check` exit 0.

## Attacked and not broken

Whitespace, key order, CRLF; `ok` as 0, "false", `[]`, `{}` or null; other letter cases, padded
keys, a BOM, trailing text, two objects, `1e400`, truncated JSON all stay successful strings byte
for byte; an `error` value of every JSON type reads back failed with that value; content split
across parts; a 200 KiB failure object; the two pinned scenarios unchanged; `contracts/suite.json`
and `contracts/schema/` regenerate identical; the Messages docs match the code.

```findings
- file: crates/llm-chat/src/ingress.rs
  line: 239
  category: contract-drift
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: hoisting content_text above call_id_of changes the refusal for a tool message with an unusable call id and an unsupported content part from InvalidRequest to Unsupported
- file: crates/llm-chat/src/ingress.rs
  line: 260
  category: boundary
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: a failed output nested deeper than serde_json's 128-level recursion limit is sent as the failure object but reads back as a success, contrary to docs/chat.md:196 and docs/design.md:101
- file: crates/llm-chat/src/ingress.rs
  line: 260
  category: boundary
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: content with a repeated key such as {"error":"x","ok":true,"ok":false} collapses last-wins to a failure and loses its bytes, contrary to the lossless claim at docs/chat.md:199
- file: docs/design.md
  line: 98
  category: contract-drift
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the Responses ingress cell says any other string is a success carrying the string, but an ok-true envelope is a success carrying its output value (request.rs:797)
- file: crates/llm-chat/src/ingress.rs
  line: 260
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: every tool message's content is fully parsed before the 256 KiB result bound refuses it, 111.6 ms for an 8 MiB integer array against 6.6 ms for plain text
```
