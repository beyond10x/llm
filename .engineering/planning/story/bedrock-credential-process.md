---
format: aep.planning-md/3
id: story:bedrock-credential-process
kind: story
status: implemented
title: A Bedrock endpoint gets its keys from a credential_process command, held in memory
summary: ProcessCredentials in llm-bedrock behind feature process; aws-process auth kind in llm.routes/1; llm-cli opts in
relations:
- decomposes: epic:bedrock
- serves: vision:portable-model-inference
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-10-23T09:48:06Z", actor: "human:timo", revision: 2, decided_on: {"recorded":{"test_result":1}}, executor: "agent:llm-session"}
- {from: "proposed", to: "active", at: "2026-10-23T09:48:06Z", actor: "human:timo", revision: 3, decided_on: {"recorded":{"test_result":1}}, executor: "agent:llm-session"}
- {from: "active", to: "implemented", at: "2026-10-23T09:48:06Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":1}}, executor: "agent:llm-session"}
---
## Context

The `llm` command reaches Bedrock with `aws-keys` (three references, typically `env:`) or
`aws-container` (EKS Pod Identity). A person signed in through AWS IAM Identity Center (SSO) has
neither: the keys live in the AWS CLI's cache and expire after about an hour. Exporting them into
the shell environment to fill `aws-keys` leaves them in every child process and in shell history.
The owner asked on 2026-10-23 for "a secret command" in the TOML file's Bedrock section that `llm`
calls itself, "this way it remains in memory only".

The AWS SDKs solve the same problem with `credential_process`: a command that prints one JSON
object (`Version: 1`, `AccessKeyId`, `SecretAccessKey`, optional `SessionToken`, optional
`Expiration`). The AWS CLI prints exactly that object by default from
`aws configure export-credentials --profile <name>` (`aws configure export-credentials help`,
aws-cli 2.34.32).

## Change

**`ProcessCredentials` in `llm-bedrock`.** A `CredentialSource` that runs a command given as a
program and its arguments (no shell, no expansion), with stdin closed, stdout read to at most
64 KiB, stderr passed through to the caller's stderr, and a 60-second limit after which the child
is killed. It parses the `credential_process` object, refuses a `Version` other than 1, a missing
or empty key, a non-UTF-8 or oversized answer, a non-zero exit and an `Expiration` that is not
RFC 3339, and keeps the answer in memory. An answer with `Expiration` is refreshed the way
`ContainerCredentials` refreshes (unexpired credentials answered at once, one refresh at a time
inside the window, callers wait only once nothing unexpired is kept); one without it is kept for
the life of the source. No error, `Debug` output or log carries the command's stdout or a key.
It sits behind a new `llm-bedrock` feature `process` (Tokio's `process` feature), off by default.

**`aws-process` in `llm.routes/1`.** A Bedrock endpoint may write
`auth = { kind = "aws-process", command = ["aws", "configure", "export-credentials", "--profile", "dev"] }`.
`command` is a non-empty array of strings whose first element is non-empty; no other `auth` field
is allowed beside it. The document stays secret-free: the command is configuration, its output is
the secret. `llm-models` builds `ProcessCredentials` for such an endpoint only with its new feature
`process` and only when the caller has allowed command execution on its `RouteResolvers`;
otherwise the endpoint's routes are refused as `unauthorized`, naming the endpoint, before any
command runs. `llm-cli` enables both, so the `llm` command runs the command for a route that needs
it, and only then. `llm model list` shows the endpoint and runs nothing.

## Out of scope

- A generic `command:` secret reference for other fields (API keys, bearer tokens).
- Reading `credential_process` or SSO caches from `~/.aws/config` directly.
- A live Bedrock call in the gate; the command in the tests is the test binary itself.

## Acceptance

- A routes document with an `aws-process` Bedrock endpoint loads from TOML and YAML; an empty
  `command`, an empty program, a non-string element and an extra `auth` field are each refused by
  their path; the catalog and `llm model list` show the endpoint without running the command.
- A turn on an `aws-process` route runs the command once, signs with the keys it printed, and the
  fixture endpoint receives a request signed with them; a second turn before the refresh window
  runs no command.
- An answer whose `Expiration` is inside the refresh window is answered at once and refreshed by
  exactly one further run; an expired answer makes the caller wait for that run.
- Each malformed answer (wrong `Version`, a missing key, invalid JSON, more than 64 KiB, a non-zero
  exit, a command that does not exit within the limit) is an `unauthorized` error that names the
  endpoint and the program, carries none of the command's output, and sends no request.
- Without the caller's permission, or without feature `process`, the route is refused as
  `unauthorized` and no command runs.
- `RUSTUP_TOOLCHAIN=1.98.0 task check` passes.
