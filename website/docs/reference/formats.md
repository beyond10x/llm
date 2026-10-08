---
title: Versioned formats
sidebar_position: 2
description: Every persisted document carries an explicit version, and an unknown version refuses.
---

# Versioned formats

Published Rust APIs follow the crate release's semantic version. Persisted documents carry an
explicit envelope, and an old or unknown version refuses rather than being coerced.

| Format | Carried by |
| --- | --- |
| `llm.turn/3` | Persisted neutral requests |
| `llm.outcome/4` | Persisted neutral outputs |
| `llm.binding/1` | Provider/account/endpoint/model declarations |
| `llm.catalog/1` | TOML routing catalogs |
| `llm.provider-description/1` | TOML provider descriptions: inference URL template, wires, authentication and pinned control-plane operations |
| `llm.prices/1` | Price books, TOML or JSON |
| `llm.usage/2` | Attributed usage observations; refuses v1 |
| `llm.cost/2` | Derived accounting reports |
| `llm.budget/1` | The SQLite budget journal |

These versions ship in llm's releases. They are not yet a published contract with a
compatibility policy (planned, see [Status](/docs/status)): until it is, a release may change a
version, and the change is named in the changelog.

## What a version is, and is not

An unversioned Rust value is an in-process value. It is not a claim of a stable vendor wire format,
and a derived inspection view or an in-process command is not an independently versioned wire.

An unsupported addition needs a version change before it is accepted. Nothing is silently widened.

## Bounds

| Input | Bound |
| --- | --- |
| Price book, observations | 1 MiB and 4096 entries each |
| Provider description | 64 KiB |
| Provider-description instance name | 1 through 48 bytes of `[a-z0-9]` |
| Price-book source description | 2048 bytes, non-blank Unicode, no control characters |
| Secret adapter bindings | 4096 per adapter |
| Concurrent blocking OS credential reads | 8 |
| Local secret file read | 1 MiB |
| Budget outstanding reservations | 1 through 65,536 |
| Money | `u64` nanounits; max `18446744073.709551615` currency units |

## Provider descriptions

An `llm.provider-description/1` document is what every consumer reads about one provider.
`ProviderDescription::parse` in `b10x-llm-providers` refuses unknown fields in every table.

| Table | Carries |
| --- | --- |
| `[provider]` | `id` and `category` |
| `[inference]` | `base_url_template`, `protocols` (non-empty, no repeats), `auth_kind` (`anonymous`, `bearer` or `api-key`, with `api_key_header` exactly when `api-key`) |
| `[control_plane]`, optional | `openapi_url` (HTTPS), `document_sha256` (64 lower-case hex digits), `server_url` (HTTPS), `auth_kind` |
| `[control_plane.operations]` | the `operationId` of `create_instance`, `list_instances`, `get_instance` and `delete_instance`, all four distinct |

The template holds `{instance}` exactly once, inside the host's first label, and a fixed domain
follows that label, so no instance can name a single-label host.
`inference_base_url(instance)` accepts only 1 through 48 bytes of `[a-z0-9]`, so an instance
cannot change the host, the path or the scheme. `subscription-oauth` is refused: a description is
API access. Parsing does no I/O. It never fetches the OpenAPI document and never checks the
digest against it; that is the reader's job.

The crate ships Runpod's description as `llm_providers::descriptions::runpod()`:
`https://{instance}-8000.proxy.runpod.net/v1/` serving Chat Completions, Messages and Responses
with a bearer key, and the Runpod REST control plane with `CreatePod`, `ListPods`, `GetPod` and
`DeletePod`. These values are checked against fixtures only. No live Runpod pod or control-plane
call has qualified them.

## Identity digests

Two digests appear in output and both are SHA-256 over validated canonical data:

- **Configuration digest** identifies a validated routing catalog. Row order and equivalent decimal
  spellings do not change it; a changed declared fact does.
- **Binding revision** hashes one complete validated binding — endpoint URL, upstream model, auth
  reference and capabilities. Secret bytes and credential generations are excluded, so rotating the
  same reference does not invalidate a continuation, while repointing an id does.

A local `SecretVersion` is a third digest, but it is redacted, non-serializable and must never be
logged or used as a public fingerprint.
