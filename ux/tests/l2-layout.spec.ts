// L2 layout contract: every sitemap page × viewport × theme. Hard gates 1, 2 and 5 at the gating viewports;
// the measured-only viewports (round mode) are recorded and never fail.
//
// One test per page and viewport: the page is loaded once in the first theme, and every later theme is
// applied in place by switching the emulated colour scheme (the site follows prefers-color-scheme). Axe runs
// its full rule set in the first theme and only its colour rules (adapter.axeThemeRules) in every later one:
// the other rules read the DOM, which a theme does not change. The tab order is walked once per viewport.

import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { test, expect, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import { adapter } from '../adapter.ts';
import { CLS_INIT, checkAlwaysVisible, keyboardWalk, measureOverflow, readShift, readyTime } from '../core/layout.ts';
import { waiverFor } from '../core/findings.ts';
import { writeResult, type AxeRule, type Failure, type PageResult } from '../core/results.ts';
import { pageDrift } from '../core/scenarios.ts';
import { sitemapPaths } from '../core/site.ts';
import { FINDINGS, OUT, ROUND_MODE, SCENARIOS, SITE_ROOT, UX, base, hermetic } from './env.ts';

const clean = (s: string) => s.replace(/[|,]/g, ' ');

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
      await context.addInitScript(CLS_INIT);
      const page = await context.newPage();
      const url = base() + path;
      const settle = ROUND_MODE ? adapter.settleMs : (adapter.checkSettleMs ?? adapter.settleMs);
      const resp = await page.goto(url, { waitUntil: 'load' });
      expect(resp?.status(), `${path} is served`).toBe(200);
      const ready = await readyTime(page);
      await page.waitForTimeout(settle);
      const shift = await readShift(page, ready);

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

        const overflow = await measureOverflow(page, adapter.scrollContainersAllowed);
        const top = await checkAlwaysVisible(page, adapter.alwaysVisible, path);
        mark('layout');
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

        await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
        await page.waitForTimeout(200);
        const bottom = await checkAlwaysVisible(page, adapter.alwaysVisible, path);
        await page.evaluate(() => window.scrollTo(0, 0));

        let shot: string | undefined;
        const k = (gate: number, detail: string) => `gate${gate}|${path}|${vp.name}|${theme}|${clean(detail)}`;
        const failures: Failure[] = [];
        const fail = (gate: Failure['gate'], detail: string) => {
          const key = k(gate, detail);
          if (!failures.some((f) => f.key === key)) failures.push({ gate, key, detail, waivedBy: waiverFor(FINDINGS, key) });
        };
        if (overflow.docScroll > 0) fail(1, 'page-scroll');
        for (const o of overflow.overflow) fail(1, `overflow:${o.desc}`);
        for (const [state, list] of [['top', top], ['bottom', bottom]] as const)
          for (const v of list) if (v.status !== 'ok' && v.status !== 'n/a') fail(2, `${v.name}:${v.status}${v.status === 'missing' ? '' : '@' + state}`);
        for (const r of rules) if (r.impact === 'serious' || r.impact === 'critical') fail(5, `axe:${r.id}`);

        if (ROUND_MODE && failures.length) {
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

        let keyboard = null;
        // The tab order does not change with the theme: walked once per gating viewport, in the last theme,
        // after which the page has been reloaded and is not measured again.
        if (vp.gating && theme === adapter.themes[adapter.themes.length - 1]) {
          const targets = adapter.keyboard.mustReach.map((n) => {
            const a = adapter.alwaysVisible.find((x) => x.name === n);
            if (!a) throw new Error(`keyboard target ${n} is not an always-visible element`);
            return { name: n, locate: a.locate };
          });
          keyboard = await keyboardWalk(page, url, adapter.keyboard.skipLink, targets, adapter.keyboard.maxTabs);
          if (!keyboard.skipLinkFirst) fail(5, 'keyboard:skip-link-first');
          else if (!keyboard.skipMovesFocus) fail(5, 'keyboard:skip-link-moves-focus');
          for (const [n, ok] of Object.entries(keyboard.reached)) if (!ok) fail(5, `keyboard:tab-reaches-${n}`);
          mark('keyboard');
        }

        const result: PageResult = {
          kind: 'page',
          scenario: sc.name,
          path,
          viewport: vp.name,
          theme,
          gating: vp.gating,
          themeApplied,
          overflow,
          visible: { top, bottom },
          axe: { counts, rules },
          keyboard,
          shift,
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
