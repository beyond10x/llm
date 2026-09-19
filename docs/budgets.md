# Durable spending admission

`llm-cost::budget` implements declared-estimate admission for one trusted deployment. Its
`BudgetEngine` is an I/O-free simulator. Enable `llm-cost`'s optional `sqlite` feature to use
`SqliteLedger`, which owns a persistent journal and serializes concurrent callers. The default
library does not depend on SQLite. No method contacts a model or hosting provider.

One immutable `BudgetPolicy` identifies the ledger, currency, monetary limit and maximum number of
outstanding reservations. Every inference attempt and owned compute operation in that deployment
must use that same ledger. A different directory or policy ID is a different administrative scope;
there is no automatic period rollover, reset, FX conversion or per-request replacement budget.
The active-obligation limit is 1 through 65,536. A zero monetary limit admits only explicitly
reserved zero amounts; it does not establish that an endpoint is free.

## Reserve, start and settle

Call `apply(now_ms, command)` using a trusted Unix-millisecond clock. Time cannot move backwards.
Each call first observes elapsed obligations, even if its requested command is then refused.
Call `Tick` when no other command is being applied; the library supplies no background timer.

1. `Reserve` records an ID, operation attribution, amount, assumption/source reference and expiry.
   Inference attribution includes the immutable provider binding, billing kind and request ID.
   Compute attribution includes hosting provider, account and deployment identity. Admission sums
   known charges and the remaining amounts held by every outstanding reservation. The cap is
   inclusive. Unknown spend or unresolved shutdown blocks admission, including a zero reservation.
2. `Begin` changes a reserved operation to started and returns a `DispatchPermit` only after the
   journal commit. Starting the same reservation again refuses. The receipt is not cloneable or
   serializable; reopening never returns a historical receipt. The consumer must use it for that
   reservation's single external effect while its deadline remains valid. A permit grants no
   tool-execution, cloud-account or credential authority.
3. `Settle` records a known amount, completeness, currency and evidence reference. Partial cost
   retains uncertainty and the unaccounted part of the reservation. Complete cost releases the
   unused reservation. Previously known cost cannot be reduced by reconciliation. An amount above
   the reservation or cap is retained; new admission stops instead of discarding the overrun.

`Cancel` releases only an operation that never started. Expiry also cancels only a never-started
reservation. A started inference that expires becomes uncertain. Explicit `Uncertain`, including
a cancellation or ambiguous dispatch after start, retains the obligation. A new account or target
cannot erase the earlier attempt's cost. Caller/provider evidence, not an expired local timer,
must resolve that uncertainty.

Amounts use the pricing library's exact nanounits. Aggregate totals use checked `u128` nanounits
and are exposed as decimal integer strings, so recorded overruns can exceed the per-observation
`Amount` range without rounding. `settled_nanos` includes known partial cost; it does not mean all
obligations have settled. For an outstanding reservation, held amount is
`max(reserved - known_cost, 0)`. Known cost is never capped at the estimate. `uncertain_count`,
`active_count`, `above_limit` and `stop_required` make incomplete totals explicit.

## Compute and fixed charges

A compute deployment can have only one outstanding reservation for its provider/account/deployment
identity. `Renew` atomically reserves an additional amount and extends its expiry. A refused renewal
of a started compute operation marks it stop-required. Expiry, explicit `RequireStop`, uncertainty
or owner recovery also preserves a stop obligation. None proves that provider billing stopped.

The hosting controller must stop the resource and call `ConfirmStopped` with its observed evidence.
Stopped resources retain unresolved cost until a complete settlement. A failed cleanup or missing
cost observation cannot release the reservation. The hosting integration must arrange timely ticks,
shutdown and reconciliation, including startup and shutdown time in its forecast. This library
has no cloud control-plane implementation and cannot itself turn a GPU off.

`PostCharge` records an observed metered, compute or subscription charge, even if already over cap.
Unknown charges block new admission until `ReconcileCharge` supplies the amount and evidence.
Reservation and charge IDs share one uniqueness boundary; duplicate IDs never add another charge.
Fixed subscription fees are posted once as explicit observations, not once per inference. Token
reference valuations from subscription/self-hosted usage are not additional monetary charges.

The caller owns forecasts and truthful evidence. Settle an existing reservation when reconciling
its invoice; do not also post that same charge as a second independent obligation. Stable IDs,
invoice reconciliation, interval nonoverlap and account/price attribution remain producer
responsibilities. The API does not authenticate invoices or infer marginal fees from a vendor name.

## Persistence and ownership

`SqliteLedger::create(directory, policy, now_ms)` creates a new directory and refuses an existing
one. `open(directory, &policy, now_ms)` opens only existing files, requires exact policy equality,
and checks `llm.budget/1`, SQLite application/schema identity, contiguous journal sequence and the
outcome of every replayed command. Missing, unsupported or inconsistent data never becomes an empty
ledger. The original journal retains reservation assumptions, extensions, charge observations,
refusals and recovery; the inspection view shows their current effects.

The owner holds `owner.lock` until its connection closes, then explicitly unlocks it before closing
the handle. This also releases the shared lock when a concurrent process spawn has briefly inherited
the descriptor before exec. Another owner handle or process must refuse while the ledger is alive;
share one `Arc<SqliteLedger>` across threads. Never use an inherited SQLite connection after fork.
The lock file is never unlinked. The standard library
[documents the OS lock's lifetime and advisory/platform behavior](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock).
Use trusted local storage and one cooperating owner. Do not delete or replace live files, share the
directory across hosts, or treat restoring a stale backup as normal restart. This is not protection
against an administrator modifying the journal or bypassing the library.

The SQLite adapter uses `DELETE` journaling with `synchronous=EXTRA`, verifies those settings, and
commits before exposing a start receipt. SQLite documents the additional journal-directory sync
needed for this rollback-journal durability mode in its
[`synchronous` contract](https://www.sqlite.org/pragma.html#pragma_synchronous). Actual power-loss
durability still depends on the filesystem/device honoring synchronization; tests do not simulate
hardware failure. Source uses rusqlite 0.40.2 with its bundled SQLite and a locked dependency graph.

After any journal write failure, that owner stops issuing permits until close and verified reopen.
Inspection retains known obligations, marks existing starts uncertain/stop-required and reports
`storage_failed`. Reopening durably recovers all interrupted starts, even after a clean close.
This handles a crash after commit but before a receipt reached its caller without replaying the
external effect. The library never automatically retries a paid request or recreates a resource.

A mutex serializes calls within the owner; SQLite transactions and the lifetime OS lock protect
persistence and process ownership. Calls are synchronous; asynchronous compositions should use
bounded blocking execution. Replay is linear in journal history, and the current engine stages a
copy of its records for each commit. There is no automatic compaction or history deletion. Journal
entries are bounded independently; history has no artificial 4096-request lifetime limit.

## Verification and integration limits

[Budget verification](verification/budgets.md) records real SQLite restart, concurrent admission,
process-lock contention, abrupt process exit, commit failure and mutation evidence. ESS views
observe returned phases, amounts, errors, permits, policy and attribution. Their notification and
execution-record entities belong to the verification adapter, not a production event bus.

This establishes the library's admission policy over explicit assumptions and observations. It is
not a hard ceiling on the provider's final invoice. Gateway/fallback/hosting consumers must route
all relevant effects through this ledger, preserve actual model/usage evidence, and consume its
shutdown obligations. Those integrations and live provider/hosting qualification remain separate
foundation stories. Ordinary tests make no paid calls and create only disposable local databases.
