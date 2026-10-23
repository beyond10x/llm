---
title: Use the llm command
sidebar_position: 7
description: The llm command reads one llm.toml routes file, lists its models and streams one turn on any of them, Anthropic, Bedrock or a local endpoint alike.
lede: One configuration file and one command. List the models, then run a turn on any of them and see its usage and cost.
source: crates/llm-cli (src/lib.rs, tests/command.rs), crates/llm-models/src/routes.rs
---

# Use the llm command

`llm` is the command-line face of this workspace. It reads an `llm.routes/1` document, the same
one [Configure model routes](configure-model-routes.md) describes, and does two things:

- `llm model list` prints one row per route. It reads no secret and makes no network call.
- `llm infer --model <id> "<prompt>"` sends one user turn and streams the assistant text to
  stdout as it arrives.

Build it from a checkout:

```bash
cargo build --locked --release -p llm-cli
./target/release/llm --help
```

:::warning[Checked against fixtures only]

The command's tests run against loopback fixture servers. No paid provider and no Bedrock
account answered them.

:::

## The configuration file

`llm` takes the first of these that is set, and does not look further:

| Order | Place |
| --- | --- |
| 1 | `--config <path>` |
| 2 | `LLM_CONFIG` |
| 3 | `~/.b10x/llm.toml`, if it exists |
| 4 | `$XDG_CONFIG_HOME/b10x/llm.toml`, if it exists; `~/.config/b10x/llm.toml` when the variable is unset or not absolute |

A file named by `--config` or `LLM_CONFIG` must exist; `llm` refuses it rather than falling back
to another place. When no place has a file, the refusal names all four and the paths it tried. A
file ending in `.yaml` or `.yml` is read as YAML, anything else as TOML. Both forms take the same
keys and refuse the same mistakes.

This file declares one Anthropic route and two Bedrock routes:

```toml
format = "llm.routes/1"

[endpoints.anthropic]
base_url = "https://api.anthropic.com/v1"
protocol = "messages"
auth = { kind = "api-key", header = "x-api-key", secret = "env:ANTHROPIC_API_KEY" }

[endpoints.bedrock-eu]
protocol = "bedrock-converse"
bedrock = { region = "eu-central-1" }
auth = { kind = "aws-container" }

[endpoints.bedrock-invoke]
protocol = "messages"
bedrock = { region = "eu-central-1" }
auth = { kind = "aws-keys", access_key_id = "env:AWS_ACCESS_KEY_ID", secret_access_key = "env:AWS_SECRET_ACCESS_KEY", session_token = "env:AWS_SESSION_TOKEN" }

[routes.haiku]
model = "anthropic/claude-haiku-5-5"
aliases = ["fast"]
price = { currency = "USD", input = "1", cached_input = "0.1", cache_creation_input = "1.25", output = "5" }

[routes.opus]
model = "bedrock-eu/eu.anthropic.claude-opus-5-5"
aliases = ["claude-opus"]

[routes.sonnet]
model = "bedrock-invoke/eu.anthropic.claude-sonnet-5-5"
```

The model ids are example ids, not verified. `bedrock-eu` uses Bedrock's Converse wire and signs
with the container credentials EKS Pod Identity serves. `bedrock-invoke` sends the Anthropic
Messages body to `InvokeModel` and signs with keys from the environment. Every secret is a
reference, never a value; a literal key is refused. The command reads `env:`, `file:` and
`keychain:` references. A `keychain:` reference reads the platform keychain (the Secret Service
on Linux, the login keychain on macOS, the Credential Manager on Windows), which `llm` opens only
when the route you ask for needs it. An endpoint whose reference cannot be read refuses only its
own routes; the other routes still work.

## List the models

```bash
llm model list
```

```text
ID      ALIASES      PROTOCOL          PROVIDER        HOST                                        MODEL                           PRICE/1M IN/OUT
haiku   fast         messages          anthropic       api.anthropic.com                           claude-haiku-5-5                1/5 USD
opus    claude-opus  bedrock-converse  bedrock-eu      bedrock-runtime.eu-central-1.amazonaws.com  eu.anthropic.claude-opus-5-5    unknown
sonnet  -            messages          bedrock-invoke  bedrock-runtime.eu-central-1.amazonaws.com  eu.anthropic.claude-sonnet-5-5  unknown
```

`PROVIDER` is the endpoint's name. A route without a `price` shows `unknown`, never `0`.
`--format json` prints the same rows as a JSON array, with `region` for a Bedrock route and
`price_per_million` (or `null`).

## Run a turn

```bash
export ANTHROPIC_API_KEY=...
llm infer --model fast "Name one prime number."
```

`--model` takes a route id, one of its aliases, or `<endpoint>/<model-id>` for a model the file
declares no route for. `--system` sets a system prompt and `--max-output-tokens` caps the answer.
Without a prompt argument, `llm` reads the prompt from stdin.

The text goes to stdout as it streams. One line on stderr closes the turn:

```text
route=haiku protocol=messages input_tokens=12 output_tokens=4 cost=0.000032 USD
```

A counter the provider did not report is `unknown`, not `0`. `cost` appears only for a priced
route, and it is `unknown` when a counter it needs is missing. A cost needs every input counter,
cache reads and cache writes included, so whether it can complete depends on the wire:

| Wire | Cost of a priced route |
| --- | --- |
| `messages`, direct or on Bedrock `InvokeModel` | Completes when the answer reports its cache counters |
| `bedrock-converse` | Completes: `llm` sets no cache point, so an absent cache counter is a known `0` |
| `responses` | Always `unknown`: the wire reports no cache-write counter |
| `chat-completions` | `unknown` unless the server reports both cache counters, which providers on this wire do not |

For `bedrock-eu`, the container credentials come from the two variables EKS Pod Identity sets,
`AWS_CONTAINER_CREDENTIALS_FULL_URI` and `AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE`. Without them
that route is refused and every other route still works. The token file is read again on every
turn, so a rotated token is picked up. It may be the Kubernetes projected-volume layout (`token`
linking to `..data/token`, `..data` linking to a timestamped directory, the file mode `0644`).
That layout is taken from the AWS and Kubernetes documentation and has not been checked on a
cluster. `llm` follows a link only when it sits in a directory owned by you or root that no one
else can write. The file it reaches must be a regular file owned by you or root that only its
owner can write.

A Bedrock endpoint can also take its keys from a command, the way an AWS profile's
`credential_process` does. This endpoint runs the AWS CLI for profile `dev`:

```toml
[endpoints.bedrock-dev]
protocol = "bedrock-converse"
bedrock = { region = "eu-central-1" }
auth = { kind = "aws-process", command = ["aws", "configure", "export-credentials", "--profile", "dev"] }
```

`command` is the program and then its arguments; `llm` runs it without a shell. It runs at the
first `llm infer` on the endpoint's routes, and again only when the keys it printed are about to
expire; `llm model list` never runs it. The command prints the `credential_process` JSON answer
(`Version` 1, `AccessKeyId`, `SecretAccessKey`, optionally `SessionToken` and `Expiration`). The
keys stay in the `llm` process's memory and are written nowhere. A command that fails, prints
something else, prints more than 64 KiB or runs longer than 60 seconds refuses the turn with exit
code `2`, naming the endpoint and the program and printing none of the command's output. The
command's stderr is passed through, so its prompts and warnings reach you.

Whoever can write the routes file can make `llm` run a program, so a file that declares an
`aws-process` endpoint is trusted only as far as a `file:` source is. It must be a regular file
owned by you or root that neither its group nor others can write, every directory on its path must
be owned by you or root and not writable by others, and a link on that path is followed only from
such a directory. Otherwise `llm infer` and `llm model list` both refuse the file with exit code
`2`, naming it and what is wrong, before anything runs. A file without an `aws-process` endpoint is
not checked. On Windows the check is not made.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | The turn completed, or the list was printed; also when the reader closed stdout (`llm infer … \| head -c 1`), which stops the turn and says so on stderr |
| `1` | The provider failed after the request was sent, or stdout could not be written for another reason; stderr names the route and the error code only |
| `2` | Refused before anything was sent: no or bad configuration, an unknown model, a missing credential, an invalid request |
| `130` | Interrupted with Ctrl-C; the text shown so far stays on stdout. A second Ctrl-C ends a turn the first did not stop |

A refusal names the field, route or reference and never prints a secret value or a provider's
error text.

## Next

- Every key of the file: [Configure model routes](configure-model-routes.md).
- What a Bedrock turn reports: [Call a model on Bedrock](call-a-model-on-bedrock.md).
