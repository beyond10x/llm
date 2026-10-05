---
format: aep.planning-md/3
id: review-result:adversary-w31-llm-parity-codex-renewal-pass-1
kind: review-result
status: active
title: Wave 2026-10-05-w31 adversary, llm story:parity-codex-renewal, pass 1
relations:
- reviews: story:parity-codex-renewal
revision: 1
---
```
unit: llm/parity-codex-renewal, implementation 31716b6 plus one untracked adversary test file
verdict: red
cases: executed 69→79, red 3
origin: introduced 7 / pre-existing 0 / undecided 0
wrote-outside-worktree: ~/.cache/ga-wave-2026-10-05-w31/llm-parity-codex-renewal/scratch/ (logs; mutant copy and its 949M build dir, since deleted)
needs-coordinator: yes (refused and uncertain grants re-sent)
```

Cases added: `crates/llm-credentials/tests/adversary_codex_renewal.rs` (a refused grant is not sent again for the unchanged file; a dropped resolve does not let the next resolve re-send; a duplicated key is refused with the read rule's kind; 7 green probes). Credentials suite: 79 cases, 3 failed.

| # | file:line | verdict / origin | finding |
|---|---|---|---|
| F1 | crates/llm-credentials/src/codex/renewal.rs:468 | NEEDS-CHANGE / introduced | a refused refresh token is POSTed again on every later resolve of the unchanged file |
| F2 | crates/llm-credentials/src/codex/renewal.rs:469 | NEEDS-CHANGE / introduced | after a resolve dropped mid-grant, the next resolve sends the same grant again (against lib.rs:265) |
| F3 | crates/llm-credentials/src/codex/renewal.rs:528 | CONFIRMED / introduced | a duplicated key is refused Missing; the read rule says Unavailable |
| F4 | docs/harness-parity.md:91 | CONFIRMED / introduced | C20 covered without recording that llm renews inside a resolve, which Harness oauth.rs:18-22 avoids |
| F5 | spec/domains/secrets.yaml:81 | CONFIRMED / introduced | 6 of 12 refusal codes have no scenario; removing the symlink refusal leaves conformance green |
| F6 | crates/llm-credentials/src/codex/renewal.rs:687 | INFEASIBLE / introduced | rename splits a hardlinked auth.json; FileResolver refuses nlink != 1, renewal does not |
| F7 | crates/llm-credentials/src/codex/renewal.rs:363 | CONFIRMED / introduced | request body and answer Value freed unzeroized |

Not broken: no token in Display/Debug for bad answers; token characters (quote, backslash, control, U+2028, non-ASCII, emoji) written as valid JSON; escaped keys and values on disk; symlinked parent directory; modes 0400, 04600, 0640 kept; unwritable directory; exp extremes; 9 of 10 conformance mutants caught; no proxy, no redirect.

```findings
[{"file":"crates/llm-credentials/src/codex/renewal.rs","line":468,"category":"concurrency","severity":"warning","verdict":"NEEDS-CHANGE","origin":"introduced","message":"RenewingCodexAuthFile re-sends a refresh grant the endpoint refused on every later resolve of the unchanged file, unlike CoordinatedResolver which binds a refused generation"},{"file":"crates/llm-credentials/src/codex/renewal.rs","line":469,"category":"concurrency","severity":"warning","verdict":"NEEDS-CHANGE","origin":"introduced","message":"a resolve dropped mid-grant lets the next resolve present the same refresh token again, against lib.rs:265 rule for dropped refreshes"},{"file":"crates/llm-credentials/src/codex/renewal.rs","line":528,"category":"boundary","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"a duplicated key on the token path is refused Missing while the read rule refuses the same file Unavailable"},{"file":"docs/harness-parity.md","line":91,"category":"contract-drift","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"C20 is marked covered without recording that llm renews inside a resolve, which Harness oauth.rs:18-22 deliberately keeps out of reach of the token source"},{"file":"spec/domains/secrets.yaml","line":81,"category":"mutant","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"six declared renewal refusal codes have no scenario; removing the symlink refusal leaves the conformance check green"},{"file":"crates/llm-credentials/src/codex/renewal.rs","line":687,"category":"judgement","severity":"note","verdict":"INFEASIBLE","origin":"introduced","message":"rename splits a hardlinked auth.json, leaving the other name with a retired refresh token; no reaching caller found"},{"file":"crates/llm-credentials/src/codex/renewal.rs","line":363,"category":"judgement","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"the request body holding the refresh token and the answer Value holding new tokens are freed unzeroized"}]
```
