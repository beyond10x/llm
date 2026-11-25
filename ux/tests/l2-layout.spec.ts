// L2: every sitemap page × viewport × theme. Two parts hold the hard gates 1, 2 and 5 at the gating viewports.
//
// The five page checks (page scroll, overflow, always visible, keyboard, layout shift) are the pinned
// ux-browser's: the global setup starts it over the same build beside these tests, and the last test
// of this file awaits its end and fails on a hard-gate failure no open finding waives, on a page it
// could not measure, and on evidence another ux-browser than the pinned one wrote. A round runs
// ux-browser before the specs (`cli.ts round`) and reports its failures without failing.
//
// Axe (gate 5) and the scroll containers (measured, never gated) are measured here, one test per page
// and viewport: the page is loaded once in the first theme, and every later theme is applied in place
// by switching the emulated colour scheme (the site follows prefers-color-scheme). Axe runs its full
// rule set in the first theme and only its colour rules (adapter.axeThemeRules) in every later one:
// the other rules read the DOM, which a theme does not change. The measured-only viewports (round
// mode) are recorded and never fail. A round takes the failure screenshot of a unit here, for an axe
// failure and for a layout failure of the page checks alike.

import { existsSync, mkdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test, expect, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import { adapter } from '../adapter.ts';
import { readEvidence } from '../core/evidence.ts';
import { waiverFor } from '../core/findings.ts';
import { layoutFailed, pageCheckFiles, pageCheckProblems, pageChecksEnded, readPageChecks } from '../core/page-checks.ts';
import { PIN_WORKFLOW, pinnedVersion } from '../core/pin.ts';
import { writeResult, type AxeRule, type Failure, type PageResult } from '../core/results.ts';
import { pageDrift } from '../core/scenarios.ts';
import { measureScrollContainers, readyTime } from '../core/scroll-containers.ts';
import { sitemapPaths } from '../core/site.ts';
import { FINDINGS, OUT, REPO, ROUND_MODE, SCENARIOS, SITE_ROOT, UX, base, hermetic } from './env.ts';

const clean = (s: string) => s.replace(/[|,]/g, ' ');
const PAGE_CHECKS = pageCheckFiles(OUT);
/** Round mode: the units the page checks failed at gate 1 or 2, which get a failure screenshot here. */
const LAYOUT_FAILED = ROUND_MODE ? layoutFailed(readPageChecks(PAGE_CHECKS.values)) : new Set<string>();

test('every sitemap page has a page scenario, and every page scenario is in the sitemap', () => {
  const drift = pageDrift(join(UX, adapter.dirs.scenarios), sitemapPaths(SITE_ROOT, adapter.site.sitemap));
  expect(drift, 'run `task ux-scenarios` to add page scenarios; remove stale ones by hand').toEqual({ missing: [], stale: [] });
});

const viewports = [
  ...adapter.viewports.gating.map((v) => ({ ...v, gating: true })),
  ...(ROUND_MODE ? adapter.viewports.measured.map((v) => ({ ...v, gating: false })) : []),
];

const themeOf = (page: Page) => page.evaluate((a) => document.documentElement.getAttribute(a), adapter.themeAttribute);

for (const sc of SCENARIOS.filter((s) => s.page)) {
  const path = sc.page!.path;
  for (const vp of viewports) {
    test(`L2 ${sc.name} ${vp.name}`, async ({ browser }) => {
      const first = adapter.themes[0];
      const context = await browser.newContext({
        viewport: { width: vp.width, height: vp.height },
        colorScheme: first as 'light' | 'dark',
        reducedMotion: 'reduce',
      });
      await hermetic(context);
      const page = await context.newPage();
      const url = base() + path;
      const settle = ROUND_MODE ? adapter.settleMs : (adapter.checkSettleMs ?? adapter.settleMs);
      const resp = await page.goto(url, { waitUntil: 'load' });
      expect(resp?.status(), `${path} is served`).toBe(200);
      await readyTime(page);
      await page.waitForTimeout(settle);

      const unwaived: string[] = [];
      for (const theme of adapter.themes) {
        const t0 = Date.now();
        const timings: Record<string, number> = {};
        const mark = (k: string) => (timings[k] = Date.now() - t0);
        if (theme !== first) {
          await page.emulateMedia({ colorScheme: theme as 'light' | 'dark' });
          await page.waitForFunction(([a, t]) => document.documentElement.getAttribute(a) === t, [adapter.themeAttribute, theme], { timeout: 5_000 }).catch(() => {});
          await page.waitForTimeout(100);
        }
        const themeApplied = (await themeOf(page)) === theme;
        expect(themeApplied, `${adapter.themeAttribute}=${theme} applied`).toBe(true);
        await page.evaluate(() => window.scrollTo(0, 0));

        const scrollContainers = await measureScrollContainers(page, adapter.scrollContainersAllowed);
        mark('scroll-containers');
        let builder = new AxeBuilder({ page }).withTags(adapter.axeTags).options({ resultTypes: ['violations'] });
        if (theme !== first && adapter.axeThemeRules) builder = builder.withRules(adapter.axeThemeRules);
        if (adapter.axeExclude && path !== adapter.axeSample) builder = builder.exclude(adapter.axeExclude);
        const axe = await builder.analyze();
        mark('axe');
        const rules: AxeRule[] = axe.violations.map((v) => ({
          id: v.id,
          impact: v.impact ?? 'unknown',
          nodes: v.nodes.length,
          help: v.help,
          targets: v.nodes.slice(0, 5).map((n) => n.target.join(' ')),
        }));
        const counts: Record<string, number> = {};
        for (const r of rules) counts[r.impact] = (counts[r.impact] ?? 0) + r.nodes;

        let shot: string | undefined;
        const k = (gate: number, detail: string) => `gate${gate}|${path}|${vp.name}|${theme}|${clean(detail)}`;
        const failures: Failure[] = [];
        const fail = (gate: Failure['gate'], detail: string) => {
          const key = k(gate, detail);
          if (!failures.some((f) => f.key === key)) failures.push({ gate, key, detail, waivedBy: waiverFor(FINDINGS, key) });
        };
        for (const r of rules) if (r.impact === 'serious' || r.impact === 'critical') fail(5, `axe:${r.id}`);

        if (ROUND_MODE && (failures.length || LAYOUT_FAILED.has(`${path}|${vp.name}|${theme}`))) {
          const sel = rules.filter((r) => r.impact === 'serious' || r.impact === 'critical').flatMap((r) => r.targets);
          const style = await page
            .addStyleTag({ content: sel.map((s) => `${s}{outline:3px solid #e0007a !important;outline-offset:1px}`).join('\n') })
            .catch(() => null);
          await page.waitForTimeout(adapter.settleMs);
          const dir = join(OUT, 'shots-all');
          mkdirSync(dir, { recursive: true });
          shot = join(dir, `fail-${sc.name}-${vp.name}-${theme}.jpg`);
          await page.screenshot({ path: shot, type: 'jpeg', quality: 70, scale: 'css' });
          await style?.evaluate((e) => (e as Element).remove()).catch(() => {});
        }

        const result: PageResult = {
          kind: 'page',
          scenario: sc.name,
          path,
          viewport: vp.name,
          theme,
          gating: vp.gating,
          themeApplied,
          scrollContainers,
          axe: { counts, rules },
          failures,
          shot,
          timings,
        };
        writeResult(OUT, result);
        for (const f of failures.filter((f) => f.waivedBy)) test.info().annotations.push({ type: 'waived', description: `${f.key} by ${f.waivedBy}` });
        unwaived.push(...failures.filter((f) => !f.waivedBy).map((f) => f.key));
      }
      await context.close();
      if (vp.gating) expect(unwaived, 'hard gate failures not waived by an open finding').toEqual([]);
    });
  }
}

// Declared last: a worker takes it when every page test above has been handed out, so it waits only
// for what is left of ux-browser's run, which started with the global setup.
test('L2 the five page checks of the pinned ux-browser: no hard-gate failure unwaived, every page measured', async () => {
  test.skip(ROUND_MODE, 'a round runs ux-browser before the specs and reports its failures (cli.ts round)');
  const pinned = pinnedVersion(readFileSync(join(REPO, PIN_WORKFLOW), 'utf8'));
  const log = () =>
    existsSync(PAGE_CHECKS.log)
      ? readFileSync(PAGE_CHECKS.log, 'utf8').split('\n').filter((l) => l && !l.startsWith('measured ')).slice(-20).join('\n')
      : `no ${PAGE_CHECKS.log}`;
  const ended = await pageChecksEnded(PAGE_CHECKS, 50_000);
  expect(ended, `ux-browser has not ended 50 s after every page test was handed out; what it wrote:\n${log()}`).not.toBeNull();
  expect([0, 1], `ux-browser writes its values with exit 0 or 1; it ended with ${JSON.stringify(ended)} and wrote:\n${log()}`).toContain(ended!.code);
  // Refused here, before any of its values counts: evidence another ux-browser than the pinned one wrote.
  const evidence = readEvidence(PAGE_CHECKS.evidence, pinned);
  const problems = pageCheckProblems(readPageChecks(PAGE_CHECKS.values));
  test.info().annotations.push({ type: 'tools', description: `ux-browser ${evidence.tools.uxBrowser}, ${evidence.tools.chromium}` });
  for (const w of problems.waived) test.info().annotations.push({ type: 'waived', description: w });
  expect(problems.unwaived, 'hard gate failures not waived by an open finding').toEqual([]);
  expect(problems.incomplete, 'what ux-browser could not measure').toEqual([]);
  expect(problems.unmapped, 'pages without a page scenario; run `task ux-scenarios`').toEqual([]);
  expect(ended!.code, `ux-browser's exit; it wrote:\n${log()}`).toBe(0);
});
