---
format: aep.planning-md/3
id: review-result:adversary-w45-llm-secrets-resolver-pass-1
kind: review-result
status: active
title: Wave 2026-10-05-w45 adversary, llm story:secrets-resolver, pass 1
relations:
- reviews: story:secrets-resolver
revision: 1
---
```
unit: beyond10x/llm story:secrets-resolver, worktree llm-w45b-secrets-resolver at 72973b2c (impl/secrets-resolver), plus the untracked adversary file
verdict: NEEDS-CHANGE
cases: executed 25→36, red 1
origin: introduced 4 / pre-existing 1 / undecided 0
wrote-outside-worktree: 1 (deleted)
needs-coordinator: decide whether the capability gap (finding 2) is fixed in code or the docs line is cut
```

Cases added: `crates/llm-credentials/tests/adversary_secrets.rs` (11 cases; 1 red: a backend without
Read answered a value). Held: path-shaped and lookalike references refused before any backend;
nested names stay in scope; a denied user reaches no backend; concurrent reads during rotation never
pair a version with two values; refresh racing rotation; size-bound and empty values; unversioned
A→B→A rejected again; error codes carry no secret bytes; zeroized wrappers (read, not tested);
feature off by default.

```findings
[
  {"file":"crates/llm-credentials/tests/secrets_boundary.rs","line":11,"category":"mutant","severity":"warning","verdict":"NEEDS-CHANGE","origin":"introduced","message":"The boundary rule names only secrets-core and secrets-keychain and checks only the owner's [features], so a core crate on another crate of the library source, or forwarding llm-credentials/secrets from its own features, passes all seven cases (shown on a scratch copy)."},
  {"file":"crates/llm-credentials/src/secrets.rs","line":91,"category":"contract-drift","severity":"note","verdict":"INFEASIBLE","origin":"introduced","message":"The resolver never consults capabilities(), so a backend without Read that answers a read returns a value, against credentials.md:68 'a backend without read UnsupportedPlatform'; no shipped backend lacks Read."},
  {"file":"checks/conformance/src/secrets.rs","line":824,"category":"judgement","severity":"note","verdict":"NEEDS-CHANGE","origin":"introduced","message":"Inserting observe_probe split the observe_renewal doc comment: observe_probe now opens with 'Dispatches llm.secrets.RenewCodexLogin' and observe_renewal has no doc."},
  {"file":"crates/llm-providers/src/auth.rs","line":114,"category":"judgement","severity":"note","verdict":"CONFIRMED","origin":"pre-existing","message":"InvalidReference and UnsafeSource, now reachable at resolve time through this adapter, classify as Unavailable and are retried four times per target with fallback although both are configuration errors."},
  {"file":"crates/llm-credentials/Cargo.toml","line":40,"category":"judgement","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"The first git-sourced optional dependency puts the secrets repository and its tree in every consumer's lockfile, so it is fetched even with the feature off."}
]
```
