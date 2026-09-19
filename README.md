# LLM

Composable model inference, provider integration, routing, provisioning and a gateway.

**Status: foundation implementation in progress.** `llm-core` supplies the asynchronous neutral
interface, `llm-credentials` supplies injected secret resolution, coordinated renewal and
[optional local adapters](docs/local-secrets.md), and
`llm-http` supplies bounded single-attempt HTTP/SSE transport. `llm-providers` validates independent
provider/account/auth/protocol bindings; `llm-routing` parses TOML and explains capability-aware
selection without I/O. The other nine runtime crates are planned
boundaries. There is no usable gateway, provider client, release or deployment yet.

The [implementation status](docs/implementation-status.md) maps the whole milestone to its AEP
stories under `.engineering/planning/`. The [design](docs/design.md) records the agreed target,
the [versioned contract](docs/contract-v1.md) defines the implemented boundary, and
the [domain](spec/system.yaml) gives its nouns typed homes.

The first milestone builds the full foundation here. Harness and Metaharness adopt a released
contract afterwards; the existing `llmgw` remains operational until a qualified reversible cutover.

## Checks

Run `task check` with Rust 1.98, AEP 0.55.0 and ESS 0.26.0. `task rust` runs the Rust tests,
formatting and lint checks without requiring the planning tools. Tests use injected models,
resolvers and local HTTP fixtures; they make no paid provider calls. See the
[verification record](docs/verification/core-foundation.md) for tested behavior and limits.
`task conformance` regenerates ESS schemas and the foundation suite, checks drift, then runs the real
libraries three times through the pinned ESS runner. Its report gate refuses missing coverage,
failures, errors, unsupported observations and skips. See [local-secret verification](docs/verification/local-secrets.md)
and the earlier [routing verification](docs/verification/routing-conformance.md).

Run the embedding example with `cargo run --locked -p b10x-llm-core --example embedded`.
It completes a local model turn without a gateway or credentials.

Run `cargo run --locked -p b10x-llm-routing --example explain` to inspect
[a TOML catalog](examples/catalog.toml) containing an arbitrary vLLM endpoint and an authenticated
alternative. This inspects configuration; protocol clients and runtime fallback are still pending.

## License

LicenseRef-B10x-Proprietary. Source publication does not grant an open-source license.
