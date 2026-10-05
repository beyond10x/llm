# Live qualification: one subscription turn

story:anthropic-access needs one live Messages turn over the operator's own subscription token,
recorded as evidence. The example `live_subscription_turn` in `crates/llm-credentials` makes that
turn and prints a JSON report. No gate runs it: it makes one real call on the operator's
subscription.

**This route is for the operator's own subscription only.** llm offers no login, never reads Claude
Code's credential files and serves no other user's token (approval record
`access-decisions-2026-10-05`, with the quoted terms). The token is obtained with Anthropic's own
flow, stored by the operator in the platform keychain through the secrets library, and read back
from there by the example.

## 1. Install `secretsctl` with the native keychain

```console
cargo install --locked --git https://github.com/beyond10x/secrets --branch main secretsctl --features native-keychain
```

Without `--features native-keychain` the keychain is unavailable and `secretsctl` says so. On Linux
the keychain is the Secret Service (GNOME Keyring or KWallet), which must be running and unlocked;
on macOS it is the login keychain and on Windows the Credential Manager.

## 2. Obtain the subscription token

```console
claude setup-token
```

This is Claude Code's own command. It signs in through the browser and prints a long-lived token
for the subscription. Copy it; do not save it to a file.

## 3. Store the token

```console
secretsctl namespace add llm
secretsctl put anthropic-subscription --namespace llm
```

`namespace add` is needed once; the namespace goes on the default keychain mount. `put` asks for
the value at a hidden prompt (`Value for anthropic-subscription:`): paste the token and press Enter.
It prints the scope `default/llm/default`, the backend `keychain/default` and the new version,
never the value. `secretsctl describe anthropic-subscription --namespace llm` shows the version
again.

These are the example's defaults (`--namespace llm`, `--name anthropic-subscription`). Under other
names, pass the same values to the example.

## 4. Run one turn

From a checkout of `beyond10x/llm`:

```console
cargo run --locked -p b10x-llm-credentials --features secrets,native-keychain --example live_subscription_turn > live-subscription-report.json
```

The example resolves the token through the secrets library's keychain backend, sends one turn to
`https://api.anthropic.com/v1/messages` with the default model `claude-haiku-4-5` and a fixed short
prompt, and writes the JSON report to stdout, here `live-subscription-report.json`. Prompts and
notes go to stderr. Exit status 0 means the turn completed.

Options: `--model`, `--prompt`, `--endpoint` (an origin; the request goes to
`<endpoint>/v1/messages`), `--namespace`, `--name`, `--rotate-check`. `--help` lists them.

When a turn is refused, `--capture-response <path>` keeps what the route sent back so the refusal
can be read against it: the response line, the response headers and the event stream exactly as it
arrived, written to a new file (mode 0600 on Unix; an existing file is refused before any request).
Nothing the client sent is written: no request header and no token. The capture is the
operator's own response data and is not evidence to hand on; the report's `capture` field gives
only the bytes written and whether every write succeeded. A refusal of a field outside the
declared subset names the field by its path, such as
`Messages field is outside the declared subset: message_start.message.usage.<name>`.

The report holds:

| Field | What it says |
| --- | --- |
| `endpoint` | The exact request URL |
| `auth_kind`, `billing_kind` | `subscription-oauth` and `subscription`, from the catalog the example builds |
| `secret` | Where the token was read: backend, keychain service `b10x-secrets`, scope and name. Never the value |
| `request_headers` | The header names sent, never their values |
| `oauth_beta_sent` | Whether `anthropic-beta: oauth-2025-04-20` was sent |
| `anthropic_version` | The `anthropic-version` sent |
| `system_opens_with_preamble` | Whether `system` opened with the client preamble block |
| `model` | The upstream model requested |
| `not_exercised` | What this request does not use, so the report does not qualify it |
| `turns` | Per turn: `outcome`, `latency_ms`, `stop_reason`, `usage`, `upstream_model`, `response_id`, `reply`, or the typed `error` |
| `refused` | A refusal before any turn: the keychain could not be opened, or an argument is invalid |
| `rotation` | With `--rotate-check`: how many resolutions there were and whether the version changed |
| `capture` | With `--capture-response`: `bytes` written and whether the capture is `complete` |

A missing token is reported as a turn `error` with code `unauthorized`, dispatch `not-sent` and the
message ``secret reference `anthropic-subscription` refused: secret reference was not found``; no
request is sent.

## 5. Rotate and check

```console
cargo run --locked -p b10x-llm-credentials --features secrets,native-keychain --example live_subscription_turn -- --rotate-check > live-subscription-rotation.json
```

After the first turn the example asks, on stderr, for the token to be replaced. In another
terminal run `claude setup-token` again for a new token, then

```console
secretsctl put anthropic-subscription --namespace llm
```

and paste it. Press Enter in the first terminal. The second turn resolves the token again, and
`rotation.version_changed` is `true` when it read a new version. Every `put` writes a new version,
so the check shows the store's rotation reached the client; a second turn that completes with the
new token shows that token works. Exit status 0 means both turns completed and the version changed.

## 6. Record the evidence

Hand both report files to whoever records evidence on story:anthropic-access. They carry no
secret: the token, its version identifiers and every header value stay out of them.
