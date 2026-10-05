# LLM

Rust libraries for calling language models through one neutral interface, `Model::turn`. Behind
that port an operator chooses the wire protocol (Responses, Messages or Chat Completions), the
account, the credential source, the route and its fallbacks; llm keeps each choice separate and
visible, and never turns an unknown usage count into a zero.

**Documentation: <https://beyond10x.github.io/llm/>**

**Status: 0.2.0. Libraries tested against fixtures; no provider route is qualified yet.** The
[status page](https://beyond10x.github.io/llm/docs/status) lists every capability as shipped or
planned, and [Limitations](https://beyond10x.github.io/llm/docs/status/limitations) says what is
missing.

## What it is not

- **Not an agent loop.** llm never runs a tool. A caller such as
  [Loom](https://beyond10x.github.io/loom/) ([GitHub](https://github.com/beyond10x/loom)) runs the
  loop and executes the tools the model asks for.
- **Not a credential store.** Credentials come from a resolver the caller injects; llm never runs
  a login flow. One adapter reads through the
  [Secrets](https://beyond10x.github.io/secrets/) ([GitHub](https://github.com/beyond10x/secrets))
  library's storage.
- **Not a server.** The gateway, hosting and provisioning crates moved to
  [llm-gateway](https://github.com/beyond10x/llm-gateway) in 0.2.0; it has no documentation site
  yet.

## Depend on it

Nothing is published to crates.io. Depend on a crate from Git at a release tag:

```toml
[dependencies]
llm-core = { package = "b10x-llm-core", git = "https://github.com/beyond10x/llm", tag = "0.2.0" }
llm-responses = { package = "b10x-llm-responses", git = "https://github.com/beyond10x/llm", tag = "0.2.0" }
```

Package names start with `b10x-`; library names do not, so the code says `use llm_core::…`. The
`secrets` feature of `b10x-llm-credentials` is on `main` and not yet in a release.

## Try it

Rust 1.98 or newer is enough. This example implements `Model` in-process and calls it through the
same port a real client implements; it sends nothing and reads no credential:

```console
$ cargo run --locked -p b10x-llm-core --example embedded
An embedded model turn. (EndTurn; usage None)
```

`usage None` is deliberate: the example reports no counters, so none are shown.
[Getting started](https://beyond10x.github.io/llm/docs/getting-started) runs the other offline
examples (a forced tool call, a route explanation, a price quote) and points at the guides.

## Crates

| Package | What it is |
| --- | --- |
| `b10x-llm-core` | The neutral turn: `Model`, requests, events, outcomes, typed failures; no I/O |
| `b10x-llm-http` | Bounded single-attempt HTTP and SSE transport |
| `b10x-llm-responses`, `b10x-llm-messages`, `b10x-llm-chat` | One protocol each: projection both ways and a client |
| `b10x-llm-credentials` | Injected secret resolution; opt-in file, keychain, environment, JSON pointer, Codex login and `secrets` adapters |
| `b10x-llm-providers` | Provider, account and endpoint bindings, independent of protocol |
| `b10x-llm-routing` | TOML catalogs, route explanation, same-target retry and ordered fallback |
| `b10x-llm-cost` | Usage pricing in exact decimals; a SQLite spending ledger behind feature `sqlite` |
| `b10x-llm-tool-call` | `call_tool` (one forced tool, its JSON input back) and the Codex Responses preset |
| `b10x-llm-blocking` | A blocking adapter over any `Model` for a synchronous loop |
| `b10x-llm-cli` | Placeholder; exports no API yet |

The [crate reference](https://beyond10x.github.io/llm/docs/reference/crates) is generated from
`cargo metadata` and lists each package's library name and features.

## Build and check

```bash
cargo test --workspace --locked
task check
```

`task check` is the full gate. Besides Rust it needs [Task](https://taskfile.dev),
[ESS](https://beyond10x.github.io/ess/) ([GitHub](https://github.com/beyond10x/ess)) 0.52.0 and
[AEP](https://beyond10x.github.io/ecosystem/aep/) ([GitHub](https://github.com/beyond10x/aep))
0.68.0; [Run the checks](https://beyond10x.github.io/llm/docs/guides/run-the-checks) says what it
proves and what it does not. The site builds with
`npm --prefix website ci && npm --prefix website run build`. Contributors and agents start at
[AGENTS.md](AGENTS.md).

## License

Apache-2.0. See [LICENSE](LICENSE).
