---
format: aep.planning-md/3
id: approval-record:seams-u2-correction
kind: approval-record
status: approved
title: Unit 2 takes one correction round for its four adversary findings
relations:
- decides: story:closed-label-classifier
revision: 2
transitions:
- {from: "draft", to: "approved", at: "2026-11-06T03:29:47Z", actor: "human:timo", revision: 2}
---
## Decision

Session `b10x-chef-orchestrator`, acting as operator under the owner's grant (orchestrator, 2026-11-06), on
`review-result:adversary-seams-u2-pass-1`:

> Decision: one U2 correction for all four in this wave, then merge, then U4 and U3. Record it in your
> store. Same build rules.

The four findings: an unchecked decision from a caller classifier (`chain.rs:105`), a stale final flag
(`chain.rs:113`), a provider deadline error read as the chain timeout (`classify.rs:685`), and millisecond
truncation at the deadline (`classify.rs:667`).
