---
title: Limitations and trust boundary
sidebar_position: 1
description: What the tests do not establish, and what this boundary refuses to promise.
lede: Shipped means tested against fixtures in the gate; it does not mean qualified against a live provider, a keychain service or a hosting account.
source: docs/implementation-status.md, docs/verification/, crates/llm-credentials
---

# Limitations and trust boundary

## No provider route is qualified

The Responses, Messages and Chat Completions clients each carry their own conformance suite and
falsification record, and every one of those scenarios runs against recorded response bytes,
loopback sockets and in-process fakes. The gate makes no paid provider call, by design.

One live probe was made outside the gate: on 2026-10-04 a Responses turn against the Codex backend
showed two incompatibilities (a success with no content type, and an empty terminal output after
streamed items), which 0.1.6 handles. A probe is not a qualification. Neither OpenAI nor Anthropic
access, API or caller-managed subscription, is qualified, and no route has recorded live evidence.
Treat the clients as ready to try against your own endpoint, not as qualified.

## No gateway and no hosting here

llm serves nothing and provisions nothing. The gateway, the hosting contract and the Runpod and
Modal adapters live in llm-gateway ([GitHub](https://github.com/beyond10x/llm-gateway)), and their
limits are that repository's to state.

There is therefore no cloud control plane here. The budget ledger's stop obligations record that a
resource *must* be stopped; they cannot turn a GPU off, and they never prove that provider billing
stopped.

## There is no operator command line

`b10x-llm-cli` exports nothing. Validating, inspecting and running one configuration from a
terminal is not available; the routing example is the closest thing today.

## What the libraries deliberately do not claim

- Core validates structure and declared capabilities. It is not a tokenizer, and it cannot verify
  that an operator's capability declaration is true.
- Request byte limits do not prove context-window admission.
- `final_usage` means the reported counters are terminal. It does not mean every count is known, or
  that any invoice was authenticated.
- A retried or fallen-back attempt may have been billed. The run records it with its own dispatch
  evidence; it does not claim the attempt was free.
- Pricing trusts caller observations. It does not authenticate bills or verify a provider's claims,
  and a rate-based estimate is not a measured invoice. There is no grand total across its six
  bases, because an estimate and a recorded charge can describe the same work.
- A budget admission bound is not a promise about a provider's final invoice.
- The core validates the values an adapter supplies, but cannot recover a fact an adapter discarded.

## Credential adapter boundaries

The trusted boundary is the kernel, the filesystem and root or effective-user writers. The file
adapter does not defend against a compromised privileged writer or a filesystem that misreports its
permissions, and a hostile writer can race its metadata checks: they are not a transactional
snapshot guarantee.

macOS ACL evaluation is outside the file adapter's implemented checks, so it refuses macOS and
Windows with `UnsupportedPlatform` rather than accepting a mode-bit-only approximation.

The Codex login adapter makes no ownership or permission check: the Codex CLI owns that file and
its protections. With `codex-renewal` it writes the file back; the copies inside the HTTP and TLS
stack and the JSON parser's scratch buffers are not zeroized.

Mock-store tests and native-backend compilation do not establish live OS credential-service
availability, or that a service supports every byte sequence. Blocking OS calls cannot be forcibly
cancelled through this interface: a stuck service can occupy capacity or delay runtime shutdown, so
an embedding needing hard process deadlines must isolate that boundary itself.

## Licence

The workspace is licensed under Apache-2.0 from version 0.1.2.
[`LICENSE`](https://github.com/beyond10x/llm/blob/main/LICENSE) is the authority. Releases up to
0.1.1 were published under `LicenseRef-B10x-Proprietary`.
