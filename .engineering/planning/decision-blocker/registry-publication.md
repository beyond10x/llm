---
format: aep.planning-md/3
id: decision-blocker:registry-publication
kind: decision-blocker
status: open
title: Publishing llm's crates to a registry needs the owner's decision
relations:
- blocks: story:crates-from-a-registry
revision: 1
---
## Missing decision

Whether llm's library crates are published to a registry, and to which one. At tag 0.7.0 the
workspace sets `publish = false` (`Cargo.toml:27`) and AGENTS.md's release procedure is a source
release with no assets. A version published to crates.io cannot be deleted, only yanked, and the
`b10x-llm-*` names become public. `b10x-llm-credentials` has optional Git dependencies on the
`secrets` library (`crates/llm-credentials/Cargo.toml:40-41`), which a registry refuses unless
`secrets` is published too or the feature leaves the published crate.

## Clears when

The owner's decision is recorded as an approval record that names the registry, the crates, and what
happens to the `secrets` feature; or the owner decides not to publish, and story:crates-from-a-registry
is archived with that reason.
