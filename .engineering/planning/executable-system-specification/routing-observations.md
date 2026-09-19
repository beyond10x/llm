---
format: aep.planning-md/1
id: executable-system-specification:routing-observations
kind: executable-system-specification
status: draft
title: Executable foundation library observations
relations:
- verifies: story:catalog-routing
- verifies: story:provider-accounts
- verifies: story:local-secret-adapters
model_digest: d00f31c65974df610b7dc83ea0c1a053210dcadb13525334747b478f3bf6b3cc
revision: 4
---
## Scope

Spec: spec/system.yaml and spec/domains/{catalog,routing,secrets}.yaml. The stable artifact ID
retains its original routing name; the compiled model now also contains local-secret declarations
and verification observations. It makes no claim of live inference, persistent credential storage,
durable budgets or hosting lifecycle qualification.

## Verification

The complete suite/5 at contracts/suite.json contains 49 authored scenarios and three generated
observation checks. All 52 pass with zero failed/error/unsupported/skipped in three consecutive
restored-source runs, and synthesis has zero refusals. This retains every previous routing check
and adds real protected-file and injected mock-keychain observations. The gate regenerates 47
schemas and the suite and compares exact bytes/file sets. See docs/verification/local-secrets.md
and its report and falsification records. Nine new production mutations fail named scenarios;
the original eight routing mutation observations remain retained at their historical revision.

Reports are produced by the pinned ESS 0.26.0 runner and carry an exact source digest and real
observation time. CI runs the same report gate, separately compiles native backends on macOS and
Windows, and retains the original suite and detailed runs. No runtime crate depends on ESS.
Mock/native compile evidence does not qualify a live OS service. No artifact lifecycle transition
is performed beside the requested governed Chat driver, which has not launched.
