# Shared library verification — 2026-09-19

Observed locally with Rust 1.98, AEP 0.55.0 and ESS 0.26.0:

```console
task check
cargo run --locked -p b10x-llm-core --example embedded
```

`task check` passed: 29 runtime tests and two compile-fail documentation tests, formatting, strict
workspace Clippy, ESS validation and AEP validation. The embedding example printed:

```text
An embedded model turn. (EndTurn; usage None)
```

| Test source | Count | Evidence |
| --- | ---: | --- |
| `crates/llm-core/src/id.rs` | 4 | Identifier bounds and validated deserialization. |
| `crates/llm-core/tests/dependency_boundary.rs` | 1 | Actual locked workspace dependencies exclude consumer crates; core has no HTTP transport dependency. |
| `crates/llm-core/tests/embedding.rs` | 8 | Async tool/text round trip, sink backpressure/cancellation, bounded output, full opaque provenance, version/unknown-field refusal, output validation and unknown usage. |
| `crates/llm-credentials/tests/injected.rs` | 5 | Per-request rotation, concurrent renewal, cancellation during renewal, bounded reference coordination, missing credentials, binary secrets and redaction. |
| `crates/llm-http/tests/framing.rs` | 4 | Every split point in UTF-8/CRLF SSE, multiline data, event names, bounded frames, terminal truth and preserved prefix before malformed data. |
| `crates/llm-http/tests/transport.rs` | 7 | Local HTTP sockets exercise cancellation, redirect refusal, partial-stream failure, header/idle/total deadlines, retry hints and single-attempt behavior. |
| `crates/llm-credentials/src/lib.rs` documentation | 2 | Secret values cannot be serialized or displayed. |

The transport fixtures assert connection closure even when a failed stream object remains held.
The resolver cancellation fixture asserts that an interrupted renewal is not repeated until the
caller reconciles the generation. SSE fixtures preserve valid output before malformed data
independently of how a transport splits its chunks.

ESS reported `llm v1 — 2 file(s), valid`. AEP reported 44 valid artifacts, with six existing
review records reported as having no findings block. Those immutable approvals contain empty
`findings` arrays; AEP 0.55.0 currently reports those arrays as absent. The records are preserved.

This does not demonstrate a protocol projection, authenticated provider request, live subscription,
fallback decision, budget enforcement, gateway or cloud deployment. Those requirements remain in
their own stories and in [implementation status](../implementation-status.md). The Cargo packages
are version 0.0.0 and this record is not a release attestation.

## CI-discovered deadline correction

[CI run 35429528719](https://github.com/beyond10x/llm/actions/runs/35429528719) at
`b66b4256dd4e5ea37a7bed0b6dfdf73c277bf1a3` failed the keepalive/total-deadline fixture:
the error was `Transport` instead of `Deadline`. The reqwest timeout and the transport's own
absolute deadline raced. The correction removes the redundant reqwest timeout so one deadline
controls the request and stream. Review also found an EOF framing error losing dispatch evidence;
the corrected fixture asserts `Accepted`, including on subsequent reads of the failed stream.

After these corrections, local `task check` passed and the exact deadline regression passed
twelve consecutive executions. The original local result above is retained; it did not establish
the race was absent. Remote verification of the corrected revision is tracked separately by CI.
