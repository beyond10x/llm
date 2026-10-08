---
format: aep.planning-md/3
id: story:runpod-provider-description
kind: story
status: active
title: Runpod is one provider description in llm-providers, read by every consumer
summary: A llm.provider-description/1 document names a provider's inference URL template, wires and authentication and its control plane's pinned OpenAPI operations; Runpod's ships in the crate
relations:
- decomposes: epic:access
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: CHANGELOG.md
- confidence: inferred
  path: Cargo.lock
- confidence: cited
  path: checks/conformance/src/providers.rs
- confidence: inferred
  path: contracts/baseline.json
- confidence: cited
  path: contracts/ess-inputs.yaml
- confidence: inferred
  path: contracts/providers/scenarios
- confidence: inferred
  path: contracts/schema
- confidence: inferred
  path: contracts/suite.json
- confidence: inferred
  path: crates/llm-providers/Cargo.toml
- confidence: inferred
  path: crates/llm-providers/descriptions
- confidence: inferred
  path: crates/llm-providers/src
- confidence: cited
  path: crates/llm-providers/src/declaration.rs
- confidence: inferred
  path: crates/llm-providers/tests
- confidence: inferred
  path: docs/implementation-status.md
- confidence: cited
  path: spec/domains/providers.yaml
- confidence: inferred
  path: website/docs
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-10-08T10:37:51Z", actor: "human:timo", revision: 4}
- {from: "proposed", to: "active", at: "2026-10-08T10:37:51Z", actor: "human:timo", revision: 5}
---
## Context

llm-gateway serves models from Runpod pods. Today Runpod's API and the URL a pod serves at are written into the gateway's `crates/llm-runpod`, so every consumer that needs them (the gateway, Connectors, Loom) would write them again. The decided shape (llm-gateway `docs/design/runpod-clients.md` § 4, D1, option A) is one provider description in `crates/llm-providers`: Runpod's control plane is reached through Connectors, which compiles it from the pinned OpenAPI document the description names, and the inference side is built from the description's URL template, wires and authentication. The gateway then holds only the vLLM key. `Provider` is an `id` and a `category` today (`crates/llm-providers/src/declaration.rs`); there is no per-provider description.

llm-gateway's `story:runpod-production-transport` and `story:provider-key-files` wait on a release carrying this.

## Design

- A document format `llm.provider-description/1`, TOML, parsed by `ProviderDescription::parse` in `b10x-llm-providers` with every table `deny_unknown_fields`; at most 64 KiB.
  - `[provider]`: `id`, `category` (the existing `Provider`).
  - `[inference]`: `base_url_template`, `protocols` (non-empty, no repeats), `auth_kind` (`anonymous`, `bearer` or `api-key`; `api_key_header` exactly when `api-key`). `subscription-oauth` is refused: a description is API access, and subscription tokens stay a caller-managed account.
  - `[control_plane]`, optional: `openapi_url` (HTTPS, no userinfo, query or fragment), `openapi_sha256` (64 lowercase hex: the pinned document), `server_url` (HTTPS base), `auth_kind`, and `[control_plane.operations]` with the fixed roles `create_instance`, `list_instances`, `get_instance`, `delete_instance`, each an OpenAPI `operationId` (1-128 bytes of `[A-Za-z0-9_.-]`), all four distinct.
- The template holds the placeholder `{instance}` exactly once, inside the host's first label, and no other brace; with the placeholder filled it must be a valid `BaseUrl`.
- `ProviderDescription::inference_base_url(instance)` fills the template. An instance is 1-48 bytes of `[a-z0-9]`; anything else is refused `invalid` before substitution, so an instance can never change the host, the path or the scheme.
- The description performs no I/O: it never fetches the OpenAPI document and resolves no secret. Checking the digest against the document is the reader's job (Connectors).
- Runpod's description is the file `crates/llm-providers/descriptions/runpod.toml`, exported as `llm_providers::descriptions::runpod()`: provider `runpod`, category `gpu-cloud`; inference `https://{instance}-8000.proxy.runpod.net/v1/`, protocols `chat-completions`, `messages` and `responses` (vLLM's OpenAI-compatible server), `bearer`; control plane `https://rest.runpod.io/v1/openapi.json` (OpenAPI 3.0.3, sha256 `9500a8989878d53d8731f27bf8dbbd57801b328c760bdb32c38ba36d5cb580db`, read 2026-10-08), server `https://rest.runpod.io/v1`, `bearer`, operations `CreatePod`, `ListPods`, `GetPod`, `DeletePod`.

## Spec first

Model the description in `spec/domains/providers.yaml`: a command `llm.providers.Describe` taking a description document or the name of a shipped description and an optional instance, and an observation view naming the parsed provider, protocols, authentication, control plane and the resolved base URL, or the typed refusal. Validate with the newest `ess`, regenerate `contracts/suite.json` and `contracts/schema/`, add the conformance adapter in `checks/conformance/src/providers.rs`, and list the new scenarios in `contracts/ess-inputs.yaml`.

## Acceptance

Conformance scenarios in `contracts/providers/scenarios/`, all passing in `task conformance` with no skips and `contracts/baseline.json` raised by their count:
- the shipped Runpod description is accepted with its provider, three protocols, bearer authentication, pinned OpenAPI source and four operations;
- a Runpod instance resolves to `https://<instance>-8000.proxy.runpod.net/v1/`;
- an instance carrying a dot, a slash, an upper-case letter, nothing, or more than 48 bytes is refused and resolves no URL;
- a template without the placeholder, with it twice, or with it outside the host is refused;
- a description with no protocol, or a repeated protocol, is refused;
- an OpenAPI digest that is not 64 lowercase hex is refused;
- a control plane over plain HTTP is refused;
- two roles naming one operation are refused;
- an `api-key` description without a header, and a `subscription-oauth` description, are refused;
- an unknown field and an unknown format are refused as invalid documents;
- a description without a control plane is accepted.

## Verification

`task conformance`; `cargo test -p b10x-llm-providers`; `cargo clippy -p b10x-llm-providers --all-targets -- -D warnings`; `task docs`. No network I/O and no paid call.
