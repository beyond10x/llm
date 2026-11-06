---
format: aep.planning-md/3
id: review-result:adversary-seams-u2-pass-1
kind: review-result
status: active
title: 'Wave 2026-11-05-llm-seams adversary, story:closed-label-classifier, pass 1: four introduced'
relations:
- reviews: story:closed-label-classifier
revision: 1
---
## Adversary pass 1, unit 2 (story:closed-label-classifier)

Attacked `impl/closed-label-classifier` at `247fdadb4`; cases committed as `b31312021`
(`crates/llm-decide/tests/adversary_u2_chain.rs`, 4 cases, 4 red).

Verdict: red. Cases executed 11 → 15, red 4. Origin: introduced 4, pre-existing 0, undecided 0.

Routed by the orchestrator (2026-11-05 wave): one correction round for all four in this wave, then merge.

```findings
[{"file": "crates/llm-decide/src/chain.rs", "line": 105, "category": "acceptance", "severity": "warning", "verdict": "CONFIRMED", "origin": "introduced", "message": "Chain::classify returns a Decided result from a public-trait Classifier unchecked, so a label outside the set and a 20000 bps confidence become the chain's decision (adversary_u2_chain::a_chain_never_returns_a_decision_outside_its_label_set)"},
 {"file": "crates/llm-decide/src/chain.rs", "line": 113, "category": "acceptance", "severity": "note", "verdict": "CONFIRMED", "origin": "introduced", "message": "the chain never clears is_final on earlier attempts, so an abstention a classifier marked final stays final before a later decision (adversary_u2_chain::an_abstention_before_a_decision_is_never_final)"},
 {"file": "crates/llm-decide/src/classify.rs", "line": 685, "category": "acceptance", "severity": "warning", "verdict": "CONFIRMED", "origin": "introduced", "message": "a provider's own Deadline error 10 ms into a 30 s chain abstains Timeout instead of ProviderError(deadline), contradicting Timeout's doc and losing the code (adversary_u2_chain::a_provider_deadline_error_inside_the_chain_deadline_abstains_with_its_code)"},
 {"file": "crates/llm-decide/src/classify.rs", "line": 667, "category": "boundary", "severity": "note", "verdict": "CONFIRMED", "origin": "introduced", "message": "the deadline test truncates both durations to whole ms, so an answer at 1999.6 ms of a 2000 ms deadline abstains Timeout (adversary_u2_chain::an_answer_before_the_deadline_is_used_on_a_clock_finer_than_a_millisecond)"}]
```
