# Responses overload classification — 2026-10-07

The Loom clock-query reproduction reached an admitted read action, then the provider returned a
Responses SSE `error` with exact machine code `server_is_overloaded`. The adapter classified that
code as `Refused`, hiding the distinction between a request refusal and temporary availability.
The original live diagnostic remains in the caller's private qualification record; no provider
payload or credential is committed here.

The correction recognizes only that exact code as `Unavailable`, accepted dispatch, with the
fixed diagnostic `the provider is temporarily overloaded`. Decoder state prevents replay after
any prior output payload, including an opening item with no visible event. ResponsesClient uses
the same state for transport-cut finality. The client remains single-attempt; a caller owns retry
bounds, cancellation, delays and accounting.

Verification:

- The real ResponsesClient loopback regression first failed twice, observing `Refused` where
  `Unavailable` was required; three control tests passed.
- The five loopback tests then passed: overload before output, final overload after visible or
  silent output, unknown and actual refusal codes, and the existing transport-cut boundaries.
- Independent review found that the first revision guarded only the live client. Centralizing
  answered state in Decoder resolved that finding; direct decoder regressions also hold output
  finality and nested overload usage retention. The reviewer found no remaining blocker.
- `RUSTUP_TOOLCHAIN=1.98.0 task check` exited 0: 831 test executions, three existing ignored tests,
  format and strict Clippy, documentation drift, specification validation, planning validation,
  and 692/692 ESS scenarios passing three times with no unsupported or skipped scenarios.
- After the review correction, `cargo test --locked -p b10x-llm-responses` under Rust 1.98.0
  passed all 149 tests, with one existing ignored adversary case. The gate's later Clippy and
  conformance phases had already checked that revision.
- `npm --prefix website ci` and `npm --prefix website run build` exited 0.

The ESS scenario is `provider-overload-is-availability`; executable tests are in
`crates/llm-responses/tests/retry_class.rs` and `projection.rs`. This is deterministic adapter
qualification. It does not claim live provider availability or perform a live retry.
