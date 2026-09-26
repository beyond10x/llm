---
title: Hosting
description: The owned-resource lifecycle every GPU hosting adapter is held to, and the Runpod adapter built on it, verified against an in-process emulator.
---

# Hosting

An endpoint that already exists needs no hosting: declare it in the catalog and call it. Hosting is
for the other case, where LLM starts a billed GPU machine, serves a model on it, and must make sure
that machine is stopped.

Two crates cover it:

| Crate | What it is |
| --- | --- |
| `b10x-llm-provision` | The hosting contract: identity, lifecycle, leases, reconciliation and cleanup. It has no dependencies, opens no socket, reads no credential and allocates nothing. `FakeProvider` demonstrates it in process |
| `b10x-llm-runpod` | The first adapter behind that contract, for vLLM on Runpod. Every Runpod call goes through a `RunpodTransport` trait, and the only transport that exists is `EmulatedRunpod`, an in-process control plane |

:::caution No real machine has been started
There is no production Runpod transport yet. Nothing in this repository has contacted a hosting
provider, allocated a GPU or stopped one. The adapter's behavior is verified against the emulator
only.
:::

## Three rules

Each rule exists because the obvious alternative costs money.

**A name is not an identity.** A provider may give a name to a new resource after the old one is
gone. A controller that matched on name would adopt somebody else's machine and eventually stop it.
A `ResourceKey` is `(provider, account, name, incarnation)`, where the provider assigns the
incarnation at creation. A resource with the same name and a different incarnation is not found,
not adopted and never stopped.

**Requested is not observed.** `DeploymentSpec` is what the operator asked for.
`ProvisionedDeployment` is what the provider reported, and none of its fields is ever filled in from
the request. An unreported address, served model or readiness is `None`. It never means `false`,
and it never becomes the requested value.

**Only evidence discharges a stop obligation.** A disconnected client, an expired lease, a
controller restart or a partial listing prove nothing about billing. An obligation to stop a
resource closes only with evidence: absence from a complete listing, a provider report that the
resource terminated, an accepted stop request, or an external confirmation. This uses the same
vocabulary as the spending ledger in `llm-cost`, so a hosting controller produces exactly the
evidence the ledger's `ConfirmStopped` needs.

## The lifecycle

```text
Declared     -> Requested | Uncertain | Cancelled
Requested    -> Requested | Active | StopRequired | Stopped | Disowned
Active       -> Active | StopRequired | Stopped | Disowned
Uncertain    -> Uncertain | Requested | Active | StopRequired | Stopped | Disowned
StopRequired -> StopRequired | Stopped | Disowned
Stopped, Cancelled, Disowned are terminal
```

- A create whose answer was lost becomes `Uncertain`, because a billed resource may or may not
  exist. It is never retried blindly. Retrying is refused unless the provider honours an
  idempotency key; otherwise only reconciliation resolves it.
- `Cancelled` is reachable only from `Declared`. Cancelling cannot discharge an obligation that was
  already incurred.
- A later healthy observation does not withdraw `StopRequired`.
- `Disowned` means a controller with a newer lease took the resource over. Nothing was stopped, so
  it is reported as transferred, not as discharged.

## Ownership, limits and cost

- **Leases fence controllers.** One lease per deployment, and every grant raises a strictly
  increasing epoch that is never reused. A controller holding an older epoch cannot mutate
  anything, including after its own restart.
- **Restart keeps obligations.** A restored controller keeps identity, phase and every open
  obligation, drops its lease and its liveness observations, and must reacquire a lease before it
  acts.
- **Ceilings oblige a stop; they do not perform one.** `HostingPolicy` caps active resources and
  lifetime. A resource past its lifetime becomes `StopRequired` with reason `lifetime-exceeded`.
- **No authorization, no resource.** Declaring a deployment requires a `ComputeAuthorization`
  naming the spending ledger and the reservation it is admitted against.
- **Listing and validation allocate nothing.** Reconciliation must never be the cause of the
  double billing it exists to prevent.

## Runpod

The Runpod adapter ports an existing gateway's pod lifecycle onto the contract:

- **Identity.** A pod is a `ResourceKey` whose incarnation is its Runpod pod id. Owner, epoch and
  request id are written into the pod's environment and read back from the listing. Pod names start
  with `b10x-llm-`.
- **One pod per cold model.** The first caller creates the pod; every later caller is told it is
  starting.
- **Ordered GPU choice.** A refused placement tries the next declared GPU type. A lost create
  answer ends the attempt, because Runpod takes no idempotency key and trying the next GPU could pay
  twice.
- **Readiness and crash recovery.** A pod that crash-loops inside the declared window, refuses its
  vLLM key, or misses its startup deadline is terminated. The next request starts a fresh one.
- **Idle reaping.** An idle pod is reaped, but never below the measured cold-start time and never
  while a `StreamLease` holds it for an open stream.
- **Orphan sweep.** Only pods carrying this controller's owner tag that no record holds are swept.
  A pod labelled for another controller is never terminated.
- **Status.** Only `TERMINATED` ends a resource. `EXITED` is a stopped pod that still exists and is
  billed, so it is terminated and replaced.
- **No key in this process.** The vLLM API key reaches a pod as a Runpod secret reference, never as
  a value.

Runpod-specific settings (ordered GPU types, cloud type, disk, network volume, data-center pinning,
startup deadline, idle timeout, crash-restart limit and every vLLM argument) live in `RunpodModel`,
not in the generic `DeploymentSpec`.

### Known open issue

Orphan sweeps and the termination of pods inherited from a previous controller call the provider
directly. No stop obligation is recorded for them, so the spending ledger never learns that the
resource existed or that it was stopped. Closing this needs a change to the hosting contract.

### Not verified

These are properties of the live Runpod control plane and have not been checked: that the REST
listing returns the environment a pod was created with, that the secret reference is substituted
into the vLLM key, that a `DELETE` stops billing, and which proxy URL to serve from.

## Try it

```bash
cargo test -p b10x-llm-provision --locked
cargo test -p b10x-llm-runpod --locked
```

Both suites run entirely in process. The Runpod suite drives `EmulatedRunpod` through single-flight
start, crash recovery, adoption after restart, idle and orphan cleanup, and the declared vLLM
settings.
