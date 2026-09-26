---
title: Versioned formats
description: Every persisted document carries an explicit version, and an unknown version refuses.
---

# Versioned formats

Published Rust APIs follow the crate release's semantic version. Persisted documents carry an
explicit envelope, and an old or unknown version refuses rather than being coerced.

| Format | Carried by | Status |
| --- | --- | --- |
| `llm.turn/3` | Persisted neutral requests | Unreleased |
| `llm.outcome/4` | Persisted neutral outputs | Unreleased |
| `llm.binding/1` | Provider/account/endpoint/model declarations | Unreleased |
| `llm.catalog/1` | TOML routing catalogs | Unreleased |
| `llm.prices/1` | Price books, TOML or JSON | Unreleased |
| `llm.usage/2` | Attributed usage observations | Unreleased; refuses v1 |
| `llm.cost/2` | Derived accounting reports | Unreleased |
| `llm.budget/1` | The SQLite budget journal | Unreleased |

"Unreleased" is load-bearing. These are contract *changes in progress*, not implicit compatibility
conversions, and there is no released artifact that pins them.

## What a version is, and is not

An unversioned Rust value is an in-process value. It is not a claim of a stable vendor wire format,
and a derived inspection view or an in-process command is not an independently versioned wire.

An unsupported addition needs a version change before it is accepted. Nothing is silently widened.

## Bounds

| Input | Bound |
| --- | --- |
| Price book, observations | 1 MiB and 4096 entries each |
| Price-book source description | 2048 bytes, non-blank Unicode, no control characters |
| Secret adapter bindings | 4096 per adapter |
| Concurrent blocking OS credential reads | 8 |
| Local secret file read | 1 MiB |
| Budget outstanding reservations | 1 through 65,536 |
| Money | `u64` nanounits; max `18446744073.709551615` currency units |

## Identity digests

Two digests appear in output and both are SHA-256 over validated canonical data:

- **Configuration digest** identifies a validated routing catalog. Row order and equivalent decimal
  spellings do not change it; a changed declared fact does.
- **Binding revision** hashes one complete validated binding — endpoint URL, upstream model, auth
  reference and capabilities. Secret bytes and credential generations are excluded, so rotating the
  same reference does not invalidate a continuation, while repointing an id does.

A local `SecretVersion` is a third digest, but it is redacted, non-serializable and must never be
logged or used as a public fingerprint.
