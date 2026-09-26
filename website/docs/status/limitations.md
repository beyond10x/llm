---
title: Limitations and trust boundary
description: What the tests do not establish, and what this boundary refuses to promise.
---

# Limitations and trust boundary

## Nothing here has met a real provider

The Responses, Messages and Chat Completions projections are implemented and each carries its own
conformance suite and falsification record. Every one of those scenarios runs against fixtures,
local sockets and in-process fakes. **No live provider credential has been used anywhere in this
repository**, by design: an ordinary local or CI gate makes no paid provider call.

So the projections are verified against what the contract says a wire looks like, not against what
a vendor actually returned on the day. Treat them as ready to try against your own endpoint, not
as qualified.

Neither OpenAI nor Anthropic access — API or caller-managed subscription — has been qualified.
A passing local suite establishes library behaviour against fixtures and nothing about a live
account.

## The gateway does not translate, and hosting reaches no cloud

Two implemented crates are narrower than their names suggest, and the gap is deliberate rather
than accidental:

- **`llm-gateway` performs no protocol translation.** Its own module documentation names what it
  refuses: "protocol translation, proxying a model call, resolving a secret, reaching a network,
  and multi-tenant accounts or quotas." It authenticates one owner and serves a read-only route
  inventory. You cannot point an OpenAI client at it and have a model answer.
- **`llm-provision` is the hosting contract, not a hosting implementation.** It "opens no socket,
  reads no credential and allocates no cloud resource", and ships an in-process `FakeProvider` to
  demonstrate the lifecycle.
- **`llm-runpod` has no production transport.** Its only transport is the in-process
  `EmulatedRunpod`, and `llm-modal` exports nothing. No real GPU has ever been allocated or stopped
  through this code, and the adapter's assumptions about Runpod's live control plane are unchecked.

There is therefore no cloud control plane here. The budget ledger's stop obligations record that a
resource *must* be stopped — they cannot turn a GPU off, and they never prove that provider
billing stopped.

## There is no operator command line

`llm-cli` exports nothing. Validating, inspecting and running one configuration from a terminal is
not available; the routing example is the closest thing today.

## What the libraries deliberately do not claim

- Core validates structure and declared capabilities. It is not a tokenizer, and it cannot verify
  that an operator's capability declaration is true.
- Request byte limits do not prove context-window admission.
- `final_usage` means the reported counters are terminal. It does not mean every count is known, or
  that any invoice was authenticated.
- Pricing trusts caller observations. It does not authenticate bills or verify a provider's claims,
  and a rate-based estimate is not a measured invoice. There is no grand total across its six
  bases, because an estimate and a recorded charge can describe the same work.
- A budget admission bound is not a promise about a provider's final invoice.
- The core validates the values an adapter supplies, but cannot recover a fact an adapter discarded.

## Credential adapter boundaries

The trusted boundary is the kernel, the filesystem and root or effective-user writers. The file
adapter does not defend against a compromised privileged writer or a filesystem that misreports its
permissions, and a hostile writer can race its metadata checks — they are not a transactional
snapshot guarantee.

macOS ACL evaluation is outside the file adapter's implemented checks, so it refuses macOS and
Windows with `UnsupportedPlatform` rather than accepting a mode-bit-only approximation.

Mock-store tests and native-backend compilation do not establish live OS credential-service
availability, or that a service supports every byte sequence. Blocking OS calls cannot be forcibly
cancelled through this interface: a stuck service can occupy capacity or delay runtime shutdown, so
an embedding needing hard process deadlines must isolate that boundary itself.

## Licence

The workspace declares `LicenseRef-B10x-Proprietary`. Publishing the source grants no open-source
licence, no redistribution right and no patent grant. Reading it, and building and running it
locally to evaluate, review or verify it, are permitted; anything else needs a separate written
agreement. [`LICENSE`](https://github.com/beyond10x/llm/blob/main/LICENSE) is the authority.
