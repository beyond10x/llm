# LLM

Rust libraries for calling language models through one neutral interface, `Model::turn`. Behind
that port an operator chooses the wire protocol (Responses, Messages or Chat Completions), the
account, the credential source, the route and its fallbacks; llm keeps each choice separate and
visible, and never turns an unknown usage count into a zero.

**Documentation: <https://beyond10x.github.io/llm/>**

**Status: 0.13.0. Libraries tested against fixtures; no provider route is qualified yet.** The
[status page](https://beyond10x.github.io/llm/docs/status) lists every capability as shipped or
planned, and [Limitations](https://beyond10x.github.io/llm/docs/status/limitations) says what is
missing.

## What it is not

- **Not an agent loop.** llm never runs a tool.
  [Harness](https://beyond10x.github.io/ecosystem/harness/) ([GitHub](https://github.com/beyond10x/harness))
  runs the agent loop and executes the tools the model asks for;
  [Loom](https://beyond10x.github.io/loom/) ([GitHub](https://github.com/beyond10x/loom)) calls llm
  for decisions; inference ([GitHub](https://github.com/beyond10x/inference)) builds on the same
  port. Each pins an llm release tag. Harness builds on 0.8.0 since it dropped its own model wire
  and credential crates, as Atlas ADR 0097 decided; [Harness parity](docs/harness-parity.md) maps
  what it needed.
- **Not a credential store.** Credentials come from a resolver the caller injects; llm never runs
  a login flow. One adapter reads through the
  [Secrets](https://beyond10x.github.io/secrets/) ([GitHub](https://github.com/beyond10x/secrets))
  library's storage.
- **Not a server.** The gateway, hosting and provisioning crates moved to
  [llm-gateway](https://github.com/beyond10x/llm-gateway) in 0.2.0.

## Depend on it

Nothing is published to crates.io. Depend on a crate from Git at a release tag:

```toml
[dependencies]
llm-core = { git = "https://github.com/beyond10x/llm", tag = "0.13.0" }
llm-responses = { git = "https://github.com/beyond10x/llm", tag = "0.13.0" }
```

A package name is its directory name; its library name uses underscores, so the code says `use llm_core::…`. The
`secrets` feature of `llm-credentials` is available in the release.

## Try it

Rust 1.98 or newer is enough. This example implements `Model` in-process and calls it through the
same port a real client implements; it sends nothing and reads no credential:

```console
$ cargo run --locked -p llm-core --example embedded
An embedded model turn. (EndTurn; usage None)
```

`usage None` is deliberate: the example reports no counters, so none are shown.
[Getting started](https://beyond10x.github.io/llm/docs/getting-started) runs the other offline
examples (a forced tool call, a route explanation, a price quote) and points at the guides.

## Crates

| Package | What it is |
| --- | --- |
| `llm-core` | The neutral turn: `Model`, requests, events, outcomes, typed failures; no I/O |
| `llm-http` | Bounded single-attempt HTTP and SSE transport |
| `llm-responses`, `llm-messages`, `llm-chat` | One protocol each: projection both ways, a gateway's ingress decoder and answer encoder, and a client |
| `llm-bedrock` | Amazon Bedrock Runtime over Converse and Anthropic `InvokeModel`: Signature Version 4, event-stream frames, static or container credentials; fixture-checked, no live call ([guide](https://beyond10x.github.io/llm/docs/guides/call-a-model-on-bedrock)) |
| `llm-credentials` | Injected secret resolution; opt-in file, keychain, environment, JSON pointer, Codex login and `secrets` adapters |
| `llm-providers` | Provider, account and endpoint bindings, independent of protocol; provider descriptions (`llm.provider-description/1`), with Runpod's built in |
| `llm-routing` | TOML catalogs, route explanation, same-target retry and ordered fallback |
| `llm-models` | Builds the protocol client a catalog serving model declares, with the caller's resolver for its account; reads `llm.routes/1` model routes in YAML or TOML, Bedrock endpoints included ([guide](https://beyond10x.github.io/llm/docs/guides/configure-model-routes)) |
| `llm-cost` | Usage pricing in exact decimals; a SQLite spending ledger behind feature `sqlite` |
| `llm-tool-call` | `call_tool` (one forced tool, its JSON input back) and the Codex Responses preset |
| `llm-blocking` | A blocking adapter over any `Model` for a synchronous loop |
| `llm-cli` | The `llm` command: lists the models an `llm.toml` routes file declares and streams one turn on any of them, with its usage and cost ([guide](https://beyond10x.github.io/llm/docs/guides/use-the-llm-command)) |

The [crate reference](https://beyond10x.github.io/llm/docs/reference/crates) is generated from
`cargo metadata` and lists each package's library name and features.

## Build and check

The gate and the toolchain it runs on are in [AGENTS.md](AGENTS.md#gate). Besides Rust it needs
[Task](https://taskfile.dev),
[ESS](https://beyond10x.github.io/ess/) ([GitHub](https://github.com/beyond10x/ess)) 0.67.0 and
[AEP](https://beyond10x.github.io/ecosystem/aep/) ([GitHub](https://github.com/beyond10x/aep))
0.68.0; [Run the checks](https://beyond10x.github.io/llm/docs/guides/run-the-checks) says what it
proves and what it does not. The site builds with
`npm --prefix website ci && npm --prefix website run build`. Contributors and agents start at
[AGENTS.md](AGENTS.md).

## License

Apache-2.0. See [LICENSE](LICENSE).
