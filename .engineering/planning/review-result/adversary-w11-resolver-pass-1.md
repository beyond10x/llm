---
format: aep.planning-md/3
id: review-result:adversary-w11-resolver-pass-1
kind: review-result
status: active
title: 'Wave 2026-10-20-w11 adversary, story:a-resolver-for-anonymous-bindings, pass 1: a misused resolver falls back'
relations:
- reviews: story:a-resolver-for-anonymous-bindings
revision: 1
---
## Pass

Wave 2026-10-20-w11, `aep:adversary` pass 1 on `story:a-resolver-for-anonymous-bindings`, head
`1f871e33` (base `f88f6907`). The report as returned, paths outside the repository shortened.

```
unit: story:a-resolver-for-anonymous-bindings at 1f871e33 (plus 1 untracked adversary test file)
verdict: NEEDS-CHANGE
cases: executed 1024→1027, red 2
origin: introduced 1, pre-existing 0, undecided 0
wrote-outside-worktree: 6 logs under the assigned scratch
needs-coordinator: fixing it conflicts with the story's acceptance (InvalidReference)
```

## Cases

In `crates/llm-models/tests/adversary_w11_anonymous_resolver.rs`:

- green `every_protocol_client_built_with_the_anonymous_resolver_sends_no_credential`: Chat,
  Responses and Messages send none of `authorization`, `x-api-key`, `api-key`, `anthropic-beta`,
  `cookie`, and never reach the resolver.
- red `a_credentialed_binding_handed_the_anonymous_resolver_is_refused_unauthorized_before_sending`:
  `Unavailable`, `NotSent` instead of `Unauthorized`.
- red `a_credentialed_target_handed_the_anonymous_resolver_through_a_lookup_does_not_fall_back`:
  the run fell back to the next target and succeeded.

## Attacked and not broken

No credential header or refresh on any wire; Codex and subscription bindings cannot be anonymous;
`b10x-llm-models` behaves the same for every `AuthKind`; feature builds pass; no other private copy
remains in `crates/*/src`.

```findings
- file: crates/llm-credentials/src/lib.rs
  line: 225
  category: acceptance
  severity: warning
  verdict: INFEASIBLE
  origin: introduced
  message: "the exported AnonymousResolver refuses with InvalidReference, which prepare_auth reports as unavailable/NotSent, so a credentialed binding handed it by mistake falls back to the next target instead of stopping as unauthorized; no known caller builds that state"
```

## Routing

Back to the implementor: the resolver refuses with `SecretError::Missing`, which `secret_error`
maps to `unauthorized` and no fallback takes (`crates/llm-providers/src/auth.rs:109-121`); the
adversary's file stays in the unit.
