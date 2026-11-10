---
format: aep.planning-md/3
id: review-result:adversary-github-zero-u3-pass-1
kind: review-result
status: active
title: Adversary, pass 1 (story:opencode-go-session-on-every-spelling, wave 2026-11-09-github-zero)
relations:
- reviews: story:opencode-go-session-on-every-spelling
revision: 1
---
unit: llm u3, story:opencode-go-session-on-every-spelling, commit 70d03d54 plus my one untracked test file
verdict: INFEASIBLE (red case, scenario constructed, nothing found that reaches it)
cases: executed 286→292, red 1
origin: introduced 1 / pre-existing 0 / undecided 0
wrote-outside-worktree: 2 paths
needs-coordinator: none

1. `git --no-pager diff --stat` printed nothing, because my only change is an untracked file. `git status --short` shows:
```
?? crates/llm-messages/tests/adversary_w2026_11_09_u3.rs
```
That is a test file. No implementation path was touched.

2. The cases I added are in `crates/llm-messages/tests/adversary_w2026_11_09_u3.rs`.
- `a_dot_dot_behind_an_encoded_separator_is_refused_or_sends_a_session` is **RED**. It builds three URLs: `https://opencode.ai/zen/..%2Finference/go/anthropic/v1`, `/x%2F..%2Finference/go/…` and `/zen%5C..%5Cinference/go/…`. For each it asserts that the client is refused with a message naming OpenCode Go, or that it carries a session.
- `url_crate_equivalents_send_a_session` is green. It covers `:443`, a trailing dot plus `:443`, `/./`, `%2E`, `%2E%2E`, `.%2e`, a host spelled `opencode%2Eai`, U+3002 used as the dot (also as a trailing dot), a soft hyphen in the host, mixed-case `%67%6f`, and a trailing `/`.
- `look_alike_hosts_are_not_opencode_go` is green. It covers `opencode.ai.evil.com` (with and without a trailing dot), `evilopencode.ai`, a punycode look-alike, `opencodé.ai`, `opencode.aii` and `www.opencode.ai.`, and checks that `with_session` stays `None` for all of them.
- `userinfo_never_forms_a_base_url` is green. `BaseUrl::new` refuses `x@`, `x:y@` and `opencode.ai@evil.example`.
- `is_opencode_go_agrees_with_the_constructor` is green. For 7 spellings, `is_opencode_go`, `session_header` and `with_session` agree, and `is_opencode_go` is false for every refused one.
- `the_refusal_carries_no_url_or_secret` is green. The refusal text, in both Display and Debug, contains none of the host, the path, `%2F`, `https://` or the key.

Red output from running this file alone (`RUSTUP_TOOLCHAIN=1.98.0 b10x-build-slot cargo test -p llm-messages --test adversary_w2026_11_09_u3`), before any suite run:
```
test a_dot_dot_behind_an_encoded_separator_is_refused_or_sends_a_session ... FAILED
thread 'a_dot_dot_behind_an_encoded_separator_is_refused_or_sends_a_session' (3060061) panicked at crates/llm-messages/tests/adversary_w2026_11_09_u3.rs:41:27:
https://opencode.ai/zen/..%2Finference/go/anthropic/v1 is built with no session and no refusal
test result: FAILED. 5 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
exit 101
```

3. The suite ran after the cases existed: `RUSTUP_TOOLCHAIN=1.98.0 b10x-build-slot cargo test -p llm-messages --no-fail-fast`. It exited 101. 292 tests ran: 291 passed and 1 failed, the case above. Every other binary passed, including `opencode_go_spellings` (4), `opencode_session` (8) and `adversary_w19_opencode_session` (4). The output ends with `error: 1 target failed:` / `exit 101`. The full log is in `suite.log`. The before count of 286 is 292 minus my file's 6, taken from the per-binary lines of this same run. I did not run the suite separately without my file.

4. Findings (all cover commit 70d03d54):

| file:line | verdict | origin | what was measured | what reaches it |
|---|---|---|---|---|
| crates/llm-messages/src/client.rs:100 | INFEASIBLE | introduced | `adversary_w2026_11_09_u3.rs:41` fails, exit 101. If the path read so far is not a prefix of `/inference/go/`, an encoded `/` or `\` returns `Ok(false)`. The unit refuses `%2F` because a server that decodes it could reach the route; on that same assumption, a `..` placed behind `%2F` also reaches it, and those spellings get neither a session nor a refusal. Suggested fix: refuse any encoded separator on the `opencode.ai` host, or do not refuse any of them. | Nothing found. No caller, default or doc uses such a spelling, and how the server treats `%2F` was not measured. |
| crates/llm-messages/src/client.rs:93-95 | INFEASIBLE | introduced | Judgement, no failing case. The refusal also fires on `/inference/go/anthropic%2Fv1`, where the prefix is intact and a session could be sent. The doc comment at :68-70 says the refusal applies only "where the OpenCode Go prefix is". The behaviour meets the acceptance, which allows "or refused". | Nothing found. |

5. What I attacked and could not break:
- **Userinfo:** `BaseUrl::new` refuses it.
- **Port:** an explicit `:443`, with or without a trailing dot, is dropped by the `url` crate.
- **Dot segments:** `/./`, `/../`, `%2E`, `%2e%2e` and `.%2e` are resolved before the check runs.
- **IDNA:** fullwidth letters, U+3002 and the soft hyphen are all mapped by the `url` crate to the plain host.
- **Mixed-case escapes:** decoded correctly.
- **Look-alike hosts:** none were treated as OpenCode Go.
- **Agreement:** `is_opencode_go`, `with_session` and `wire_headers` agree, because `wire_headers` reads `session_header`.
- **No leak:** the refusal is a fixed string.
- **Doubled `//`:** not counted as a finding. `adversary_w19_opencode_session.rs:84` already asserts `/inference//go/` is not OpenCode Go.

6. Paths written outside the worktree (under the unit's assigned `scratch-adv/` directory):
- `case.log`
- `suite.log`

The directory `scratch-adv` was created for them. My lease (`adv-llm-u3`) is released.

```findings
- file: crates/llm-messages/src/client.rs
  line: 100
  category: acceptance
  severity: warning
  verdict: INFEASIBLE
  origin: introduced
  message: "an encoded separator before the prefix returns Ok(false), so /zen/..%2Finference/go/ gets neither a session nor the refusal the unit applies to /inference%2fgo/ on the same server-decodes assumption"
- file: crates/llm-messages/src/client.rs
  line: 93
  category: judgement
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: "the refusal also fires on an encoded separator below an intact /inference/go/ prefix, which the doc comment's wording 'where the OpenCode Go prefix is' does not cover"
```
