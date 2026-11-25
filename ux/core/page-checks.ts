// Running the pinned `ux-browser checks` over a build, and what it leaves in the out directory: the
// evidence document the ratchet reads (`evidence.ts`), its values file with the hard-gate failures per
// page, viewport and theme, its log and its exit status. The binary is the one the caller was handed
// as `UX_BROWSER`; none is looked for on `PATH`.

import { spawn, spawnSync, type ChildProcess } from 'node:child_process';
import { closeSync, existsSync, mkdirSync, openSync, readFileSync, renameSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { isAbsolute, join } from 'node:path';
import { pageValues, readEvidence, type PageValues } from './evidence.ts';
import { PIN_WORKFLOW, pinnedVersion } from './pin.ts';
import type { Failure, Result } from './results.ts';
import type { Scenario } from './types.ts';

export interface PageCheckFiles {
  /** The `ratchet-evidence/1` document. */
  evidence: string;
  /** The values file, `ux-browser-values/1`. */
  values: string;
  /** What ux-browser wrote to its two streams. */
  log: string;
  /** `{code, signal}` once the run ended. */
  exit: string;
  /** Chromium's profile directory for the run. */
  profile: string;
}

export function pageCheckFiles(out: string): PageCheckFiles {
  return {
    evidence: join(out, 'page-checks.evidence.json'),
    values: join(out, 'page-checks.json'),
    log: join(out, 'ux-browser.log'),
    exit: join(out, 'ux-browser.exit.json'),
    profile: join(out, 'ux-browser-profile'),
  };
}

export interface PageCheckRun {
  /** The checks configuration, under `root`. */
  config: string;
  /** The built site ux-browser serves, under `root`. */
  build: string;
  /** The directory the configuration's scenario and finding patterns start from. */
  root: string;
  project: string;
  assessment: string;
  /** Page and viewport pairs measured at a time. */
  jobs: number;
}

export function pageCheckArgs(run: PageCheckRun, files: PageCheckFiles): string[] {
  return [
    'checks',
    ...['--config', run.config, '--build', run.build, '--root', run.root],
    ...['--project', run.project, '--assessment', run.assessment],
    ...['--out', files.evidence, '--values', files.values, '--profile', files.profile, '--jobs', String(run.jobs)],
  ];
}

/** Whether `path` names a regular file. A symlink counts as what it names; a directory and a name that leads nowhere are no file. */
function isFile(path: string): boolean {
  try {
    return statSync(path).isFile();
  } catch {
    return false;
  }
}

/** The `ux-browser` the caller was handed: `UX_BROWSER`, an absolute path to a regular file. Anything else is refused. */
export function uxBrowser(env: Record<string, string | undefined>): string {
  const handed = env.UX_BROWSER;
  const needed =
    'the pinned ux-browser is handed over as an absolute path in UX_BROWSER: run the gate as `task ux-check` (a round as `task ux-round`), ' +
    'or set it to what `cargo run --locked -p llm-conformance -- ux-browser` prints at the repository root';
  if (!handed) throw new Error(`UX_BROWSER is unset; ${needed}`);
  if (!isAbsolute(handed) || !isFile(handed)) throw new Error(`UX_BROWSER is ${JSON.stringify(handed)}, which is no absolute path to a file; ${needed}`);
  return handed;
}

/** The name the exit status is written under, beside `files.exit`, before it is renamed to it. */
const exitDraft = (files: PageCheckFiles): string => files.exit + '.tmp';

/**
 * Writes the exit status of a run: under a temporary name in the same directory, then renamed. A
 * reader finds no exit file or the whole one, never a part of it.
 */
export function writePageChecksExit(files: PageCheckFiles, exit: PageCheckExit): void {
  writeFileSync(exitDraft(files), JSON.stringify(exit) + '\n');
  renameSync(exitDraft(files), files.exit);
}

function clear(files: PageCheckFiles): void {
  for (const f of [files.evidence, files.values, files.log, files.exit, exitDraft(files)]) rmSync(f, { force: true });
  rmSync(files.profile, { recursive: true, force: true });
  for (const f of [files.evidence, files.values, files.log, files.exit, files.profile]) mkdirSync(join(f, '..'), { recursive: true });
}

/**
 * Starts ux-browser and returns at once. What a run before left is removed first, so a file found
 * afterwards is this run's. The child leads a process group of its own, which `stopPageChecks` ends
 * with its Chromium; its exit status goes to `files.exit` when it ends.
 */
export function startPageChecks(binary: string, run: PageCheckRun, files: PageCheckFiles): ChildProcess {
  clear(files);
  const log = openSync(files.log, 'w');
  const child = spawn(binary, pageCheckArgs(run, files), { detached: true, stdio: ['ignore', log, log] });
  closeSync(log);
  child.on('error', (e) => writePageChecksExit(files, { code: null, signal: null, error: e.message }));
  child.on('exit', (code, signal) => writePageChecksExit(files, { code, signal }));
  return child;
}

/** Ends a run that has not ended, and the Chromium it started: the whole process group. */
export function stopPageChecks(child: ChildProcess | null): void {
  if (!child?.pid || child.exitCode !== null || child.signalCode !== null) return;
  try {
    process.kill(-child.pid, 'SIGKILL');
  } catch {
    // The group is gone already.
  }
}

/** Runs ux-browser to its end with its output on this process's streams, and returns its exit status. */
export function runPageChecks(binary: string, run: PageCheckRun, files: PageCheckFiles): number | null {
  clear(files);
  const done = spawnSync(binary, pageCheckArgs(run, files), { stdio: 'inherit' });
  writePageChecksExit(files, { code: done.status, signal: done.signal });
  return done.status;
}

export interface PageCheckExit {
  code: number | null;
  signal: string | null;
  error?: string;
}

/** What the exit file holds: the exit status when it is a whole one, else its text, and neither when there is no file. */
function exitFile(file: string): { exit: PageCheckExit | null; text: string | null } {
  let text: string;
  try {
    text = readFileSync(file, 'utf8');
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === 'ENOENT') return { exit: null, text: null };
    throw e;
  }
  try {
    const doc = JSON.parse(text) as PageCheckExit | null;
    if (doc && typeof doc === 'object' && 'code' in doc && 'signal' in doc) return { exit: doc, text };
  } catch {
    // Not JSON, or not all of it yet.
  }
  return { exit: null, text };
}

/**
 * The exit status of the run, waiting for it up to `withinMs`; null when it has not ended by then.
 * A file that is there and holds no whole exit status is not the end of the run: it is read again,
 * and one that still holds none at the deadline is refused with what it holds. This module's own
 * writers rename a whole file into place (`writePageChecksExit`).
 */
export async function pageChecksEnded(files: PageCheckFiles, withinMs: number): Promise<PageCheckExit | null> {
  const until = Date.now() + withinMs;
  for (;;) {
    const found = exitFile(files.exit);
    if (found.exit) return found.exit;
    if (Date.now() >= until) {
      if (found.text === null) return null;
      throw new Error(`${files.exit}: holds ${JSON.stringify(found.text)} after ${withinMs} ms, which is no exit status; needed is {"code", "signal"} as a run writes it when it ends`);
    }
    await new Promise((r) => setTimeout(r, 200));
  }
}

/**
 * The values file ux-browser writes beside the evidence (`--values`):
 * `units`, the hard-gate failures per page, viewport and theme, each with the open finding that
 * waives it; `incomplete` and `unmapped`, what the run could not turn into a value; and, with a
 * configuration that declares `values.gating`, `viewports`, the numbers per viewport over all pages.
 */
export interface PageChecks {
  values: Record<string, Record<string, number>>;
  units: { path: string; viewport: string; theme: string; failures: Failure[] }[];
  incomplete: unknown[];
  unmapped: unknown[];
  viewports?: Record<string, Record<string, number | boolean>>;
}

export function readPageChecks(file: string): PageChecks {
  if (!existsSync(file)) throw new Error(`${file}: no such file; ux-browser writes its values file there`);
  const pc = JSON.parse(readFileSync(file, 'utf8')) as PageChecks;
  for (const k of ['units', 'incomplete', 'unmapped'] as const)
    if (!Array.isArray(pc?.[k])) throw new Error(`${file}: no list \`${k}\`; it is not a values file of ux-browser`);
  return pc;
}

/**
 * The five numbers of every page scenario from the evidence document of the run in `out`, over the
 * `gating` viewports. The document must be the one the ux-browser pinned in `repo` wrote; a missing
 * one, one of another kind and one that does not cover the scenarios are refused (`evidence.ts`).
 */
export function pageValuesOf(out: string, repo: string, scenarios: Scenario[], gating: string[]): PageValues {
  const version = pinnedVersion(readFileSync(join(repo, PIN_WORKFLOW), 'utf8'));
  const pages = scenarios.flatMap((s) => (s.page ? [{ name: s.name, path: s.page.path }] : []));
  return pageValues(readEvidence(pageCheckFiles(out).evidence, version), pages, gating);
}

const unitKey = (path: string, viewport: string, theme: string) => `${path}|${viewport}|${theme}`;

/** The page results, each with the page checks' failures of its page, viewport and theme added to its own. */
export function withPageChecks(results: Result[], pc: PageChecks): Result[] {
  const by = new Map(pc.units.map((u) => [unitKey(u.path, u.viewport, u.theme), u.failures]));
  return results.map((r) => (r.kind === 'page' ? { ...r, failures: [...r.failures, ...(by.get(unitKey(r.path, r.viewport, r.theme)) ?? [])] } : r));
}

/** The page, viewport and theme units the page checks failed at a layout gate (1 or 2), as `path|viewport|theme`. */
export function layoutFailed(pc: PageChecks): Set<string> {
  return new Set(pc.units.filter((u) => u.failures.some((f) => f.gate !== 5)).map((u) => unitKey(u.path, u.viewport, u.theme)));
}

/** What a run left that is no value: the unwaived failures, and the incomplete and unmapped entries. */
export function pageCheckProblems(pc: PageChecks): { unwaived: string[]; waived: string[]; incomplete: unknown[]; unmapped: unknown[] } {
  const failures = pc.units.flatMap((u) => u.failures);
  return {
    unwaived: failures.filter((f) => !f.waivedBy).map((f) => f.key),
    waived: failures.filter((f) => f.waivedBy).map((f) => `${f.key} by ${f.waivedBy}`),
    incomplete: pc.incomplete,
    unmapped: pc.unmapped,
  };
}
