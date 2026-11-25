---
format: aep.planning-md/3
id: review-result:adversary-w34-docs-ux-browser-pass-2
kind: review-result
status: active
title: 'Adversary, pass 2 (story:docs-ux-layout-checks-from-ux-browser, wave 2026-11-25-docs-ux-browser): red, 1 warning and 3 notes, 1 red case'
relations:
- reviews: story:docs-ux-layout-checks-from-ux-browser
revision: 1
---
Adversary pass 2, the last, on unit `story:docs-ux-layout-checks-from-ux-browser`, head
`cadcf074cc4984d054f97c973459dab5a4a54208` (correction 1).

Verdict: red, one warning, no blocker. Cases executed 183 → 187, 1 red. Origin: introduced 4.

Pass 1's findings 1 to 5 are closed, each held by a case: the workflow guard case is green with
its body unchanged but for the name, and six guard mutants are red on the unit's own cases; the
directory case and the symlink case hold `uxBrowser`; the exit status is renamed into place and
the write-in-place mutant is red; the seven cases of pass 1 are in the suite under plain names
with no expectation, input, variant or timeout changed; `UXF-005.md` points to the values file.

1. A step `uses: ./.github/actions/<name>` passes `pin` and `steps`
   (`checks/xtask/tests/gate/ux_browser_pin.rs`). No guard opens the action's file, and the doc
   comment of `steps` says the guard does not pass what it does not read. The repository has no
   `.github/actions` today.
2. The words of `AGENTS.md` say no workflow step starts or fetches a `ux-browser` of its own; the
   guard finds text that names one. A name built from parts, a file appended to `GITHUB_ENV`, a
   script file a step runs, an action of another repository, `defaults.run.shell`, a job-level
   `BASH_ENV` and a directory put on `GITHUB_PATH` before the install step pass. Nothing does any
   of this today.
3. Four mutants pass the unit's suite: `steps` not reading the workflow outside its jobs,
   `foreign` matching case-sensitively, `exitFile` taking a document without `signal`, `isFile` as
   "not a directory". The adversary left three guard cases, green at the head and red on each.
4. `AGENTS.md` names "a pool job killed at its limit" as a runner ended by SIGKILL. Measured in a
   pool job with a 20 s limit: the runner got SIGTERM, its handler ended the stand-in, nothing was
   left running. The sentence without the parenthesis holds.

Measured at the head: the docs UX step at 4 workers 38.1 s, 111 passed; at 2 workers 90.5 s, 111
passed, on the build node at load 21 to 32; the ratchet exit 0 after each. `llm-xtask` 72 passed
before the adversary's cases. No run over 120 s, no case over 60 s. The L1 file leaves no process.

Attacked and not broken: the five real workflow files give no finding; in the real `docs-ux.yml` a
comment, `curl --retry 3` and a dropped `tar` line are each refused naming the line; the guard
finds `gh release download`, the API address of the uxui repository, an `env` holding the address,
a container and a service image, `export`, `tee -a`, `printf` with the whole name, a read of
`UX_BROWSER`, a matrix member and a second install step; an exit document that is `{"code":0}`,
`null`, `[]`, a number or a text is refused at the deadline; a FIFO and a device are refused as
`UX_BROWSER`; a file without the execute bit ends with the spawn error in the exit file, which the
last L2 case prints.

```findings
- file: checks/xtask/tests/gate/ux_browser_pin.rs
  line: 786
  category: contract-drift
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: A step that uses an action of this repository (uses ./.github/actions/<name>) passes pin and steps although no guard opens the action file and the doc comment of steps says it does not pass what it does not read; nothing does this today.
- file: AGENTS.md
  line: 207
  category: contract-drift
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: The words say no workflow step starts or fetches a ux-browser of its own, while the guard finds only text that names one; a name built from parts, a file appended to GITHUB_ENV, a script file, a foreign action, defaults.run.shell, BASH_ENV and an early GITHUB_PATH entry pass.
- file: ux/core/page-checks.ts
  line: 147
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: Four mutants pass the unit's suite (a document without signal taken, isFile as not-a-directory, the workflow outside its jobs not read, foreign case-sensitive); three guard cases left by the adversary are red on each.
- file: AGENTS.md
  line: 275
  category: contract-drift
  severity: note
  verdict: NEEDS-CHANGE
  origin: introduced
  message: The parenthesis names a pool job killed at its limit as a runner ended by SIGKILL, but a pool job at its limit got SIGTERM first and the handler ended the stand-in, leaving nothing running.
```
