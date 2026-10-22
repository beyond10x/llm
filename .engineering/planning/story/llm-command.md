---
format: aep.planning-md/3
id: story:llm-command
kind: story
status: draft
title: The llm command lists the configured models and runs one turn on any of them
relations:
- serves: vision:portable-model-inference
revision: 3
---
## Context

The owner, 2026-10-22: "what I want to build is a llm CLI, and it would have a subcommand like
infer, llm infer, and then we can reference some model, an ID, and there would be in my home or
default config folder some .b10x folder, then llm.toml, and in there we basically have model
routes. So it would just say here this is the endpoint, this is Haiku because this is the provider,
this is the protocol to use, aliases and so on, eventually costs, and then we can list that. So llm
model list, and then we have llm infer, and then I can say infer --model=…"

Today a person who wants to send one prompt to a configured model writes a Rust program against
`llm-models`. The workspace already has everything a turn needs: the `llm.routes/1` routes document
(`llm-models`, `docs/design.md`), the three protocol clients, credential resolvers and the price
book (`llm-cost`). The crate `crates/llm-cli` exists as an empty scaffold ("This crate exports no
runtime API yet"). This story turns it into the `llm` binary with two commands.

## Change

**Binary.** `crates/llm-cli` builds a binary named `llm`, Rust with clap derive. It is a thin layer
over `llm-models`: no second routing implementation, no second protocol decoder.

**Configuration file.** One TOML file, found in this order, first hit wins:

1. `--config <path>`;
2. the environment variable `LLM_CONFIG`;
3. `~/.b10x/llm.toml`;
4. `$XDG_CONFIG_HOME/b10x/llm.toml` (default `~/.config/b10x/llm.toml`).

No file found is a refusal that names all four places it looked. The TOML carries the same model as
`llm.routes/1`, parsed into the existing `llm-models` routes types, with the same strict refusal by
field path for an unknown or invalid key. It is not a new schema. Each route has an id, optional
aliases, the endpoint (base URL), the provider/account, the protocol (`responses`, `messages`,
`chat-completions`), the upstream model name, capabilities and a credential reference (env, file or
keychain, never a literal secret). Prices are optional, read through the existing `llm-cost` price
book shape. Example:

```toml
format = "llm.routes/1"

[[routes]]
id = "haiku"
aliases = ["fast", "claude-haiku"]
protocol = "messages"
endpoint = "https://api.anthropic.com"
model = "claude-haiku-5-5"
credential = { env = "ANTHROPIC_API_KEY" }
```

**Bedrock routes.** The owner, 2026-10-22: "we do need to make sure to support bedrock too, i
mean we do somehow need to be able to configure opus, sonnet, etc". So this story lifts wave
2026-10-22-w15's decision D2 (routes refuse `bedrock-converse`): a route may name Amazon Bedrock and
`llm-models` builds the `llm-bedrock` `BedrockClient` for it, as it builds the other three clients.
A Bedrock route carries:
- `region`;
- the model or inference-profile id;
- the wire: `bedrock-converse`, or `messages` over Anthropic `InvokeModel`;
- an optional endpoint override;
- the credential source. That is either static keys as three secret references (access key id,
  secret key, optional session token), or the container credentials endpoint (EKS Pod Identity,
  read from `AWS_CONTAINER_CREDENTIALS_FULL_URI` and `AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE`).

One file then configures Opus, Sonnet and Haiku side by side:

```toml
[[routes]]
id = "opus"
aliases = ["claude-opus"]
provider = "bedrock"
protocol = "bedrock-converse"
region = "eu-central-1"
model = "eu.anthropic.claude-opus-5-5"
credential = { container = true }

[[routes]]
id = "sonnet"
provider = "bedrock"
protocol = "messages"
region = "eu-central-1"
model = "eu.anthropic.claude-sonnet-5-5"
credential = { access_key_id = { env = "AWS_ACCESS_KEY_ID" }, secret_access_key = { env = "AWS_SECRET_ACCESS_KEY" } }
```

The inference-profile ids are examples, not verified against a Bedrock account. The field names
above are illustrative: the TOML keys are the `llm.routes/1` keys, whatever they
are, so one document can be converted between YAML and TOML without loss.

**`llm model list`.** One row per route: id, aliases, protocol, provider, endpoint host, upstream
model, and price per million input/output tokens or `unknown`. Plain columns by default,
`--format json` for scripts. It reads no secret and makes no network call.

**`llm infer`.** `llm infer --model=<id-or-alias> [PROMPT]` sends one user turn and streams the
assistant text to stdout as it arrives.
- The prompt comes from the argument or from stdin when the argument is absent.
- `--system <text>` sets the system prompt.
- `--max-output-tokens <n>` caps the answer.
- After the turn, one line to stderr: route id, protocol, input and output tokens (`unknown` where
  the provider did not report a count, never 0), and the cost when the route has prices.
- An alias that names two routes, an unknown id, or a credential that does not resolve is refused
  before any request is sent, with exit code 2 and a message naming the id or alias.
- A provider failure exits 1 with the `llm_core::Error` kind and no provider prose or secret.
- Ctrl-C cancels the turn through `Cancel`.

**Documentation.** A site guide "Use the llm command", README.md, `docs/implementation-status.md`,
CHANGELOG.md and AGENTS.md (the binary in the owns list).

## Out of scope

- A live Bedrock call in the gate; Bedrock is checked against fixtures only.
- Multi-turn chat, tool calls, files and images.
- Writing or editing the configuration file from the CLI.
- Any gateway or serving command; that is beyond10x/llm-gateway.

## Acceptance

- `llm model list` on a fixture `llm.toml` with three routes, one aliased and one priced, prints
  those three rows with the alias and the price, and `unknown` for the unpriced route.
- `llm infer --model=<alias> "hi"` against a local fixture server for each of the three protocols
  prints the fixture's text deltas to stdout in order and the usage line to stderr. An unreported
  counter prints `unknown`.
- The four lookup places are tried in the order above (one test per place), and a missing file is
  refused with all four named.
- A file with an Opus route on `bedrock-converse` and a Sonnet route on `messages` over
  `InvokeModel` lists both in `llm model list`. `llm infer --model=opus` and `--model=sonnet` each
  against a local fixture server stream that route's text. The fixture server sees a SigV4-signed
  request to the route's region and model path.
- A Bedrock route with no region, with both credential sources, or with a literal key is refused by
  its field path.
- A TOML file and the YAML `llm.routes/1` document with the same content give the same catalog
  digest.
- An unknown model or alias, an alias claimed by two routes, a literal secret in the file and an
  unresolved credential are each refused with exit code 2 before the fixture server sees a request.
- No test makes a paid call.
