# LLM

Composable model inference, provider integration, routing, provisioning and a gateway.

**Status: foundation implementation in progress.** `llm-core` supplies the asynchronous neutral
interface, `llm-credentials` supplies injected secret resolution, coordinated renewal and
[optional local adapters](docs/local-secrets.md), and
`llm-http` supplies bounded single-attempt HTTP/SSE transport. `llm-providers` validates independent
provider/account/auth/protocol bindings; `llm-routing` parses TOML and explains capability-aware
selection without I/O. `llm-cost` prices attributed usage with exact arithmetic and explicit
unknowns, keeping estimates and recorded charges separate. Its optional SQLite budget ledger
adds durable reservations, concurrency admission and explicit shutdown obligations. `llm-routing`
also runs ordered fallback over caller-supplied models. `llm-chat`, `llm-messages` and
`llm-responses` project the neutral turn onto Chat Completions, Messages and Responses; the first
two include single-attempt clients. `llm-gateway` authenticates one owner and serves a read-only
route inventory, without translating model calls. `llm-provision` defines the hosting lifecycle, and
`llm-runpod` implements it for vLLM against an in-process emulator only. `llm-modal` and `llm-cli`
are empty placeholders. Everything is tested against fixtures and local sockets; no live provider
or hosting account has been used, and there is no release or deployment.

The [implementation status](docs/implementation-status.md) maps the whole milestone to its AEP
stories under `.engineering/planning/`. The [design](docs/design.md) records the agreed target,
the [versioned contract](docs/contract-v1.md) defines the implemented boundary, and
the [domain](spec/system.yaml) gives its nouns typed homes.

The first milestone builds the full foundation here. Harness and Metaharness adopt a released
contract afterwards; the existing `llmgw` remains operational until a qualified reversible cutover.

## Checks

Run `task check` with Rust 1.98, the newest AEP and ESS releases (AEP 0.60.0 and ESS 0.35.0 at the time of writing). `task rust` runs the Rust tests,
formatting and lint checks without requiring the planning tools. Tests use injected models,
resolvers and local HTTP fixtures; they make no paid provider calls. See the
[verification record](docs/verification/core-foundation.md) for tested behavior and limits.
`task conformance` regenerates ESS schemas and the foundation suite, checks drift, then runs the real
libraries three times through the pinned ESS runner. Its report gate refuses missing coverage,
failures, errors, unsupported observations and skips. See [local-secret verification](docs/verification/local-secrets.md)
and the earlier [routing verification](docs/verification/routing-conformance.md).

The [public-surface verification](docs/verification/public-surface.md) records the documentation manifest, site and catalog declaration, and names the three steps outside
this repository that the Atlas change is blocked on.

Run `cargo run --locked -p b10x-llm-cost --example quote` for a local price/usage fixture.
[Pricing](docs/pricing.md) explains exact amounts, cache partitions, compute units and separate
subscription charges. [Budgets](docs/budgets.md) explains durable spending admission and its limits;
[budget verification](docs/verification/budgets.md) records the current suite.

Run the embedding example with `cargo run --locked -p b10x-llm-core --example embedded`.
It completes a local model turn without a gateway or credentials.

Run `cargo run --locked -p b10x-llm-routing --example explain` to inspect
[a TOML catalog](examples/catalog.toml) containing an arbitrary vLLM endpoint and an authenticated
alternative. This inspects configuration and sends no request.

## License

LicenseRef-B10x-Proprietary. Source publication does not grant an open-source license.

<!-- b10x-docs:start -->
## Documentation

[LLM documentation](https://beyond10x.github.io/docs/llm/) · [Start](https://beyond10x.github.io/) · [Ecosystem](https://beyond10x.github.io/ecosystem/) · [Impact](https://beyond10x.github.io/changes/) · [Releases](https://beyond10x.github.io/releases/)
<!-- b10x-docs:end -->
