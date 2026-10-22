---
format: aep.planning-md/3
id: story:bedrock-signing
kind: story
status: draft
title: Requests to Bedrock are signed with hand-written SigV4 from injected credentials
summary: SigV4 on ring passing the AWS v4 suite; static keys through a resolver; the container credentials endpoint
relations:
- decomposes: epic:bedrock
- serves: vision:portable-model-inference
revision: 2
---
## Change

- `llm_bedrock::sigv4`: canonical request, string to sign and signature for header signing,
  HMAC-SHA256 and SHA-256 from `ring`.
- `AwsCredentials`, `CredentialSource`; `StaticCredentials` resolves the access key id, secret
  access key and optional session token through an injected `SecretResolver`;
  `ContainerCredentials` reads the container credentials endpoint
  (`AWS_CONTAINER_CREDENTIALS_FULL_URI`) with the authorization token the resolver returns
  (bound by the caller to `AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE`), caching until five minutes
  before `Expiration`.

## Acceptance

- `crates/llm-bedrock/tests/sigv4_vectors.rs` runs every case under `tests/data/sigv4/` (the AWS
  v4 suite from aws-c-auth) and matches its canonical request, string to sign and signature;
  none is skipped without a named reason.
- The container source refuses a URI outside `https` and the documented link-local and loopback
  hosts, sends the token as `authorization`, and maps every failure to `unauthorized` before
  dispatch (`crates/llm-bedrock/tests/credentials.rs`).
- No secret appears in a `Debug` output or an error message.
