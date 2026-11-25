# Amazon Bedrock

`llm-bedrock` runs a neutral turn on Amazon Bedrock Runtime behind `Model::turn`. A caller builds
`BedrockClient` directly from a `BedrockTarget` and a `CredentialSource`, or `Routes::port` builds
it for a Bedrock endpoint of an `llm.routes/1` document; no catalog or binding document can name
Bedrock.

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
control as `additionalModelRequestFields.thinking` for an Anthropic model id only. A JSON Schema
output format is `outputConfig.textFormat` `{"type": "json_schema", "structure": {"jsonSchema":
{"schema", "name"}}}`: the schema as a JSON string, as the API reference's
`JsonSchemaDefinition` takes it, and the format's name as `jsonSchema.name` when it has one.
Bedrock always holds the answer to the schema through constrained decoding
([user guide](https://docs.aws.amazon.com/bedrock/latest/userguide/structured-output.html)), so a
format whose `strict` is `false` is refused by name, and `true` sends nothing more; llm does not
validate the answer ([design](design.md#structured-output)). A user image (story:image-input) is
an `image` block in the user message beside the user text, in item order: `{"image": {"format":
"png", "source": {"bytes": "<base64>"}}}`, the format `png`, `jpeg`, `gif` or `webp` of its media
type. Converse's image source holds bytes or an S3 location and no URL, so an image by URL is
refused by name before any I/O (`Bedrock Converse carries an image only as inline bytes, not by
URL`). A reasoning effort, the parallel tool call control, thinking for another model and opaque
state from another wire are refused by name before any I/O. Advisory hints are never sent, `anthropic-beta`
included; a successful turn that carried any ends with one `advisory-omitted` warning. Converse reports no response id and no
upstream model, so both stay absent.

The Anthropic body is the Messages body `llm-messages` encodes, plus
`anthropic_version: "bedrock-2023-05-31"`, minus `model` (the path names it) and `stream` (the
operation says it). A turn's `anthropic-beta` names, which the Messages route takes as a header,
travel as the body's `anthropic_beta` array, and no `anthropic-beta` header is sent. A client's own
`cache_control` marks arrive where it placed them and none is added (`Hints::cache_marks`,
[messages.md](messages.md)); a request without a mark gets the Messages breakpoints. An output
format is the Messages body's `output_config.format`, under the Messages rules, and a user image
the Messages `image` block with a `base64` source. Anthropic documents only base64 image sources on
Bedrock and a per-image limit of 5 MB, base64-encoded
(`https://platform.claude.com/docs/en/build-with-claude/vision.md`, lines 32-34, 60 and 1283), so
an image by URL is refused by name before any I/O (`Bedrock InvokeModel carries an image only as
inline bytes, not by URL`), and so are inline bytes whose base64 passes 5,000,000 characters
(`too-large`), the smaller reading of 5 MB; neither refusal quotes a byte or the URL. Its stream is
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
token) for each request, under the turn deadline. A failure there is dispatch `not-sent`. Four
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

- **`WebIdentityCredentials`** asks the AWS Security Token Service for a role's temporary
  credentials with `AssumeRoleWithWebIdentity`, as IAM roles for service accounts (IRSA) do. The
  caller passes the regional STS endpoint (`https://sts.<region>.amazonaws.com`), the role ARN
  (the value of `AWS_ROLE_ARN`), a session name and a secret reference the resolver binds to the
  token file (`AWS_WEB_IDENTITY_TOKEN_FILE`). Each fetch reads the token again, so a token the
  platform rewrote is used as it is now, and sends one POST without a signature: the form
  `Action=AssumeRoleWithWebIdentity&Version=2011-06-15&RoleArn=…&RoleSessionName=…&WebIdentityToken=…`,
  each value percent-encoded, with no `authorization` and no `x-amz-*` header, so the token is in
  the body and never in a URL. One trailing line end of the token is removed; the rest must be
  non-empty visible ASCII of at most 20000 bytes. The role ARN must be 20 to 2048 visible ASCII
  characters starting `arn:`, and the session name 2 to 64 letters, digits or `+=,.@_-`; either is
  refused when the source is built. The XML answer is read without a parser dependency, straight
  into the four `Credentials` members `AccessKeyId`, `SecretAccessKey`, `SessionToken` and
  `Expiration`, the secrets into zeroizing strings: a member missing, empty or named twice, a
  second `Credentials`, a comment, a document type declaration or an entity other than the five
  predefined ones and character references is refused, so no entity is ever expanded. The answer
  is kept and refreshed by the single-flight code `ContainerCredentials` uses;
  `WebIdentityCredentials::refreshing` says whether a fetch is in flight. The `Debug` form shows
  the endpoint's origin, the role ARN, the session name and the token reference.

  The endpoint must be `https`, or `http` to a loopback address for a fixture; a host name over
  `http`, `localhost` and the container endpoint addresses included, and an endpoint with user
  information, a query or a fragment, are refused before a connection. The exchange uses the
  transport's `HttpClient::post_form`, which follows no redirect, ignores ambient proxy settings
  and shows a response tap nothing.

  | Web identity failure | Class |
  | --- | --- |
  | Refused endpoint, role ARN or session name, token that does not resolve or is malformed, STS `ExpiredToken`, `InvalidIdentityToken` or any other 4xx but 408, a redirect, an answer missing a member or naming one twice, an answer already expired | `unauthorized`, final |
  | Endpoint that does not answer, cuts its answer, or answers 408 or 5xx; STS `IDPCommunicationError` | `transport`, retriable, a 5xx's `Retry-After` kept |
  | Endpoint that answers after the transport's deadline | `deadline`, retriable |

  Every web identity failure is dispatch `not-sent`. A message may name an error code the STS
  reference documents; it never quotes the endpoint, the token, STS's message or the answer.

- **`ProcessCredentials`**, behind feature `process`, runs a program with its arguments the way an
  AWS profile's `credential_process` does: no shell, stdin empty, stderr inherited, stdout read up
  to 64 KiB into a zeroizing buffer, and the child killed after 60 seconds or when the source or
  its run is dropped. The answer is one JSON object with `Version` 1, a non-empty `AccessKeyId`
  and `SecretAccessKey`, optionally a `SessionToken` (`null` is absent) and an RFC 3339
  `Expiration` in any offset; other members are ignored and a member named twice is refused. It
  is decoded straight into zeroizing strings. An answer without `Expiration` is kept for the
  source's life; one with it is refreshed exactly as `ContainerCredentials` refreshes, by the same
  single-flight code, so at most one run is in flight. A command that cannot start, exits non-zero,
  times out, prints more than 64 KiB or prints an unusable answer is `unauthorized`, final,
  dispatch `not-sent`; the message names the program and a fixed reason, never the output.
  `ProcessCredentials::refreshing` says whether a run is in flight. The `Debug` form shows the
  program, the argument count and the time limit.

Not supported: shared credential and config files, profiles, instance metadata (IMDS) and SSO.
A caller that has one of them names the command that resolves the keys
(`aws configure export-credentials --profile dev`), or resolves them itself and hands them to
`StaticCredentials` through a resolver.

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

## IAM roles for service accounts (unverified)

This section restates AWS documentation. It was not run on a cluster, and no request has reached
STS.

When a pod's service account is annotated with an IAM role, the pod gets `AWS_ROLE_ARN` and
`AWS_WEB_IDENTITY_TOKEN_FILE`, the latter naming
`/var/run/secrets/eks.amazonaws.com/serviceaccount/token`, a projected service account token the
kubelet refreshes
([Configure Pods to use a Kubernetes service account](https://docs.aws.amazon.com/eks/latest/userguide/pod-configuration.html)).
A caller passes `AWS_ROLE_ARN` as the role, binds a secret reference to the token file (for
example with the `file` adapter's `FileResolver::mounted`, as for Pod Identity) and names its
region's STS endpoint.

`AssumeRoleWithWebIdentity` "does not require the use of AWS security credentials": it is called
with `RoleArn`, `RoleSessionName` and `WebIdentityToken` and answers XML `Credentials` with
`AccessKeyId`, `SecretAccessKey`, `SessionToken` and `Expiration`
([AssumeRoleWithWebIdentity](https://docs.aws.amazon.com/STS/latest/APIReference/API_AssumeRoleWithWebIdentity.html)).
The same reference states the limits `WebIdentityCredentials` checks before sending: a role ARN of
20 to 2048 characters, a session name of 2 to 64 characters from `[\w+=,.@-]`, a token of at most
20000 characters. It lists `IDPCommunicationError` as the identity provider not answering STS,
which `WebIdentityCredentials` reports as retriable; every other error it lists is final here.

A routes document names this source as `auth.kind = "aws-web-identity"` on a Bedrock endpoint, in
YAML or TOML, and nothing else: the role ARN, the session name, the STS endpoint and the token
file are the caller's, given to `SecretSources::with_web_identity` as a `WebIdentityRole` and a
path, as an `aws-container` endpoint's URI and token file are given to
`SecretSources::with_container_credentials`. The token file resolves through
`FileResolver::mounted`, read at each fetch. Without them, or with a value `WebIdentityCredentials`
refuses, only that endpoint's routes are refused as `unauthorized`. The `llm` command supplies no
web identity role yet.

## Usage

Unknown counters stay unknown; none becomes zero. llm counts no image tokens itself: an image is
in whatever input the answer reports. The one exception is a Converse cache counter, which is a
known 0 when the answer leaves it out.

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

`BedrockClient` makes one attempt a turn and retries nothing itself. No code in this repository
retries its failure or falls back from it, on either wire: a Bedrock route has no catalog entry
(`Routes::binding` refuses it), `Routes::port` hands out the bare client, and the `llm` command
calls `turn` once (`llm infer`) or refuses the route before any request (`llm replay`). A caller
that retries a failure marked retriable does so in its own loop, bounds it and may be billed for
each attempt. Routing's loop (`Catalog::run_turn`) runs a model only under a catalog target whose
binding has the model's provenance. No catalog names `bedrock-converse`, so it refuses a Converse
client ([Who retries](#a-converse-answer-that-stops-without-content)). An `InvokeModel` client's
protocol is `messages`, which a catalog target can name: routing retries and falls back for one
only when the caller declares such a target and hands the client to `run_turn` itself.

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
| A streamed `InvokeModel` answer that stops without content for a reason `llm-messages` does not model ([messages.md](messages.md#what-the-decoder-requires-of-a-stream)) | `unavailable`, retriable although its terminal `message_delta` answered: that delta follows no content and the caller was shown nothing, so this one failure after an answer may be retried. `too-large` for `model_context_window_exceeded` and `refused` for `refusal`, both final | not applicable: with content it is an incomplete turn |
| A streamed Converse answer that stops with no content block and no kept event for a reason the Converse decoder does not model: any reason but `end_turn`, `tool_use` and `max_tokens` | `unavailable`, retriable: only `messageStop` and `metadata` follow `messageStart`, so the answer held no content block and no kept event and the caller was shown nothing. The failure carries the usage the answer reported as final, which may name output tokens, so each attempt of a caller that retries may be billed. `too-large` for `model_context_window_exceeded`; `refused`, with no cause named, for `guardrail_intervened` and `content_filtered`. These three are final | not applicable: after a content block, an empty text block included, or a kept event it is an incomplete turn |
| Any failure while a whole (unstreamed) answer is read or decoded, an `InvokeModel` or a Converse answer that stops without content included | final | not applicable |

"Answered" means any event after the message header (`messageStart` on Converse,
`message_start` on InvokeModel), shown to the caller or not. An exception frame has dispatch
`accepted`. The class is read from `:exception-type` (or `:error-code`) and `x-amzn-errortype`; no
diagnostic quotes Bedrock's own message.

### A Converse answer that stops without content

`story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason`. The Converse decoder models
`end_turn`, `tool_use` and `max_tokens`. A stream and a whole answer end in one function
(`Header::outcome` in `crates/llm-bedrock/src/converse.rs`), so the rule is the same for both:

| Stop reason, no content block and no kept event | Error code | Retried |
| --- | --- | --- |
| `model_context_window_exceeded` | `too-large` | never |
| `guardrail_intervened`, `content_filtered` | `refused`, no `RefusalCause` named | never |
| every other reason not modelled (`stop_sequence`, a name in no list) | `unavailable`, marked retriable | a streamed answer only; a whole answer is final |

The rule counts as the Messages decoder counts ([messages.md](messages.md#what-the-decoder-requires-of-a-stream)):
it applies only to an answer that held no content block and no kept event. On the stream that
is `messageStart`, then `messageStop` and `metadata`, with no content block event and no event
of an unknown type; on a whole answer it is an empty `content`. A content block ends the rule
even when it leaves no item. A text block that carried no text is one: it took a delta without
text, or it only stopped, which is how this wire may end a text block that took no delta. The
attempt has started answering then, and the answer is the incomplete turn it was before this
rule: `StopReason::Incomplete` under the reason's own name, with no item for that block and
nothing shown. The Messages decoder keeps such a block as an empty text item and the Converse
decoder keeps none; both return the incomplete turn. Every other closed block leaves an item:
text, a tool call, a reasoning block even without text, and a block or an event kept whole as
opaque state. A `contentBlockStart` whose `start` is not a kind the decoder knows (it knows
`toolUse` only, so `{"text": ""}` is not one) opens such an unknown block, which is a content
block for the rule: it is kept as an opaque item, the caller is shown `unknown-output-item`, and
the answer is the incomplete turn with that item. `end_turn` and `max_tokens` without content are
the outcome they were, and `tool_use` with no call the final `protocol` refusal it was. A block
left open at `messageStop`, and a reason that is not 1 to 64 visible ASCII bytes (a space is not
one), stay the final `protocol` refusal.

The failure has dispatch `accepted` and carries the usage the answer reported as final, whether
`metadata` came before `messageStop` or after it. No row reads a counter: that usage may name
output tokens, on the retriable row too, so each attempt of a caller that retries may be billed;
the Messages decoder marks the same answer retriable after a header that reported a cost
(`crates/llm-messages/tests/retry_class.rs::adversary_w30_a_stop_without_content_after_a_costed_header_is_retried_as_an_overloaded_error_is`).
A counter the answer left out stays unknown; a
Converse cache counter the answer left out is the known 0 it is on every Converse answer
([Usage](#usage)). An answer that reports no counter at all carries no usage. Its text is fixed
for each row and carries no byte of the reason and nothing of a guardrail trace. Any other
failure after a `messageStop` that follows no content is final as the table above says. A hint
the wire omits adds no `advisory-omitted` warning to a turn that fails: the caller is shown
nothing.

| Sentence | Test |
| --- | --- |
| Each row on the stream, fed whole, in parts and one byte at a time; a reason of 1 and of 64 bytes | `tests/converse.rs::every_unmodelled_reason_without_a_content_block_fails_the_stream_as_its_row_however_it_is_fed` |
| Each row on a whole answer, final | `tests/converse.rs::every_unmodelled_reason_without_a_content_block_fails_a_whole_answer_as_its_row_and_final` |
| With an item, of every kind, it is the incomplete turn | `tests/converse.rs::every_unmodelled_reason_with_an_item_is_still_the_incomplete_turn` |
| After an empty text block it is the incomplete turn with no item | `tests/converse.rs::after_an_empty_text_block_the_turn_is_incomplete_with_no_item_and_shows_nothing` |
| A start that names `text` opens an unknown block: the incomplete turn with its opaque item, and `unknown-output-item` shown | `tests/converse.rs::a_start_that_names_text_opens_an_unknown_block_and_the_turn_is_incomplete_with_its_item` |
| The retriable row whatever output tokens the answer reports, carried as final usage | `tests/converse.rs::a_stop_without_content_is_retriable_whatever_output_it_reports_and_carries_it_as_final` |
| A recorded empty text block, then the stop, is not marked retriable | `tests/client.rs::an_unlisted_stop_after_a_recorded_silent_text_block_is_not_marked_retriable` |
| The same answer reads alike on Converse and `InvokeModel` | `tests/client.rs::a_stop_after_an_empty_text_block_reads_alike_on_both_wires` |
| Usage left out is unknown, a cache counter left out a known 0 | `tests/converse.rs::usage_on_the_failure_is_unknown_where_the_answer_left_it_out_and_a_cache_counter_a_known_zero` |
| `metadata` before or after the stop | `tests/converse.rs::metadata_before_the_stop_gives_the_same_failure_as_metadata_after_it` |
| The three modelled reasons | `tests/converse.rs::a_modelled_reason_without_an_item_is_what_it_was` |
| What stays the `protocol` refusal | `tests/converse.rs::a_block_left_open_or_a_reason_that_is_no_bounded_name_is_still_the_protocol_refusal` |
| The retriable mark reaches the caller of a streamed turn | `tests/client.rs::a_converse_stream_that_stops_without_content_for_an_unmodelled_reason_fails_as_its_row` |
| A whole answer is final through the client | `tests/client.rs::a_whole_converse_answer_that_stops_without_content_for_an_unmodelled_reason_fails_as_its_row` |
| Any other failure after that stop is final | `tests/client.rs::any_other_failure_after_a_converse_stop_without_content_is_final` |
| No cause named, nothing of the trace quoted | `tests/client.rs::a_guardrail_stop_without_content_names_no_cause_and_quotes_none_of_its_trace` |
| A failed turn shows no advisory for a hint it omitted | `tests/client.rs::a_converse_turn_that_fails_shows_no_advisory_for_a_hint_it_omitted` |

The conformance suite holds the rule in ten scenarios of `contracts/bedrock/scenarios/`, named
`turn-a-converse-stream-*stopped-*` (seven) and `turn-a-whole-converse-answer-stopped-*` (three).
Two of them hold one condition of `Header::outcome` each: a whole answer whose `content` is one
text block without text is the incomplete turn with no item
(`turn-a-whole-converse-answer-stopped-for-an-unmodelled-reason-after-an-empty-text-block-is-incomplete.yaml`),
and a stream that held no content block and kept an event of an unknown type is the incomplete
turn with that event as its item
(`turn-a-converse-stream-stopped-for-an-unmodelled-reason-after-a-kept-event-is-incomplete.yaml`).

Who retries. `BedrockClient` asks once a turn and hands the retriable failure to its caller
(`crates/llm-models/tests/relayed_failure.rs::the_converse_client_asks_once_a_turn_and_routing_takes_it_under_no_catalog_target`).
No code in this repository puts a Converse client under the retry policy: a Bedrock route has no
catalog entry, no catalog names `bedrock-converse`, `Routes::port` hands out the bare client and
the `llm` command calls `turn` once. Routing's loop (`Catalog::run_turn`) refuses a Converse
client given to it under a catalog target before any request (the same test). A caller that
retries bounds the retry itself and pays for each attempt: the stop without content carries the
usage its attempt reported, output tokens included when the answer names them.

A streamed `InvokeModel` answer holds a reported `usage.cost` as `llm-messages` does
([messages.md](messages.md#what-the-decoder-requires-of-a-stream)): a failure after the stream
answered, other than the stop without content, shows what was held before it is returned
(`tests/client.rs::an_invoke_stream_that_fails_after_a_held_cost_answered_shows_what_was_held`),
and the stop without content shows nothing
(`tests/client.rs::an_invoke_stream_that_stops_without_content_for_an_unmodelled_reason_fails_as_its_row`).

## Example target

An example, not verified against a live account:

| Field | Value |
| --- | --- |
| Region | `eu-central-1` |
| Model | `eu.anthropic.claude-haiku-5-5` (example id, not verified) |
| Wire | Converse, or Anthropic `InvokeModel` for the same Anthropic model |

An inference profile id such as the one above goes where the model id goes.

## Not supported

- Catalog and binding document support: they refuse `bedrock-converse` by name. An `llm.routes/1`
  document names a Bedrock endpoint, and its routes stay out of the catalog.
- A live qualification: no turn has been sent to Bedrock.
- The credential sources listed under Credentials as not supported.
- Query-string (presigned) signing and SigV4a.
