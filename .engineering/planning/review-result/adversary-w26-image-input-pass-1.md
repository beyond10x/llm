---
format: aep.planning-md/3
id: review-result:adversary-w26-image-input-pass-1
kind: review-result
status: active
title: Wave 2026-11-05-w26 adversary pass, llm story:image-input, pass 1
relations:
- reviews: story:image-input
revision: 1
---
```
unit: beyond10x/llm story:image-input, commit 8a8e3f9f7 with wave merge 10ba66c10
verdict: NEEDS-CHANGE
cases: executed 21→33, red 4
origin: introduced 6 / pre-existing 0 / undecided 0
```

The operator asked for two checks beyond a normal pass. The Messages image wire names (`image`,
`source`, `base64`/`url`, `media_type`, `data`, `url`, the four media types) match
https://platform.claude.com/docs/en/api/messages.md lines 260-294, fetched 2026-11-05;
`wire_names.rs` now lists them as confirmed. Each of the four reachable new Messages refusals has an
aimed case that kills a real mutant (r1a, r1b and r4 had survived the unit's tests); the falsification
record lists them as aimed. The fifth, a non-exhaustive wildcard arm, stays unaimed.

Red: InvokeModel sent a `url` image source, while
https://platform.claude.com/docs/en/build-with-claude/vision.md lines 32-34 and 60 document only
base64 sources on Bedrock; InvokeModel had no 5 MB base64 per-image bound (vision.md:1283);
`from_base64` accepted MAX_IMAGE_BYTES+1; `https://@/a.png` passed as naming a host. All fixed in
5f364e982 (merged e2fc94f3b), cases committed in the three `adversary_w26_image_input.rs` files.

```findings
- file: crates/llm-bedrock/src/invoke.rs
  line: 60
  category: acceptance
  severity: blocker
  verdict: NEEDS-CHANGE
  origin: introduced
  message: InvokeModel sent a url image source although only base64 is documented on Bedrock InvokeModel
- file: crates/llm-bedrock/src/invoke.rs
  line: 84
  category: contract-drift
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: no 5 MB base64 per-image bound on InvokeModel before I/O
- file: crates/llm-core/src/image.rs
  line: 110
  category: boundary
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: from_base64 accepted text decoding to one byte past the bound
- file: crates/llm-core/src/image.rs
  line: 180
  category: boundary
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: validate accepted an https URL whose authority names no host
- file: crates/llm-messages/src/codec.rs
  line: 949
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: bypassing the missing-source refusal survived the unit's tests
- file: crates/llm-messages/src/codec.rs
  line: 966
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: reading an untyped image source as base64 survived the unit's tests
```
