# LLM foundation design

This is the target accepted by the operator on 2026-09-19, not a claim that the scaffold implements it.

## Ownership and sequence

Build the full LLM foundation before consumer migrations. Extract model-facing behavior from
Harness and the archived Platform inference component, and hosting behavior from llmgw, preserving
provenance. Depend on none of those consumers. Tool execution, approval envelopes, sandboxing,
delegation and agent-loop policy stay with Harness. Metaharness retains vendor process launch and
confinement. Retire llmgw only after a reversible gateway cutover.

## Neutral boundary

Core describes a model turn, tools as model-facing schemas/results, streaming, cancellation,
capabilities, usage and typed errors. It does not grant a tool permission to execute. Protocol
adapters implement Responses, Messages and Chat Completions with one documented text/tool/streaming
subset. Unsupported fields and provider-specific opaque continuation state must be preserved or
refused, never dropped to make translation appear successful.

## Credentials and configuration

Inference accepts an injected SecretResolver trait. A SecretReference is an opaque lookup name,
not a secret value or mandatory storage backend. Optional OS keychain and explicit file adapters
serve local applications; remote compositions inject their own implementation. Resolve at request
time to permit rotation. Return redacted short-lived material and safe errors. Caller-owned
credential acquisition and renewal remain outside inference; an injected refresh facility may be
used without owning a login flow or modifying another program's credential file.

TOML names providers/accounts, endpoints, served models/capabilities, routes and fallback order,
prices and provisioning references. IDs are operator-defined; no built-in-name-only resolver.
Authentication kind is separate from protocol. OpenAI API/subscription and Anthropic
API/subscription each have their own compatibility evidence; subscription access must not silently
fall back to billable API credentials. The future Connectors arbitrary-secret adapter must not
require changing route references. Core never depends on Connectors.

## Routing and accounting

Fallback is opt-in and ordered. Only defined failures before output becomes visible are eligible;
request capabilities, state compatibility, deadlines and remaining spending limits constrain every
alternative. Do not replay an ambiguously accepted request by assuming it was free. Record every
attempt and preserve unknown spend. No semantic downgrade, cross-account switch outside the named
chain, or fallback after an exposed partial stream.

Account for tokens, cache reads/writes, reasoning usage where reported, and provisioned compute.
Version price inputs; distinguish measured usage, estimates, subscription charges and unknowns.
Unknown is never zero. Define budget scope, reservations/concurrency, restart policy and uncertain
charges before claiming enforcement. An admission bound is not a promise about a provider's final
invoice. Product-level cheapest-model optimization is outside the first milestone.

## Gateway and hosting

Serve one owner/trusted deployment with authenticated access. Translate only the published neutral
subset across three protocol ingress surfaces; preserve streaming, tools, cancellation and failure
semantics. Listing or explaining routes must not resolve secrets or provision resources.

Existing endpoints do not require provisioning. Hosting adapters have explicit deploy/readiness,
owned-resource reconciliation, concurrency, lease and cleanup behavior. Implement Runpod and Modal
independently behind this seam. Cloud-specific provisioning capabilities require documented control
plane evidence; unsupported lifecycle actions must be reported rather than simulated. Prevent two
controllers from owning one billed resource. No deployment or paid qualification is authorized by
a normal repository gate.

## Source evidence and draft limits

- Harness `crates/harness-wire/src/{port,turn,bearer}.rs`: neutral turns/usage, credential injection,
  and the tool-authority coupling to leave behind; `crates/harness-loop/src/price.rs`: existing prices.
- Harness `crates/harness-cli/src/provider.rs`: endpoint, model, wire and credential bundle.
- Platform `runtime/inference/crates/inference-route/src/{catalog,vocabulary}.rs`: endpoint/model
  capabilities; archived source is evidence, never a retained dependency.
- llmgw `src/config.rs` and `src/runpod.rs`: provider/model configuration and deployment mechanics.
- Operator decisions: full foundation first; single owner; injected custody; caller-managed
  subscription auth; opt-in fallback; accounting/limits; subset translation; later llmgw retirement.

The catalog ESS domain describes declaration records with a single Declared state. The routing
and secrets verification domains describe adapter observations of real library calls; their
authored scenarios assert returned selection, refusal, request preservation and secret-resolution facts. They do not
promise runtime event publication, persistent routing jobs or hosting/budget state machines.
Budget ownership/scope is now resolved by [the single-owner ledger contract](budgets.md). Its ESS
domain declares the policy, owned obligations and inspection records with closed runtime phase
values; authored programs observe the real durable engine's transitions and refusals.
The hosting lifecycle is resolved: `spec/domains/hosting.yaml` declares the permitted transition
table the library enforces, and a pair absent from it is refused.
