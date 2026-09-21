# Messages projection verification — 2026-09-21

Revised after adversarial review rounds 1 and 2; what each round changed is recorded below, and so
is what this document used to claim where that turned out to be false of the tree it describes.

`llm-messages` implements the declared Anthropic Messages text/tool subset: one shared codec for
the outgoing projection and for stateless gateway ingress, a complete-response decoder, an event
stream decoder that finishes through that same decoder, and a single-attempt streaming client over
the shared HTTP transport. [The contract](../messages.md) states the subset, the refusals and the
usage rules. This is fixture evidence for those functions. It establishes no live Anthropic access:
API and subscription presentation qualification, and gateway server composition, are separate
required stories and no paid call runs in any gate here.

## Counted behavior

`cargo test -p b10x-llm-messages --locked --no-fail-fast` executes **70 runtime tests** across
eleven targets, **all passing, exit 0**: 7 client, 15 projection, 24 streaming, 2 wire-name and 4
falsification-record checks owned by this unit, 2 + 2 + 3 written by adversarial round 1, and
2 + 7 + 2 written by adversarial round 2. The client tests speak HTTP over a loopback socket bound
to port 0; nothing leaves this machine, and no credential is read from it — the injected resolvers
hold a literal fixture string or never answer at all.

`cargo test -p b10x-llm-http --locked` executes **13 tests**, all passing: that crate is in this
unit's evidence because this round added one method to it, and the section below says why.

The `llm.messages` conformance suite selects **41 scenarios**: 32 authored and 9 generated, with
zero synthesis refusals, and every one of the 32 files on disk is selected. Three consecutive runs
over restored source each report **41 passed, 0 failed, 0 error, 0 unsupported, 0 skipped**; the
[retained report](messages-report.json) is the one the runner re-admitted against those exact suite
bytes.

The conformance adapter calls `encode_request`, `decode_request`, `decode_message` and
`decode_stream` and exposes what they returned: the canonical wire body, the neutral turn document,
the decoded item labels, the streamed text, reasoning and tool-argument fragments, the observed
upstream model and response id, usage finality, the usage encoding, the stop reason, and — on a
refusal — the code, the dispatch evidence, the diagnostic itself and whatever snapshot the failure
retained. It reads no suite, names no scenario and reimplements nothing.

Its fixture deployment is a `llm.catalog/1` TOML document parsed by the real catalog, so the
binding under observation is the one an operator's configuration produces, binding revision
included. That revision, `6161a641b63d57ad0a5a44e5f5004e292e932f82b1cb8cad55d191a3ba3f4260`, was
confirmed against an independent SHA-256 of the declaration before any scenario asserted it.

## The turn deadline, and the one method this round added to `crates/llm-http`

The implementation contract says cancellation **and the absolute turn deadline** cover credential
resolution, the HTTP exchange and a blocked sink. All three are now covered by both, and that is
measured rather than stated.

`MessagesClient::new` derives one absolute instant from the transport's own `total`
(`crates/llm-messages/src/client.rs:56`), read through `HttpClient::limits`
(`crates/llm-http/src/transport.rs:59`), so every client has a turn deadline and neither side keeps
a second copy of the value that could drift from the first. `with_turn_limit` replaces it with a
shorter one. The instant reaches credential resolution through `bounded`, the sink through
`StreamDecoder::with_deadline`, and — as of this round — the HTTP exchange through
`HttpClient::post_sse_until`, which takes the caller's instant and bounds the response headers and
every read of the returned `SseStream` by whichever of the two comes first.

Before that method existed the turn's instant stopped at the transport's front door:
`post_sse` started a fresh clock of its own and ran for the `Limits` the `HttpClient` was built
with, so a 300 ms turn on a ten-second transport was still running at three seconds.
`tests/adversary2_deadline.rs` measures both halves of that — a response whose headers never
arrive, and a stream that goes quiet after `message_start` — and both now end on the turn's own
limit. `crates/llm-http/tests/transport.rs` measures the same bound from the transport's side, for
any caller rather than for this one, together with the case that proves a *later* caller instant
cannot lengthen the client's own bound.

**What this document used to say, and what was wrong with it.** It stated that `MessagesClient::new`
does not derive the turn instant, that `HttpClient` exposes no accessor for its limits, and that two
adversarial cases were still failing for want of both. All three were already false when they were
written: the derivation was at `client.rs:56`, the accessor at `transport.rs:59`, and the suite read
52 passed, 0 failed. Its line 17 also said "two of the fifty fail" two lines after saying 52 tests
ran. The gap that did exist was a different one — the instant never reached `post_sse` — and it is
the one this round closed.

## Falsification

[The retained evidence](messages-falsification.json) is generated by one harness in one pass: every
mutation is applied to the source it names, run against **both** lanes, restored, and verified by
SHA-256 against the bytes it started from.

**39 mutations, 0 survivors.** 24 are killed by a scenario and a Rust case together, 4 by a scenario
alone, and 11 by a Rust case alone. The 11 matter most: they are guards no scenario in this suite
reaches, which is exactly the class adversarial round 2 found — twelve mutations aimed at guards
outside the scenarios' reach, of which nine survived the whole gate the unit had shipped. Seven of
those nine are closed by the cases that round wrote in `tests/adversary2_documented_refusals.rs`,
each written from a sentence of `docs/messages.md`; the remaining two are closed by this round, and
one of them is now reachable by a scenario as well:

| The guard nothing reached | Closed by |
| --- | --- |
| the in-flight `MAX_ITEMS` content-block bound | `content_blocks_are_bounded_while_the_stream_is_still_arriving` |
| the shape of an accepted field this subset never reads | `an_accepted_field_this_subset_never_consumes_keeps_its_documented_shape`, plus two authored scenarios |

Three of the twelve were killed by the shipped scenario suite and adversarial round 2 does not name
which; they are not identified here rather than guessed at.

**A record of kills says nothing about the guards nothing was ever aimed at**, and that gap is the
defect rather than any one missing entry. So the record now carries both halves, and the halves are
checked rather than maintained: `tests/falsification.rs` recomputes every fixed diagnostic this
projection can emit from the projection's own source — a string literal with a space in it, the
crate's other literals being producer wire names that `tests/wire_names.rs` owns — and fails when
one is in neither half. Today that is **91 diagnostics: 24 claimed by a mutation and 67 that nothing
is aimed at**, enumerated by name under `unaimed`. A refusal added to the codec is in neither until
somebody puts it in one.

What is still hand-written is the `guards` attribution — which refusal a given mutation deletes.
The check bounds it to the mutated file and no further, so a mutation could still claim another
refusal of that same file and shrink `unaimed` by one without covering anything. That was measured,
not assumed: a cross-file claim fails `a_mutation_only_claims_a_refusal_its_own_source_makes`, and a
same-file one does not. It is the one edge of this record that a reader has to check by reading.

Four of the mutations, and four of the authored scenarios, exist because of adversarial round 1:
arriving thinking that is rebound to the reader, a stream start that is not checked as an assistant
message, content assembled in stop order, and an explicit null read as a value. The first was a
defect that came in with the carried draft and had been written into a contract document —
`ingress-preserves-the-declared-subset` asserted the laundered provenance as the expected value, and
no scenario could see it because the adapter rendered an opaque item as its payload alone. The
adapter now renders the binding as well.

Three scenarios were **rewritten because a mutation did not kill them**, which is the whole reason
the mutations are run. `event-name-contradicting-its-payload-refused` originally framed a lone
`ping` under a contradicting name; with the name check disabled that stream simply ended without a
terminal event and was refused anyway, so the scenario passed for a reason it did not name. It now
frames an otherwise complete turn whose terminal event is announced as a keep-alive.
`duplicate-message-start-refused` and `unstarted-content-block-delta-refused` had the same defect —
a truncated stream refuses whatever else is wrong with it — and all three now carry an otherwise
complete turn.

## Verification scope

Package-scoped gate, all green on restored source, each command's own exit status read:
`cargo test -p b10x-llm-messages --locked --no-fail-fast`, `cargo test -p b10x-llm-http --locked`,
`cargo test -p b10x-llm-conformance --locked`,
`cargo clippy -p b10x-llm-messages --all-targets --all-features --locked -- -D warnings`,
`cargo clippy -p b10x-llm-http --all-targets --all-features --locked -- -D warnings`,
`cargo fmt -p b10x-llm-messages --check`, `cargo fmt -p b10x-llm-http --check` and
`ess specify validate --path spec`. The whole-workspace gate, the committed `contracts/suite.json`
and the generated schemas are regenerated on the integration branch, not here. `contracts/ess-inputs.yaml`
lists no messages scenarios, so the committed suite does not carry `llm.messages` yet; that file is
the coordinator's.

What this does not establish: that any real Anthropic endpoint accepts these bytes; that the pinned
`2023-06-01` API version is current; or that **fourteen of the eighty-one producer names this
projection speaks** are the producer's. Sixty-seven were read from the extraction provenance the
implementation contract names, `harness-messages` at `709a2eb`; fourteen — `auto`,
`cache_creation`, `ephemeral_1h_input_tokens`, `ephemeral_5m_input_tokens`, `caller`, `citations`,
`inference_geo`, `output_tokens_details`, `thinking_tokens`, `rate_limit_error`, `service_tier`,
`timeout_error`, `web_fetch_requests`, `web_search_requests` — are supported by nothing read here
and need the access story's live qualification.

That split is enforced rather than written down: `tests/wire_names.rs` scans this crate's source for
every wire-shaped literal and fails on a name in neither list, and on a list entry the source no
longer speaks. **The count moved from 79 to 81 this round because the scan was too narrow to see the
two names the projection sends most often.** `is_wire_shaped` admitted only `[a-z0-9_]`, so
`anthropic-version` and `2023-06-01` — on the wire of every single request, from
`src/client.rs:23` and `:25` — were in neither list and the check could not say so. The scan now
admits a hyphen and a leading digit, both names are classified as read from `harness-messages`
`src/lib.rs:74` and `:77` at `709a2eb`, and the widening was falsified: respelling `VERSION_HEADER`
makes `every_wire_name_the_projection_speaks_carries_its_evidence` name it.

Fallback after a failed attempt, gateway serving and budget admission are other stories and are
absent here by design.
