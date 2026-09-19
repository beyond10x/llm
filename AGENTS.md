# Working on LLM

## Serves

- O1: explicit model reach, credentials and spending decisions.
- O3: one neutral inference interface for multiple agent harnesses.
- O6: attributable usage, costs and failures that can be compared.

## Boundaries

This repository owns model inference and hosting, not agent loops or tool execution. Core types
perform no I/O, carry no execution permission and depend on no consumer repository. Protocol,
provider/account, credential source, model and hosting provider remain distinct concepts.

Read README.md, docs/design.md and the owning AEP artifact before implementation. Validate the
ESS domain before decomposing new entities. All planning-store changes use the AEP CLI. Keep
unknown usage absent, secrets out of serializable configuration, and incompatible opaque state
explicitly refused. A provider adapter never silently changes endpoints or billing accounts.

Use managed worktrees. Commits and publication use the organization bot and existing Gates/Atlas
controls. No private policy or credentials belong in this repository. Run `task check` before
publication. Runtime code and checkers are Rust; no paid provider call belongs in the default gate.

Read docs/implementation-status.md for the implemented libraries and remaining planned boundaries.
Do not mark implementation stories complete merely because the workspace builds. Published
contracts are versioned; later consumer adoption pins a released or explicitly qualified exact
revision. Local fixture evidence does not establish live provider or hosting qualification.
