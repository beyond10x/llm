## Four decisions the gateway unit handed back

Recorded here rather than answered in a message, because each settles something a later story will
otherwise re-open.

### The gateway takes no dependency on the routing crate, now or later

The brief told the unit to inspect routes through `llm_routing::Catalog::explain`. It measured that
this is unbuildable in a unit worktree: adding any dependency requires a lock-file update, the lock
is the coordinator's, and `cargo metadata --locked` exits 101 rather than write one.

It took the alternative, and the alternative is better. The crate declares **zero dependencies**.
Its route inventory is a data-only snapshot with no field for a URL, a secret reference or
credential material, and no callback. A test asserts the absence of every crate that could resolve
a secret or provision a resource, by name. "Reveals no credential and triggers no provisioning" is
now a fact about the dependency graph rather than a property somebody has to keep remembering.

**The decision: keep it.** Adding the routing dependency later would reverse that guarantee to buy
convenience. The adapter from a catalog to a route inventory belongs in the binary that composes
them, which is `story:operator-cli`'s crate, not here.

### Correction, after the adversary read the check

The paragraph above was written before anyone tested the assertion, and its last sentence was
wrong as implemented. The check splits the metadata output at the first bracket, which closes the
first dependency's own features array: measured on a real workspace package it examined one of
eighteen dependencies. The other half suppresses dependencies entirely, so no transitive edge is
examined at all — and the crate this document names as the natural next dependency reaches a
credential crate and an async runtime in two hops.

So "a fact about the dependency graph rather than a discipline" was a claim about an assertion
nobody had exercised, and the coordinator recorded it. The decision to keep the crate free of that
dependency still stands; the reason it is safe today is that the crate declares nothing, not that
the check would notice if it did. The check is being repaired in correction round 1.

### The gateway gets no specification domain in wave 1

Its scope inferred one; its brief excluded one. The exclusion stands and the evidence is in the
tree: this wave's opening commit pre-declared three new domains and deliberately not a gateway one.

The story's acceptance is about refusal, authentication and disclosure, and Rust tests against a
real loopback socket establish all three. `story:gateway-translation` is where a gateway domain
earns its place, because that story is about behaviour across protocols, which is what a shared
executable specification is for.

The unit also declined to write `docs/verification/gateway-report.json`. That file is the retained
report of a conformance run; there is no run, so writing one would record something that did not
happen. Correct, and the reason is the right one.

### Two coordinator files get the gateway's rows at integration

`README.md` says there is no usable gateway yet, which is now partly false, and
`docs/implementation-status.md` has no gateway row at all. Both are coordinator-owned and both are
fixed when the unit merges, not by the unit.

### One gap the unit named against itself

Its documentation carries a Rust example in a markdown file, which nothing compiles. Its doc-test
lane reported zero before and zero after, and it named that as a gap rather than letting an
unchanged count pass unmentioned. Worth carrying into whichever story adds the composing binary,
since that is where the example becomes executable.
