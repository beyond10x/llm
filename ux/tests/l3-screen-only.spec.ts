// L3 screen-only user: from the home page, visible controls only, toward each task's truth (hard gate 4).

import { test, expect } from '@playwright/test';
import { adapter } from '../adapter.ts';
import { waiverFor } from '../core/findings.ts';
import { writeResult, type Failure } from '../core/results.ts';
import { runScreenOnly } from '../core/screen-only.ts';
import { FINDINGS, OUT, SCENARIOS, base, hermetic } from './env.ts';

const vp = adapter.viewports.gating.find((v) => v.name === adapter.screenOnly.viewport);
if (!vp) throw new Error(`screen-only viewport ${adapter.screenOnly.viewport} is not a gating viewport`);

for (const sc of SCENARIOS.filter((s) => s.task)) {
  const { card, truth, expectations } = sc.task!;
  test(`L3 ${sc.name}`, async ({ browser }) => {
    const context = await browser.newContext({
      viewport: { width: vp.width, height: vp.height },
      colorScheme: adapter.screenOnly.theme as 'light' | 'dark',
      reducedMotion: 'reduce',
    });
    await hermetic(context);
    const page = await context.newPage();
    const result = await runScreenOnly(page, card, truth, {
      base: base(),
      primary: adapter.primaryRegion,
      hubWords: adapter.hubWords,
      maxActions: adapter.screenOnly.maxActions,
      maxScrolls: adapter.screenOnly.maxScrolls,
      settleMs: adapter.settleMs,
    });
    await context.close();
    const failures: Failure[] = [];
    if (expectations.reach && !result.reached) {
      const key = `gate4|${truth.path}|${vp.name}|${adapter.screenOnly.theme}|${sc.name}`;
      failures.push({ gate: 4, key, detail: result.reason ?? 'not reached', waivedBy: waiverFor(FINDINGS, key) });
    }
    writeResult(OUT, { kind: 'task', scenario: sc.name, viewport: vp.name, theme: adapter.screenOnly.theme, result, failures });
    test.info().annotations.push({ type: 'trace', description: result.trace.map((s) => `${s.action} -> ${s.url}`).join(' | ') });
    expect(failures.filter((f) => !f.waivedBy).map((f) => `${f.key}: ${f.detail}`), 'screen-only user reaches the truth').toEqual([]);
  });
}
