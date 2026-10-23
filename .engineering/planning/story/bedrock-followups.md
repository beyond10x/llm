---
format: aep.planning-md/3
id: story:bedrock-followups
kind: story
status: implemented
title: 'Bedrock follow-ups: a public pre-send check, a non-blocking credential refresh, anthropic-beta to InvokeModel'
summary: check_request and encode_request on BedrockTarget; single-flight container refresh; Hints::anthropic_beta carried by Messages and InvokeModel
relations:
- decomposes: epic:bedrock
- serves: vision:portable-model-inference
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-10-23T02:19:43Z", actor: "human:timo", revision: 2, decided_on: {"recorded":{"test_result":1}}, executor: "agent:llm-session"}
- {from: "proposed", to: "active", at: "2026-10-23T02:19:43Z", actor: "human:timo", revision: 3, decided_on: {"recorded":{"test_result":1}}, executor: "agent:llm-session"}
- {from: "active", to: "implemented", at: "2026-10-23T02:19:43Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":1}}, executor: "agent:llm-session"}
---
## Context

Three follow-ups to `epic:bedrock`, found while a gateway put `llm-bedrock` behind a Messages
ingress for a coding-agent client (llm 0.12.0):

1. A gateway cannot ask whether a turn will be refused by the Converse encoder before it commits
   to the route: the encoder is private, and its refusals (no initial user message, tool history
   without published tools, a reasoning effort, a parallel tool call control, the encoded-size
   bound and the rest) are only seen as the turn's error.
2. `ContainerCredentials` holds a Tokio `Mutex` across the container credentials fetch (up to the
   transport's total limit), and every turn inside the five-minute refresh window fetches. Turns
   queue behind one slow endpoint while the kept credentials are still valid.
3. Bedrock `InvokeModel` for Anthropic models takes beta flags as the body field `anthropic_beta`
   (an array of strings). The neutral turn has no field for them, so a gateway drops the
   `anthropic-beta` header a coding-agent client sends.

## Change

**Pre-send check.** `llm_bedrock::check_request(&TurnRequest, &BedrockTarget)` returns exactly the
refusal the target's encoder returns, and `llm_bedrock::encode_request` is the body the client
sends. The client encodes through `encode_request`, and `check_request` runs it and drops the
body, so the two cannot drift.

**Single-flight refresh.** While the kept container credentials are unexpired they are answered at
once. Inside the refresh window at most one refresh runs, in the background, not on the caller's
cancel; a caller waits only when the kept credentials are expired or absent, and then joins the
one fetch in flight. A failed refresh keeps the unexpired credentials.

**`anthropic-beta`.** A new advisory hint `anthropic_beta` (`Hints::anthropic_beta`, reported as
`anthropic-beta`) carries the beta names. `llm_messages::decode_request_with_headers` reads the
`anthropic-beta` header values beside the body; a gateway that holds the values otherwise sets the
field. The Messages client sends them as one `anthropic-beta` header (after `oauth-2025-04-20` for
a subscription account), the Bedrock `InvokeModel` encoder writes `anthropic_beta`, and Converse,
Chat Completions and Responses omit the hint and report it in their `advisory-omitted` warning.

## Out of scope

- A live Bedrock or Anthropic call in the gate; everything is checked against fixtures.
- Choosing which beta names a wire accepts: the names are carried as the client sent them.

## Acceptance

- For each refusal of the Converse encoder, `check_request` returns the same code and message as
  the turn, and the fixture endpoint receives no request; for an accepted turn it is `Ok` and
  `encode_request` is the body the endpoint received.
- N concurrent `credentials` calls inside the refresh window, with a refresh that stalls, all
  return the kept credentials within a short bound; the endpoint sees exactly one refresh request.
  After the kept credentials expire, callers wait for that fetch and get its answer.
- A request with `anthropic-beta` headers decoded by `decode_request_with_headers` carries the
  names; the Messages client sends them as one `anthropic-beta` header; the `InvokeModel` body
  carries `anthropic_beta`; a Converse turn omits it and its `advisory-omitted` warning names
  `anthropic-beta`.
- `RUSTUP_TOOLCHAIN=1.98.0 task check` passes.
