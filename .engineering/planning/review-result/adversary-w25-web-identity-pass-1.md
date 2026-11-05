---
format: aep.planning-md/3
id: review-result:adversary-w25-web-identity-pass-1
kind: review-result
status: active
title: Wave 2026-11-04-w25 adversary, llm story:bedrock-web-identity, pass 1
relations:
- reviews: story:bedrock-web-identity
revision: 1
---
```
unit: beyond10x/llm story:bedrock-web-identity, integration head 196f113c8 plus test commit 2e313fffc
verdict: CONFIRMED (no red case, no finding)
cases: executed 20→34, red 0
origin: introduced 0 / pre-existing 0 / undecided 0
needs-coordinator: none
```

Cases added (all green), in `crates/llm-bedrock/tests/adversary_w25_web_identity.rs` and
`crates/llm-http/tests/adversary_w25_form_exchange.rs`: the form encoding of a token holding reserved
characters; a `Credentials` block in the wrong parent and a member outside it; CDATA, processing
instructions, namespace prefixes, a byte-order mark and a second declaration refused; CRLF, tabs and
fractional `Expiration` accepted; a success over 64 KiB and a cut document final; `Throttling` final
and `IDPCommunicationError` retriable; endpoint guard spellings (uppercase, IPv6 and numeric loopback
admitted; `LOCALHOST`, `localhost.`, `0.0.0.0`, `[::]`, userinfo refused); every redirect status
unfollowed; one shared request under concurrent failure, not kept; a missing token file during a
refresh keeps the credentials; the proxy test fails when `.no_proxy()` is removed.

Out of scope: none beyond the routes-document kind, left out by design.

```findings
[]
```
