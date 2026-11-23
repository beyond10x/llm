// Per-run result files under the out dir, and their reduction to ratchet values and gate counts.

import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import type { KeyboardResult, OverflowResult, VisibleResult } from './layout.ts';
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
  overflow: OverflowResult;
  visible: { top: VisibleResult[]; bottom: VisibleResult[] };
  axe: { counts: Record<string, number>; rules: AxeRule[] };
  keyboard: KeyboardResult | null;
  shift: { load: number; afterReady: number };
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

/** Ratchet values from the gating results. All lower is better. */
export function toMeasured(results: Result[]): Measured {
  const m: Measured = {};
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
    const s = (m[r.scenario] ??= {
      'layout.page_scroll': 0,
      'layout.overflow': 0,
      'always_visible.misses': 0,
      'keyboard.misses': 0,
      'layout_shift.after_ready': 0,
      ...Object.fromEntries(IMPACTS.map((i) => [`axe.${i}`, 0])),
    });
    s['layout.page_scroll'] += r.overflow.docScroll > 0 ? 1 : 0;
    s['layout.overflow'] += r.overflow.overflow.length;
    s['always_visible.misses'] += [...r.visible.top, ...r.visible.bottom].filter((v) => v.status !== 'ok' && v.status !== 'n/a').length;
    if (r.keyboard)
      s['keyboard.misses'] +=
        (r.keyboard.skipLinkFirst ? 0 : 1) + (r.keyboard.skipMovesFocus ? 0 : 1) + Object.values(r.keyboard.reached).filter((v) => !v).length;
    s['layout_shift.after_ready'] = Math.max(s['layout_shift.after_ready'], r.shift.afterReady);
    for (const i of IMPACTS) s[`axe.${i}`] += r.axe.counts[i] ?? 0;
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
