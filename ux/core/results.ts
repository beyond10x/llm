// Per-run result files under the out dir, and their reduction to ratchet values and gate counts. A page
// result holds what L2 measures itself: axe and the scroll containers. The five page checks' numbers
// come from ux-browser's evidence (`evidence.ts`), its failures from its values file (`page-checks.ts`).

import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import type { PageValues } from './evidence.ts';
import type { ScrollContainer } from './scroll-containers.ts';
import type { ScreenOnlyResult } from './screen-only.ts';
import type { Measured } from './ratchet.ts';

export interface Failure {
  gate: 1 | 2 | 3 | 4 | 5;
  key: string;
  detail: string;
  waivedBy: string | null;
}

export interface AxeRule {
  id: string;
  impact: string;
  nodes: number;
  help: string;
  targets: string[];
}

export interface PageResult {
  kind: 'page';
  scenario: string;
  path: string;
  viewport: string;
  theme: string;
  gating: boolean;
  themeApplied: boolean;
  scrollContainers: ScrollContainer[];
  axe: { counts: Record<string, number>; rules: AxeRule[] };
  failures: Failure[];
  shot?: string;
  timings?: Record<string, number>;
}

export interface TaskResult {
  kind: 'task';
  scenario: string;
  viewport: string;
  theme: string;
  result: ScreenOnlyResult;
  failures: Failure[];
}

export type Result = PageResult | TaskResult;

export function writeResult(outDir: string, r: Result): void {
  const d = join(outDir, 'results');
  mkdirSync(d, { recursive: true });
  const name = r.kind === 'page' ? `${r.scenario}__${r.viewport}__${r.theme}.json` : `${r.scenario}.json`;
  writeFileSync(join(d, name), JSON.stringify(r, null, 2) + '\n');
}

export function readResults(outDir: string): Result[] {
  const d = join(outDir, 'results');
  if (!existsSync(d)) return [];
  return readdirSync(d)
    .filter((f) => f.endsWith('.json'))
    .sort()
    .map((f) => JSON.parse(readFileSync(join(d, f), 'utf8')) as Result);
}

const IMPACTS = ['critical', 'serious', 'moderate', 'minor'];

/**
 * Ratchet values, all lower is better. A page scenario's five page-check numbers are `pageValues`,
 * read from ux-browser's evidence, and nothing of a result file adds to them; its axe counts are
 * summed over the gating page results. A task scenario's numbers are its result's.
 */
export function toMeasured(results: Result[], pageValues: PageValues): Measured {
  const m: Measured = {};
  for (const [scenario, values] of Object.entries(pageValues)) m[scenario] = { ...values };
  for (const r of results) {
    if (r.kind === 'task') {
      m[r.scenario] = {
        'screen_only.actions': r.result.actions,
        'screen_only.looks': r.result.looks,
        'screen_only.steps': r.result.steps,
        'screen_only.unreached': r.result.reached ? 0 : 1,
      };
      continue;
    }
    if (!r.gating) continue;
    const s = (m[r.scenario] ??= {});
    for (const i of IMPACTS) s[`axe.${i}`] = (s[`axe.${i}`] ?? 0) + (r.axe.counts[i] ?? 0);
  }
  return m;
}

export interface GateCount {
  checked: number;
  pass: number;
  fail: number;
  waived: number;
}

/** Per gate: checked units (page × viewport × theme, or task), passed, failed unwaived, failed but waived by an open finding. */
export function gateCounts(results: Result[], gatingOnly = true): Record<string, GateCount> {
  const g: Record<string, GateCount> = {};
  const add = (gate: string, fs: Failure[]) => {
    const c = (g[gate] ??= { checked: 0, pass: 0, fail: 0, waived: 0 });
    c.checked++;
    if (fs.length === 0) c.pass++;
    else if (fs.every((f) => f.waivedBy)) c.waived++;
    else c.fail++;
  };
  for (const r of results) {
    if (r.kind === 'page') {
      if (gatingOnly && !r.gating) continue;
      for (const gate of [1, 2, 5]) add(`gate${gate}`, r.failures.filter((f) => f.gate === gate));
    } else add('gate4', r.failures);
  }
  return g;
}
