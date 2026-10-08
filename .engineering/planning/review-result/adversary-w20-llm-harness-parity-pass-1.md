---
format: aep.planning-md/3
id: review-result:adversary-w20-llm-harness-parity-pass-1
kind: review-result
status: archived
title: Wave 2026-10-05-w20 adversary, llm story:harness-parity, pass 1
relations:
- reviews: story:harness-parity
revision: 2
transitions:
- {from: "active", to: "archived", at: "2026-10-08T10:43:37Z", actor: "human:timo", revision: 2}
---
```
unit: llm/harness-parity — docs/harness-parity.md (untracked) in llm-w20-harness-parity, base aaf21d41, harness origin/main 2fd7235b
verdict: red
cases: rows checked 143 by script (151 llm test citations, 367 Harness citations), 60 read in full (every covered row in harness-http and harness-credential)
origin: introduced 9 / pre-existing 0 / undecided 0
wrote-outside-worktree: 1 directory — ~/.cache/ga-wave-2026-10-05-w20/llm-harness-parity/scratch/adv1/
needs-coordinator: none
```

| Row | What the document says | What I found | Evidence |
|---|---|---|---|
| H8 (doc :39) | llm gives `Protocol` with `Dispatch::Unknown`; `covered` | Through the transport the caller gets `Dispatch::Accepted`, and the test the row cites asserts `Accepted`. `Accepted` is never eligible for fallback, so Harness's retriable truncation has no llm equivalent. Should be `partial` and listed under Gaps. | `llm-http/src/transport.rs:254-256`; `llm-http/tests/transport.rs:91`; `llm-routing/src/fallback.rs:140-146` |
| H23/H13 (doc :54, :239) | `partial`: "no retriable flag" | Understated. Harness retries 408, 5xx and 529. llm marks those `Transport`/`Unknown`, which fallback refuses, and an llm test pins 503 as `Unknown`. Of Harness's retriable classes, only 429 falls back in llm. | `harness-http/src/status.rs:31`; `llm-http/src/transport.rs:268`; `llm-responses/tests/adversary2_client.rs:505-511` |
| R18 (doc :114) | `covered` | Harness test `a_string_result_is_passed_through_unquoted` pins plain text. llm wraps every result as `{"ok":true,"output":…}`; failed results use `error` in Harness and `output` in llm. Should be `partial` and listed under Gaps. | `harness-responses/src/project.rs:42-46`, `:536`; `llm-responses/src/request.rs:407-411`; `llm-responses/tests/projection.rs:166-167` |
| M39 (doc :184) | `covered` | Contradicts M19 and M21: the Harness test checks three refusals before sending; the llm test checks only temperature. Should be `partial`. | `harness-messages/src/lib.rs:1249`; `llm-messages/tests/projection.rs:88` |
| C7 (doc :77) | `covered` | The cited test asserts the path is absent from `FileResolver`'s `Debug`; Harness `oauth.rs:260` asserts the source is named. Should be `partial` for the file source. | `llm-credentials/tests/local_adapters.rs:147-159` |
| H25 (doc :56) | llm `transport.rs:116` exercised by `every_unsent_refusal_is_a_valid_not_sent_failure` | That test is refused earlier in `prepare_auth`; :116 is never reached. Note. | `llm-providers/src/auth.rs:38`; `llm-responses/tests/adversary2_client.rs:462-473` |
| R35 (doc :131) | `covered` (deliberate difference) | llm does the opposite of Harness test `lib.rs:865`; not `covered` under the document's definition and missing from Gaps. Note. | `llm-responses/src/stream.rs:251-260` |
| R14 (doc :110) | `partial`: no llm test asserts absence | The round-trip test fails if a numeric default is emitted; only an emitted `null` would pass. Note. | `llm-responses/tests/projection.rs:151`, `:157`; `llm-responses/src/request.rs:183-189` |
| completeness | one row per test-pinned behaviour | 3 Harness tests have no row: `renewal.rs:789`, `harness-messages/src/lib.rs:981`, `harness-messages/tests/provider_emulated.rs:565`. Note. | as cited |

Held up: all 151 llm test citations at the cited line; all Harness citations; Gaps list matches gap/partial rows (M41 twice); counts 88/29/20/6 = 143; no personal paths; harness-cli call-site lines; C2 and H15 gaps; R36 analysis.

```findings
[
 {"file":"docs/harness-parity.md","line":39,"verdict":"CONFIRMED","origin":"introduced","category":"correctness","severity":"blocker","message":"H8 marked covered, but transport yields Dispatch::Accepted (llm-http/src/transport.rs:254-256), never eligible for fallback; should be partial and in Gaps."},
 {"file":"docs/harness-parity.md","line":54,"verdict":"CONFIRMED","origin":"introduced","category":"correctness","severity":"blocker","message":"H23/H13 omit that Harness-retriable 408/5xx/529 map to Dispatch::Unknown in llm (transport.rs:268), refused by fallback (fallback.rs:140-146); only 429 falls back."},
 {"file":"docs/harness-parity.md","line":114,"verdict":"CONFIRMED","origin":"introduced","category":"correctness","severity":"blocker","message":"R18 covered although llm wraps every tool result as {ok,output} where Harness pins plain text (project.rs:536); failed results use a different key; should be partial and in Gaps."},
 {"file":"docs/harness-parity.md","line":184,"verdict":"CONFIRMED","origin":"introduced","category":"correctness","severity":"blocker","message":"M39 covered by a temperature-only codec test while M19/M21 say the other two pre-flight refusals in Harness lib.rs:1249 have no llm test; should be partial."},
 {"file":"docs/harness-parity.md","line":77,"verdict":"CONFIRMED","origin":"introduced","category":"correctness","severity":"blocker","message":"C7 cites a test asserting the path is absent from FileResolver Debug, the opposite of Harness's 'Debug names the source'; should be partial for the file source."},
 {"file":"docs/harness-parity.md","line":56,"verdict":"CONFIRMED","origin":"introduced","category":"correctness","severity":"note","message":"H25's cited test is refused at llm-providers/src/auth.rs:38 and never reaches llm-http/src/transport.rs:116; no llm-http test cancels post_sse before it sends."},
 {"file":"docs/harness-parity.md","line":131,"verdict":"CONFIRMED","origin":"introduced","category":"correctness","severity":"note","message":"R35 covered although llm does the opposite of Harness test lib.rs:865; deliberate, but not covered by the document's definition and missing from Gaps."},
 {"file":"docs/harness-parity.md","line":110,"verdict":"CONFIRMED","origin":"introduced","category":"correctness","severity":"note","message":"R14 says no llm test asserts absence, but projection.rs:157 round-trip fails on an emitted numeric default; only an emitted null passes."},
 {"file":"docs/harness-parity.md","verdict":"CONFIRMED","origin":"introduced","category":"correctness","severity":"note","message":"Three Harness-pinned tests have no row: renewal.rs:789, harness-messages/src/lib.rs:981, harness-messages/tests/provider_emulated.rs:565."}
]
```
