---
title: Use the llm command
sidebar_position: 7
description: The llm command reads one llm.toml routes file, lists its models and streams one turn on any of them, Anthropic, Bedrock or a local endpoint alike.
lede: One configuration file and one command. List the models, then run a turn on any of them and see its usage and cost.
source: crates/llm-cli (src/lib.rs, tests/command.rs), crates/llm-models/src/routes.rs
---

# Use the llm command

`llm` is this workspace's command. It reads an `llm.routes/1` document, the same one
[Configure model routes](configure-model-routes.md) describes, and has three subcommands:

- `llm model list` prints one row per route. It reads no secret and makes no network call.
- `llm infer --model <id> "<prompt>"` sends one user turn and streams the assistant text to
  stdout as it arrives.
- `llm replay --route <id> --corpus <file> --cap <amount> --reserve <amount>` sends a recorded
  corpus to a priced route under a money cap and prints which answers are the same.

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

A file named by `--config` or `LLM_CONFIG` must exist. If it does not, `llm` refuses it and does
not fall back to another place. When no place has a file, the refusal names all four and the paths
it tried. A file ending in `.yaml` or `.yml` is read as YAML, anything else as TOML. Both forms
take the same keys and refuse the same mistakes.

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
Messages body to `InvokeModel` and signs with keys from the environment.
[Call a model on Bedrock](call-a-model-on-bedrock.md) shows what a Bedrock turn reports.

A secret is written as a reference, not a value. A literal key is refused. The command reads `env:`,
`file:` and `keychain:` references. A `keychain:` reference reads the platform keychain: the
Secret Service on Linux, the login keychain on macOS, the Credential Manager on Windows. `llm`
opens it only when the route you ask for needs it. An endpoint whose reference cannot be read
refuses only its own routes. The other routes still work.

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

`PROVIDER` is the endpoint's name. A route without a `price` shows `unknown`, not `0`.
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

The text goes to stdout as it streams. When the model declines, its refusal text is its answer and
goes to stdout the same way. Chat Completions sends a refusal apart from the text. A Responses
server may send a message only in its final answer, without streaming it. That message is printed
once after the streamed text. Each message starts on its own line. One line on stderr closes the
turn:

```text
route=haiku protocol=messages input_tokens=12 output_tokens=4 cost=0.000032 USD
```

A turn that stopped before its end keeps the text it produced on stdout, and a second line on stderr
says why. `llm` then exits with `3` (see [Exit codes](#exit-codes)):

```text
route=gpt protocol=responses input_tokens=20 output_tokens=3
llm: route `gpt`: the turn stopped at the output limit; the answer is cut short
```

The output limit is `--max-output-tokens` or the model's own. Any other reason the provider named,
such as a content filter, is printed as `the turn stopped before its end: <reason>`.

A counter the provider did not report is `unknown`, not `0`.

`cost` appears only for a priced route. It prices the model the provider reported serving the
turn, at the route's rates, and only when that is the route's model. When the answer names another
model or none, `cost` is `unknown`. It is also `unknown` when a counter it needs is missing. A cost
needs every input counter, cache reads and cache writes included, so whether it can complete
depends on the wire:

| Wire | Cost of a priced route |
| --- | --- |
| `messages`, direct or on Bedrock `InvokeModel` | Completes when the answer names the route's model and reports its cache counters |
| `bedrock-converse` | Always `unknown`: the answer names no model |
| `responses` | Completes when the answer names the route's model and reports its cache counters (`cached_tokens` and `cache_write_tokens`) |
| `chat-completions` | `unknown` unless the server reports both cache counters, which providers on this wire do not |

For `bedrock-eu`, the container credentials come from the two variables EKS Pod Identity sets,
`AWS_CONTAINER_CREDENTIALS_FULL_URI` and `AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE`. Without them
that route is refused and every other route still works. The token file is read again on every
turn, so a rotated token is picked up.

The token file may use the Kubernetes projected-volume layout: `token` links to `..data/token`,
`..data` links to a timestamped directory, and the file mode is `0644`. That layout is taken from
the AWS and Kubernetes documentation and has not been checked on a cluster.

`llm` follows a link only when it sits in a directory owned by you or root that no one else can
write. The file it
reaches must be a regular file owned by you or root that only its owner can write.

`llm` does not read the IAM roles for service accounts variables (`AWS_ROLE_ARN`,
`AWS_WEB_IDENTITY_TOKEN_FILE`) yet. It supplies no web identity role, so a route on an
`aws-web-identity` endpoint is refused and every other route still works.

A Bedrock endpoint can also take its keys from a command, the way an AWS profile's
`credential_process` does. This endpoint runs the AWS CLI for profile `dev`:

```toml
[endpoints.bedrock-dev]
protocol = "bedrock-converse"
bedrock = { region = "eu-central-1" }
auth = { kind = "aws-process", command = ["aws", "configure", "export-credentials", "--profile", "dev"] }
```

`command` is the program and then its arguments. `llm` runs it without a shell, at the first
`llm infer` on the endpoint's routes, and again only when the keys it printed are about to expire.
`llm model list` does not run it.

The command prints the `credential_process` JSON answer: `Version` 1, `AccessKeyId`,
`SecretAccessKey`, optionally `SessionToken` and `Expiration`. The keys stay in the `llm`
process's memory and are written nowhere. A command that fails, prints something else, prints more
than 64 KiB or runs longer than 60 seconds refuses the turn with exit code `2`. The message names
the endpoint and the program and prints none of the command's output. The command's stderr is
passed through, so its prompts and warnings reach you.

Whoever can write the routes file can make `llm` run a program. A file that declares an
`aws-process` endpoint is therefore trusted only as far as a `file:` source is:

- it must be a regular file owned by you or root that neither its group nor others can write;
- every directory on its path must be owned by you or root and not writable by others;
- a link on that path is followed only from such a directory.

Otherwise `llm infer` and `llm model list` both refuse the file with exit code `2` before anything
runs, and the message names the file and what is wrong. A file without an `aws-process` endpoint is
not checked. On Windows the check is not made.

## Replay a recorded corpus

`llm replay` sends recorded requests to a candidate route and says, case by case, whether the
route answers what was recorded. It needs a route with a `price`: that price is the run's price
book and names its currency.

```bash
llm replay --route fast --corpus corpus.json --cap 0.05 --reserve 0.001
```

The corpus is a JSON file you assemble from your own store; `llm` writes none. It is one
`llm.corpus/1` document with an ordered list of cases:

```json
{
  "format": "llm.corpus/1",
  "cases": [
    {
      "id": "greeting",
      "kind": "turn",
      "input_tokens": 100,
      "turn": {
        "format": "llm.turn/3",
        "request": {
          "model": "recorded-route",
          "instructions": "",
          "items": [{ "kind": "user-text", "text": "Say good day in German." }],
          "tools": []
        }
      },
      "recorded": { "text": "Guten Tag" }
    }
  ]
}
```

A case of kind `turn` carries a turn document and the recorded text. Kind `classification`
carries `label_set` (`id`, `version`, `labels`), optional `instructions`, `input` and the recorded
`label`. Kind `verdict` carries `rubric` (`id`, `version`, `text`, `schema`), `subject` and the
recorded `verdict`. `input_tokens` is your estimate, which route selection reads, and
`primary_record_id` may name the record the case was taken from. The file is read only whole: a
cut file, a member given twice or a member the format does not declare refuses the run before
anything is sent, as does a file over 16 MiB or with more than 4096 cases.

A case `id` holds no `/`, and a rubric `id` has at most 177 bytes: the ids of a run's call
records are built from them, and no two calls of one run share one. A recorded verdict is taken
as given and is not checked against the rubric's schema; a case whose recorded verdict the schema
does not hold reports `differs`.

`--cap` is what admission is held to, and `--reserve` is what each case holds under the cap
until its cost is known; both are decimal amounts with at most nine fractional digits. A case is
sent only while what the run spent plus the reserve is at most the cap. The first case that does
not fit is not sent, the run stops, and that case and the rest count `not-sent`.

Set `--reserve` to the most one case can cost: the spend then stays within the cap. The cap
alone does not bound the spend. A case that costs more than the reserve can take the spend past
the cap; the run stops there, and it has then spent at most the cap, less the reserve, plus the
cost of that one case. A call whose cost is not known, because the provider reported no usage,
keeps its reserve held and stops the run at that case. `spent` then counts that reserve, which
is a hold and not a known cost.

The summary is one line of JSON on stdout, here for a corpus of two cases:

```json
{"id":"replay","candidate_route_id":"haiku","corpus_digest":"…","currency":"USD","money_cap":"0.05","reserve":"0.001","ledger_id":"replay","spent":"0.000064","stopped":null,"cases":2,"same":1,"differs":1,"failed":0,"not_sent":0,"results":[{"case_id":"greeting","result":"same","error_code":null},{"case_id":"parting","result":"differs","error_code":null}]}
```

`same` means the answer equals the recorded one: a turn's text and a label byte for byte, a
verdict as canonical JSON. `failed` carries the error code of the call that failed. `stopped` is
`limit-exceeded` when a case did not fit under the cap or the spend is past it, and
`uncertain-spend` when a cost is not known. It is set at the last case too, and `llm` then exits
with `3`. `corpus_digest` is the SHA-256 of the file's bytes. `--run <id>` names the run; the
default is `replay`. The summary and stderr carry no text of a case: a refusal of the corpus
names the member by its path, and shows `*` for every member whose name the format does not
define.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | The turn completed, the list was printed, or a replay sent every case with its spend known and within the cap, whatever the cases' results; also when the reader closed stdout (`llm infer … \| head -c 1`), which stops the turn and says so on stderr |
| `1` | The provider failed a turn after the request was sent (a replay counts the case `failed` instead), or stdout could not be written for another reason; stderr names the route and the error code only |
| `2` | Refused before anything was sent: no or bad configuration, an unknown model, a missing credential, an invalid request; for a replay also a route without a price, a corpus that is refused, a reserve above the cap |
| `3` | The turn stopped before its end: at the output limit or for another reason the provider named. Its text so far is on stdout and stderr says why; the turn is not a whole answer, so it is not `0`, and nothing failed, so it is not `1`. Also a replay that stopped: a case was not sent, the last case took the spend past the cap, or a cost is not known. The summary is on stdout, and stderr says how the run stopped and how many cases were not sent |
| `130` | Interrupted with Ctrl-C; the text shown so far stays on stdout. A second Ctrl-C ends a turn the first did not stop |

A refusal names the field, route or reference. It does not print a secret value or a provider's
error text.
