# LLM

Rust libraries for calling language models through one neutral interface. A caller runs a turn
through `Model::turn`; behind it, an operator chooses the wire protocol (Responses, Messages or
Chat Completions), the account, the credential source, the route and its fallbacks, and llm keeps
each choice separate and visible. It never runs a tool, never runs a login flow, and never turns an
unknown usage count into a zero.

**Documentation: <https://beyond10x.github.io/llm/>**

**Status: 0.1.7, libraries tested against fixtures; no provider route qualified.** The clients,
credential adapters, routing with same-target retry and ordered fallback, the `call_tool` and
blocking helpers, and usage pricing ship and are tested in the gate against recorded responses,
loopback sockets and in-process fakes. llm is the client side: the gateway and hosting crates
(`b10x-llm-gateway`, `b10x-llm-provision`, `b10x-llm-runpod`, `b10x-llm-modal`) moved to
[llm-gateway](https://github.com/beyond10x/llm-gateway), which serves what these clients call. The
[status page](https://beyond10x.github.io/llm/docs/status) lists every capability.

## Crates

| Package | What it is |
| --- | --- |
| `b10x-llm-core` | The neutral turn: `Model`, requests, events, outcomes, typed failures |
| `b10x-llm-http` | Bounded single-attempt HTTP and SSE transport |
| `b10x-llm-responses`, `b10x-llm-messages`, `b10x-llm-chat` | Protocol projections and clients |
| `b10x-llm-credentials` | Injected secret resolution and opt-in adapters |
| `b10x-llm-providers` | Provider, account and endpoint bindings |
| `b10x-llm-routing` | TOML catalogs, explanation, retry and ordered fallback |
| `b10x-llm-cost` | Usage pricing and the optional spending ledger |
| `b10x-llm-tool-call` | `call_tool` and the Codex Responses preset |
| `b10x-llm-blocking` | A blocking adapter for a synchronous loop |
| `b10x-llm-cli` | Not yet built |
| `llm-docs` | Generates and checks the site's derived pages |

The [crate reference](https://beyond10x.github.io/llm/docs/reference/crates) is generated from
`cargo metadata` and lists every package with its features.

## Build and run

```bash
cargo test --workspace --locked
cargo run --locked -p b10x-llm-core --example embedded
cargo run --locked -p llm-docs --example forced_tool_call
task check
```

`task check` also needs [Task](https://taskfile.dev),
[ESS](https://beyond10x.github.io/ess/) ([GitHub](https://github.com/beyond10x/ess)) and
[AEP](https://beyond10x.github.io/docs/aep/) ([GitHub](https://github.com/beyond10x/aep)); [Getting started](https://beyond10x.github.io/llm/docs/getting-started)
lists the versions. To build the site: `npm --prefix website ci && npm --prefix website run build`.

## License

Apache-2.0. See [LICENSE](LICENSE).

<!-- b10x-docs:start -->
## Documentation

[LLM documentation](https://beyond10x.github.io/docs/llm/) · [Start](https://beyond10x.github.io/) · [Ecosystem](https://beyond10x.github.io/ecosystem/) · [Impact](https://beyond10x.github.io/changes/) · [Releases](https://beyond10x.github.io/releases/)
<!-- b10x-docs:end -->
