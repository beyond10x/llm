---
format: aep.planning-md/3
id: approval-record:prefix-and-bedrock-2026-10-22
kind: approval-record
status: draft
title: Package rename and the Bedrock backend, approved by b10x-chef-orchestrator under the owner's grant on 2026-10-22
relations:
- decides: story:drop-package-prefix
revision: 1
---
## The stop

The wave skill's stage-1 proposal stop for the package rename and the Bedrock backend, and the
release of llm as the next minor.

## What authorises passing it

Session `b10x-chef-orchestrator` sent on 2026-10-22, under the owner's grant in the workspace
instructions (class C items in a repository under the workspace):

> Read and follow ~/beyond10x/.claude/skills/chef-orchestrator/brief.md, then: implement
> ~/.cache/b10x-layers/brief-llm-bedrock.md (Step 0, the b10x- prefix removal in llm per
> ~/.cache/b10x-layers/brief-drop-b10x-prefix.md, then the llm-bedrock crate). Disk is low (28G)
> while the ESS 0.69.0 bar runs: no cargo, task or build of any kind until I message you 'go
> builds'; until then edit source, fetch the AWS sig-v4 test vectors, write tests. Approved under
> the grant in ~/beyond10x/CLAUDE.md.

The owner's words the briefs quote (relayed, 2026-10-22): "makes no sense, remove the b10x
prefix", then "for all of them!"; and on signing, "use the handwriting then".

Covered: the commits of the waves for `story:drop-package-prefix` and the Bedrock stories under
`epic:bedrock`; bot pushes of their branches, their pull requests and their landing on `main` by a
bot fast-forward of a green head; the source release of llm as the next minor through
`AGENTS.md` § Release. Not covered: registry publishing, consumer pin moves in other
repositories, and the llm-gateway hosted target (that repository's own work).

## Decisions taken inside the item

- The conformance checker's binary is renamed with its package (`llm-conformance`): it is run
  only through `cargo run -p`, never installed, so no installed command changes name.
- Historical records (`docs/plan/`, `docs/verification/`, `changes/`, released `CHANGELOG.md`
  sections and this store) keep the names they were written with.
