Request for a coordinator-owned change — not applied, not mine to apply.

Owner:   crates/llm-core (frozen for wave 1) and docs/contract-v1.md (versioned contract)
Raised by: story:responses-projection, correction round 1, adversary finding 3
Status:  the gap is closed conservatively inside crates/llm-responses; this would close it well.

## What the projection had to do instead

`ingest_request` now REFUSES any `input` entry outside the four modelled shapes, with
`ErrorCode::Unsupported`. See crates/llm-responses/src/request.rs, `input_to_item`, and
contracts/responses/scenarios/ingress-refuses-an-unmodelled-entry-it-cannot-attribute.yaml.

## Why

`Item::Opaque` has exactly one representation: bound to a `Provenance`. A Responses request body
carries no provenance. So a gateway reading one back has two choices and both are wrong:

  - stamp it with the binding doing the reading. This is what the first implementation did. It
    asserts an origin nobody observed, and it launders: a payload minted under binding revision
    `rev-1`, replayed by a client after the endpoint was repointed to `rev-2`, comes back out
    addressed to `rev-2`. State that `project_request` refuses becomes state `project_request`
    sends, in one pass. That contradicts the invariant directly — "Opaque continuation state is
    bound to its exact target ... A mismatch is refused, never silently reused."

  - refuse it. Sound, and what the code now does, but it costs reasoning continuity across a tool
    round trip through a gateway: under `store: false` the provider's reasoning item is exactly
    such an entry, and a client that cannot replay it makes the model re-derive its plan on every
    call.

## What would close it

A third state for carried-but-unattributed continuation state, which a gateway may hold and
forward only back to a binding that an explicit decision names. Sketch, not a patch, because the
shape is llm-core's to choose:

    pub enum Item {
        ...
        Opaque { provenance: Provenance, payload: Value },
        /// Carried across an ingress surface that could not observe where it came from.
        /// Never sendable: an adapter refuses it until a caller binds it explicitly.
        UnattributedOpaque { payload: Value },
    }

with, in docs/contract-v1.md, a sentence in "Turn ownership" stating that an ingress surface may
carry opaque state it cannot attribute, that such state is not sendable, and that binding it is an
explicit caller decision rather than an adapter's.

This is a contract change: `llm.turn/2` would need to become `llm.turn/3`, since an older reader
must not silently accept the new variant. That is why it is a request and not an edit.

## What must not be done instead

Do not widen `Provenance` with a sentinel revision, and do not make the comparison partial. Both
make the mismatch representable as a match, which is the failure this whole section exists to
prevent.
