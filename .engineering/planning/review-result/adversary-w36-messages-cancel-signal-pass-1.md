---
format: aep.planning-md/3
id: review-result:adversary-w36-messages-cancel-signal-pass-1
kind: review-result
status: active
title: 'Adversary, pass 1 (story:messages-cancellation-case-waits-on-a-signal, wave 2026-11-25-tests-without-a-clock): green, 1 pre-existing note, 0 red cases, 1 case added'
relations:
- reviews: story:messages-cancellation-case-waits-on-a-signal
revision: 1
---
Adversary pass 1 (the only one) on unit `story:messages-cancellation-case-waits-on-a-signal`, head
`b8f575e6fe11479b28a27121a0c51c7d813982d6`, base `45e7c1c13`, one file:
`crates/llm-messages/tests/client.rs`.

Verdict: green. Cases executed 12 → 13, 0 red. Origin: pre-existing 1 (a note). The pass ran in
three rounds: the host restarted during the first, and the second stopped when a job went to the
build node while the rule of that hour called that a fault. The third round's three builds ran on
the build node; every run of a changed client and of the base executed the fetched executable on
the workstation.

The one finding, `crates/llm-messages/tests/client.rs:469` (`() = &mut stop => turn.await`), note,
verdict INFEASIBLE, pre-existing: the case polls the turn after the cancel, so a client whose wait
in `emit` reads the token only when it is polled, and registers no wake, passes it. Built and run:
the unit's case passes 10 of 10 on that client (7 to 16 ms), and the base's case passes 10 of 10
too, each after 10.00 s, when the turn's own limit wakes it. No other case of the crate cancels into
a blocked sink. Nothing in the tree reaches it: `Cancel` is `tokio_util::sync::CancellationToken`,
which wakes.

The case the adversary wrote, `adversary_w36_a_cancel_wakes_a_turn_parked_in_the_sink`: green at the
head (`1 passed; 0 failed; 12 filtered out`), red on that client in 9 ms:

```
panicked at crates/llm-messages/tests/client.rs:826:5:
the cancel did not wake the turn parked in the sink
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 12 filtered out; finished in 0.00s
```

It is red at once on a client that ignores the cancel, where the unit's case needs the 10 s limit.
Suite with the case: `13 passed; 0 failed`. Clippy with `-D warnings` exit 0, `rustfmt --check` exit
0.

The response head or the first event made late, head against base:

| What | Head | Base |
| --- | --- | --- |
| head 400 ms late, cancellation | pass, 0.40 s | red, `left: Unknown` / `right: Accepted` |
| head 400 ms late, deadline | pass, 2 rungs, 0.90 s | red, `left: Unknown` / `right: Accepted` |
| head 150 ms late, cancellation | pass, 0.15 s | red, `left: Unknown` / `right: Accepted` |
| head 150 ms late, deadline | pass, 1 rung, 0.31 s | pass |
| head 2 s late, deadline | pass, 4 rungs (0.3, 0.6, 1.2, 2.4 s), 4.51 s | not run |
| first event 400 ms late, cancellation | pass, 0.40 s | pass, the sink is never offered an event |
| first event 400 ms late, deadline | pass, 2 rungs, 0.90 s | pass, the sink is never offered an event |

Every rung ended 0.4 to 4.8 ms over its limit.

Changed clients and changed tests at the head:

| Change | Result | Failing case and output |
| --- | --- | --- |
| `emit` registers no wake for the cancel | survives the unit's 12 cases | the adversary's case alone |
| `emit` ignores the cancel | caught after 10.0 s | the cancellation case, `left: Deadline` / `right: Cancelled` |
| `emit` ignores the deadline | caught after 3.0 s | the deadline case, `Elapsed(())` |
| no observation kept | caught | the cancellation case, the deadline case and `a_truncated_stream_keeps_its_partial_usage_and_is_not_sent_again` |
| the turn limit ignored | caught after 3.0 s | the deadline case, `Elapsed(())` |
| the deadline halved | caught | the deadline case, `the turn ended after 151.229926ms, before its limit of 300ms`; the base's file passes on it |
| test: `cancel()` removed | red after 10.0 s | the cancellation case, `left: Deadline` |
| test: the server panics before the head | red in 8 ms | `the server wrote the response head: RecvError(())` |
| test: `emit` returns at once | red | `the blocked sink was cancelled` |
| test: head and stream in one write | pass | |
| test: one write, the head's signal 300 ms late | pass, 0.30 s | |

Under load: each of the two changed cases 100 of 100 runs exit 0 beside four looping copies of the
test executable on the workstation (slowest run 32 ms and 986 ms by the script's clock; the 986 ms
run reports `finished in 0.31s`). The runs beside a Clippy build were not measured by the adversary:
the build ran on the build node. The implementor's 100 of 100 beside a Clippy build on the
workstation stand alone.

Read and not broken: no `sleep` or bound between the start of the turn and the cancel; a cancel that
never reaches the sink ends red after 10.0 s (`MessagesClient::new` sets the turn limit to the
transport's total), it does not hang; the three assertions of the cancellation case are the base's;
the deadline case reads its start before the client takes its deadline, so load can only lengthen
the measured turn; the six grep hits for a bounded wait that the unit's list does not name are upper
bounds on a connect; both case names of `docs/verification/messages-falsification.json` still fail
on the edit that file cites.
