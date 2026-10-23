# Amazon Bedrock

`llm-bedrock` runs a neutral turn on Amazon Bedrock Runtime behind `Model::turn`. A caller builds
`BedrockClient` directly from a `BedrockTarget` and a `CredentialSource`; no catalog, binding
document or `llm.routes/1` document can name Bedrock yet.

No live Bedrock call was made for this crate. Everything below is checked against vendored test
data and loopback fixture servers (`crates/llm-bedrock/tests/data/`, sources and licences in its
README.md) and the `llm.bedrock` ESS domain. It is not a qualification.

## Two wires

| Wire | Streamed | Whole answer | Provenance protocol | Models |
| --- | --- | --- | --- | --- |
| Converse | `POST /model/{model}/converse-stream` | `POST /model/{model}/converse` | `bedrock-converse` | any model Converse serves |
| Anthropic `InvokeModel` | `POST /model/{model}/invoke-with-response-stream` | `POST /model/{model}/invoke` | `messages` | Anthropic models |

The host is `https://bedrock-runtime.{region}.amazonaws.com`, or the target's `endpoint` origin (a
VPC endpoint, a fixture); the request is signed for the target's region either way. An `endpoint`
is `https`, or `http` to a loopback address (`127.0.0.0/8`, `[::1]`) for a local fixture; a host
name is not an address, `localhost` included, and anything else is refused as `invalid-request`.
The model id,
inference profile id or ARN is one percent-encoded path segment. Only `-`, `.`, `_`, `~`, letters
and digits stay as they are, so `:` is sent as `%3A`, as the recorded request URIs in
`tests/data/frames/uris.txt` show, and an ARN's `/` as `%2F`. No recording has an ARN.

`BedrockTarget::stream` must equal `capabilities.output_streaming`, and the provenance protocol
must be the wire's. `input_during_output` is refused: neither wire carries it. A Converse target
must declare no `capabilities.reasoning_efforts`: Converse carries no reasoning effort.

The Converse body carries `messages`, `system` (only when the turn has instructions),
`inferenceConfig` (only the sampling fields the turn sets), `toolConfig`, and the thinking
control as `additionalModelRequestFields.thinking` for an Anthropic model id only. A reasoning
effort, the parallel tool call control, thinking for another model and opaque state from another
wire are refused by name before any I/O. Advisory hints are never sent, `anthropic-beta`
included; a successful turn that carried any ends with one `advisory-omitted` warning. Converse reports no response id and no
upstream model, so both stay absent.

The Anthropic body is the Messages body `llm-messages` encodes, plus
`anthropic_version: "bedrock-2023-05-31"`, minus `model` (the path names it) and `stream` (the
operation says it). A turn's `anthropic-beta` names, which the Messages route takes as a header,
travel as the body's `anthropic_beta` array, and no `anthropic-beta` header is sent. Its stream is
`chunk` frames, each `{"bytes": base64(<Messages event>)}`, read by
the Messages stream decoder. So text, tool calls, thinking, stop reasons and refusals on that wire
are the Messages wire's ([messages.md](messages.md)).

`llm_bedrock::encode_request(&request, &target)` returns the body a turn on that target sends, and
`check_request` answers only whether it would be refused. `BedrockClient` encodes through the
same function, so the check refuses exactly what a turn refuses, with the same error, before any
credential is asked for. A gateway can run it before it answers its own caller.

Streamed answers on both wires are AWS event-stream frames (`application/vnd.amazon.eventstream`):
a 12-byte prelude, typed headers, the payload and two CRC32 checksums. `llm_bedrock::eventstream`
decodes them incrementally, bounded at 16 MiB per frame and by the transport's stream bound. A
malformed frame is a final `protocol` failure with a fixed message; a frame over 16 MiB is
`too-large`. A body that ends inside a frame is a retriable `protocol` failure of unknown dispatch
from the decoder alone; inside a turn it has dispatch `accepted` and is retriable only before the
turn answered (see Retry classes).

## Signing

Every request is signed with AWS Signature Version 4 for service `bedrock` and the target's
region, at the current time. The signer (`llm_bedrock::sigv4`) is written here on `ring`
(HMAC-SHA256, SHA-256). No AWS SDK crate is a dependency, and nothing pulls `aws-lc`. A session
token is sent and signed as `x-amz-security-token`.

The signer is checked against the AWS Signature Version 4 test suite vendored from aws-c-auth
(`tests/data/sigv4/`), header signing only: canonical request, string to sign and signature per
case. Query-string signing and SigV4a are not implemented.

## Credentials

A `CredentialSource` answers `AwsCredentials` (access key id, secret access key, optional session
token) for each request, under the turn deadline. A failure there is dispatch `not-sent`. Two
sources ship:

- **`StaticCredentials`** resolves the three values through an injected `SecretResolver` on every
  request, like every other account in llm. No key is a configuration value.
- **`ContainerCredentials`** reads the container credentials endpoint that EKS Pod Identity and ECS
  task roles serve. The caller passes the endpoint URI (the value of
  `AWS_CONTAINER_CREDENTIALS_FULL_URI`) and a secret reference the resolver binds to the token
  file (`AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE`), for example through the `file` adapter. The
  source sends the token as the `authorization` header of a GET and keeps the answer until five
  minutes before its `Expiration`. Inside those five minutes a request gets the kept credentials
  at once and starts one refresh in the background if none is running; at most one fetch is in
  flight, on its own task, so a slow endpoint holds no turn. Only a request with no unexpired
  credentials kept waits, and all such requests wait for the same fetch; a request that cancels
  its wait leaves the fetch to finish. A refresh that fails keeps the unexpired credentials until
  their `Expiration` passes, and the next request after it waits for a new fetch.
  `ContainerCredentials::refreshing` says whether a fetch is in flight. Dropping the source
  cancels its fetch. One trailing line end of the token
  is removed; the rest must be non-empty printable ASCII of at most 16 KiB. The answer is decoded
  straight into its four members, the secret access key and the token into zeroizing strings,
  with no intermediate JSON document; a member named twice, or an answer that is not a JSON
  object, is refused.

  The URI must be `https`, or `http` to a loopback address, `169.254.170.2` (ECS),
  `169.254.170.23` or `[fd00:ec2::23]` (EKS Pod Identity). A host name is not an address,
  `localhost` included. Any other URI is refused before a connection.

  | Container failure | Class |
  | --- | --- |
  | Refused URI, token that does not resolve or is malformed, 4xx answer, answer missing `AccessKeyId`, `SecretAccessKey`, `Token` or `Expiration` or naming one twice, answer already expired | `unauthorized`, final |
  | Endpoint that does not answer, cuts its answer, or answers 408 or 5xx | `transport`, retriable, its `Retry-After` kept |
  | Endpoint that answers after the transport's deadline | `deadline`, retriable |

  Every container failure is dispatch `not-sent`, and no message quotes the URI, the token or the
  answer.

Not supported: shared credential and config files, profiles, instance metadata (IMDS), web
identity (IRSA), SSO and `credential_process`. A caller that has one of them resolves the keys
itself and hands them to `StaticCredentials` through a resolver.

Secret values never appear in a `Debug` form or an error message.

## EKS Pod Identity (unverified)

This section restates AWS documentation. It was not run on a cluster.

When a pod's service account has a Pod Identity association, the cluster adds two environment
variables and a projected token volume to the pod
([How EKS Pod Identity works](https://docs.aws.amazon.com/eks/latest/userguide/pod-id-how-it-works.html)):

| Variable | Value |
| --- | --- |
| `AWS_CONTAINER_CREDENTIALS_FULL_URI` | `http://169.254.170.23/v1/credentials` |
| `AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE` | `/var/run/secrets/pods.eks.amazonaws.com/serviceaccount/eks-pod-identity-token` |

The token is a projected service account token with audience `pods.eks.amazonaws.com` and a
24-hour expiry, read again from the file on each resolution.

Other facts from the same documentation:

- The EKS Pod Identity Agent runs as a DaemonSet on each node's host network, on ports 80 and 2703
  of `169.254.170.23` and `[fd00:ec2::23]`. A pod behind a proxy adds both addresses to
  `no_proxy` ([EKS Pod Identities](https://docs.aws.amazon.com/eks/latest/userguide/pod-identities.html)).
  llm never uses an ambient proxy, so that only matters for other clients in the pod.
- It serves Linux EC2 nodes only; Fargate and Windows nodes are not supported (same page).
- The agent is the `eks-pod-identity-agent` add-on, and the node role needs
  `eks-auth:AssumeRoleForPodIdentity`
  ([Set up the Amazon EKS Pod Identity Agent](https://docs.aws.amazon.com/eks/latest/userguide/pod-id-agent-setup.html)).
- `kubectl describe pod` shows whether `AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE` is set
  ([Configure pods](https://docs.aws.amazon.com/eks/latest/userguide/pod-id-configure-pods.html)).

Not known: whether `automountServiceAccountToken: false` on the pod or its service account removes
the Pod Identity token volume is unknown. None of the pages above mentions it.

## Usage

Unknown counters stay unknown; none becomes zero. The one exception is a Converse cache counter,
which is a known 0 when the answer leaves it out.

- **Converse** reports `inputTokens` (uncached input), `outputTokens`, `totalTokens` and, when
  caching is used, `cacheReadInputTokens` and `cacheWriteInputTokens`. The neutral `input_tokens`
  is the sum of the three input counters. The Converse body `llm-bedrock` sends never carries a
  `cachePoint`, so Bedrock neither reads nor writes the prompt cache on its turns: a cache
  counter the answer leaves out is a known 0, and `input_tokens` is `inputTokens` plus the
  cache counters the answer does report. An answer without `inputTokens` leaves `input_tokens`
  unknown. `totalTokens` is checked and never used to fill a counter in. The test
  `the_converse_body_never_carries_a_cache_point` fails once the encoder writes a `cachePoint`,
  so sending cache points means revisiting this rule.
- **Anthropic `InvokeModel`** reports the Messages usage. Bedrock adds
  `amazon-bedrock-invocationMetrics` to `message_stop`; its counts fill only a counter the Messages
  usage never reported, and never replace one it did. The input total still needs both cache
  counters, so a stream that reports neither leaves `input_tokens` unknown.

## Retry classes

`BedrockClient` makes one attempt; retry and fallback belong to routing.

| Failure | Before the turn answered | After |
| --- | --- | --- |
| `throttlingException` frame | `rate-limited`, retriable | final |
| `serviceUnavailableException`, `internalServerException`, `modelNotReadyException` frame | `transport`, retriable | final |
| `modelTimeoutException` frame | `transport`, retriable, as the HTTP 408 Bedrock answers a model timeout with before a stream | final |
| `validationException` frame | `refused`, final | final |
| `modelStreamErrorException`, `serviceQuotaExceededException`, `accessDeniedException`, any other exception frame | final | final |
| Connection cut inside a frame or while the body is read | retriable | final |
| Body ends cleanly between frames before the terminal events, before any event included | `protocol`, final, as in `llm-messages` | final |
| HTTP 429 with `x-amzn-errortype: ServiceQuotaExceededException` | `refused`, dispatch `rejected`, final | not applicable |
| Any other HTTP 429 | `rate-limited`, retriable | not applicable |
| Any other HTTP error status | the transport's class for the status | not applicable |
| Any failure while a whole (unstreamed) answer is read | final | not applicable |

"Answered" means any event after the message header (`messageStart` on Converse,
`message_start` on InvokeModel), shown to the caller or not. An exception frame has dispatch
`accepted`. The class is read from `:exception-type` (or `:error-code`) and `x-amzn-errortype`; no
diagnostic quotes Bedrock's own message.

## Example target

An example, not verified against a live account:

| Field | Value |
| --- | --- |
| Region | `eu-central-1` |
| Model | `eu.anthropic.claude-haiku-5-5` (example id, not verified) |
| Wire | Converse, or Anthropic `InvokeModel` for the same Anthropic model |

An inference profile id such as the one above goes where the model id goes.

## Not supported

- Catalog, binding document and `llm.routes/1` support: they refuse `bedrock-converse` by name.
- A live qualification: no turn has been sent to Bedrock.
- The credential sources listed under Credentials as not supported.
- Query-string (presigned) signing and SigV4a.
