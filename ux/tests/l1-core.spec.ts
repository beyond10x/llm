// L1: the core's functions, the findings rule of § 5, and break-tests proving each layout check can fail.

import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test, expect, type Page } from '@playwright/test';
import { adapter } from '../adapter.ts';
import { findingProblems, glob, parseFinding, waiverFor } from '../core/findings.ts';
import { checkAlwaysVisible, keyboardWalk, measureOverflow } from '../core/layout.ts';
import { compare, update, type Baseline } from '../core/ratchet.ts';
import { runScreenOnly, score, wordMatches } from '../core/screen-only.ts';
import { resolveFile } from '../core/site.ts';
import { assetResults, specPattern } from '../core/specs.ts';
import { scrollContainerPages, scrollContainerLines } from '../core/round.ts';
import { FINDINGS, REPO, SITE_ROOT, UX } from './env.ts';

test.describe('core', () => {
  test('the core imports nothing from the adapter or the UI', () => {
    for (const f of readdirSync(join(UX, 'core'))) {
      const src = readFileSync(join(UX, 'core', f), 'utf8');
      expect(src, f).not.toMatch(/from ['"]\.\.\/(adapter|website)/);
      expect(src, f).not.toMatch(/from ['"][^'"]*\/website\//);
    }
  });

  test('resolveFile maps clean URLs the way the host does', () => {
    expect(resolveFile(SITE_ROOT, '/llm/', '/llm/')).toBe(join(SITE_ROOT, 'index.html'));
    expect(resolveFile(SITE_ROOT, '/llm/', '/llm/docs/reference/crates')).toBe(join(SITE_ROOT, 'docs/reference/crates.html'));
    expect(resolveFile(SITE_ROOT, '/llm/', '/other/')).toBeNull();
    expect(resolveFile(SITE_ROOT, '/llm/', '/llm/../../etc/passwd')).toBeNull();
  });

  test('cue words match inflections, not longer words', () => {
    expect(wordMatches('syscalls', 'syscall')).toBe(true);
    expect(wordMatches('clipboard', 'cli')).toBe(false);
    expect(score('Copy code to clipboard', [{ word: 'cli', weight: 2 }])).toBe(0);
    expect(score('llm CLI', [{ word: 'cli', weight: 2 }, { word: 'llm trace', weight: 3 }])).toBe(2);
    expect(score('llm trace', [{ word: 'llm trace', weight: 3 }])).toBe(3);
  });

  test('glob and waivers: only open findings waive', () => {
    expect(glob('gate2|*|*|*|search:missing', 'gate2|/llm/docs|1366x768|dark|search:missing')).toBe(true);
    expect(glob('gate2|*|*|*|search:missing', 'gate2|/llm/docs|1366x768|dark|sidebar:missing')).toBe(false);
    const open = parseFinding('UXF-900.md', '---\nid: UXF-900\nstatus: open\nwaives: gate5|*|*|*|axe:x, gate1|/a|*|*|page-scroll\n---\n');
    const closed = parseFinding('UXF-901.md', '---\nid: UXF-901\nstatus: closed\nwaives: gate5|*|*|*|axe:y\n---\n');
    expect(waiverFor([open, closed], 'gate1|/a|1280x720|light|page-scroll')).toBe('UXF-900');
    expect(waiverFor([open, closed], 'gate5|/a|1280x720|light|axe:y')).toBeNull();
  });
});

test.describe('ratchet', () => {
  const tol = { counts: 0, steps: '5%' as const, layout_shift: 0.01 };
  const base: Baseline = {
    about: 't',
    tolerances: tol,
    scenarios: {
      a: { 'axe.serious': { value: 2, tolerance: 0 }, 'screen_only.steps': { value: 20, tolerance: '5%' }, 'layout_shift.cls': { value: 0.05, tolerance: 0.01 } },
    },
  };
  test('equal values pass', () => {
    expect(compare(base, { a: { 'axe.serious': 2, 'screen_only.steps': 20, 'layout_shift.cls': 0.05 } }, null).problems).toEqual([]);
  });
  test('worse beyond tolerance fails; within tolerance passes', () => {
    expect(compare(base, { a: { 'axe.serious': 3, 'screen_only.steps': 21, 'layout_shift.cls': 0.059 } }, null).problems).toEqual([
      'worse: a axe.serious 3 > baseline 2 + 0',
    ]);
    expect(compare(base, { a: { 'axe.serious': 2, 'screen_only.steps': 22, 'layout_shift.cls': 0.05 } }, null).problems[0]).toMatch(/^worse: a screen_only.steps/);
  });
  test('an improvement not taken fails', () => {
    expect(compare(base, { a: { 'axe.serious': 1, 'screen_only.steps': 20, 'layout_shift.cls': 0.05 } }, null).problems[0]).toMatch(/^improvement not taken: a axe.serious/);
  });
  test('a missing value fails, both ways', () => {
    const p = compare(base, { a: { 'axe.serious': 2, 'screen_only.steps': 20 }, b: { x: 0 } }, null).problems;
    expect(p).toContain('missing value: a layout_shift.cls (baseline 0.05)');
    expect(p.some((x) => x.startsWith('missing value: b x'))).toBe(true);
  });
  test('a baseline made worse than main needs a decision id', () => {
    const worse: Baseline = structuredClone(base);
    worse.scenarios.a['axe.serious'].value = 4;
    const m = { a: { 'axe.serious': 4, 'screen_only.steps': 20, 'layout_shift.cls': 0.05 } };
    expect(compare(worse, m, base).problems[0]).toMatch(/^baseline made worse without a decision id/);
    worse.scenarios.a['axe.serious'].decision = 'R-02';
    expect(compare(worse, m, base).problems).toEqual([]);
  });
  test('update refuses a worse value without a decision id', () => {
    const m = { a: { 'axe.serious': 3, 'screen_only.steps': 20, 'layout_shift.cls': 0.05 } };
    expect(update(base, m, tol, 't').refused.length).toBe(1);
    expect(update(base, m, tol, 't', 'R-02').baseline.scenarios.a['axe.serious']).toEqual({ value: 3, tolerance: 0, decision: 'R-02' });
  });
});

test.describe('findings', () => {
  const rec = (status: string, sev: string, asset: string) =>
    `---\nid: UXF-001\ntitle: t\nclass: layout\nseverity: ${sev}\nmetric: m\nstatus: ${status}\nfound_by: f\nevidence_recording: none\nevidence_screenshot: none\nevidence_lines: none\nregression_asset: ${asset}\n---\nbody\n`;
  test('a closed S1/S2 record without a regression asset fails', () => {
    expect(findingProblems([parseFinding('UXF-001.md', rec('closed', 'S2', 'none'))], REPO)).toEqual(['UXF-001.md: closed S2 without a regression asset']);
    expect(findingProblems([parseFinding('UXF-001.md', rec('closed', 'S1', 'ux/tests/nope.spec.ts::x'))], REPO)[0]).toMatch(/does not exist/);
    expect(findingProblems([parseFinding('UXF-001.md', rec('closed', 'S1', 'ux/tests/l2-layout.spec.ts::x'))], REPO)).toEqual([]);
    expect(findingProblems([parseFinding('UXF-001.md', rec('closed', 'S3', 'none'))], REPO)).toEqual([]);
    expect(findingProblems([parseFinding('UXF-001.md', rec('open', 'S1', 'none'))], REPO)).toEqual([]);
  });
  test('the findings dir holds well-formed records, none closed S1/S2 without an asset', () => {
    expect(findingProblems(FINDINGS, REPO)).toEqual([]);
  });
});

async function fixture(page: Page, pages: Record<string, string>): Promise<string> {
  await page.route('http://fixture.test/**', (route) => {
    const p = new URL(route.request().url()).pathname;
    const body = pages[p];
    return body ? route.fulfill({ contentType: 'text/html', body }) : route.fulfill({ status: 404, body: 'nf' });
  });
  return 'http://fixture.test';
}

const shell = (main: string, extra = '') =>
  `<html><body style="margin:0"><a href="#m" class="skip">Skip to main content</a><nav aria-label="Main" style="position:fixed;top:0;left:0;right:0;height:40px;background:#eee">` +
  `<a href="/">Home</a> <button aria-label="Switch between dark and light mode">t</button></nav>${extra}<main id="m" tabindex="-1" style="margin-top:50px">${main}</main></body></html>`;

test.describe('break-tests: each layout check can fail', () => {
  test.use({ viewport: { width: 800, height: 600 } });

  test('overflow past the viewport and past the parent fail; a scrolling code block is measured only', async ({ page }) => {
    const base = await fixture(page, {
      '/': shell(
        '<div style="width:300px;white-space:nowrap">' + 'word '.repeat(12) + '</div>' +
          '<pre style="overflow-x:auto;width:200px"><code>' + 'x'.repeat(400) + '</code></pre>' +
          '<p><svg width="1200" height="10"></svg></p>',
      ),
    });
    await page.goto(base + '/');
    const r = await measureOverflow(page, adapter.scrollContainersAllowed);
    expect(r.docScroll).toBeGreaterThan(0);
    expect(r.overflow.map((o) => o.against).sort()).toEqual(['parent', 'viewport']);
    expect(r.scrollContainers.length).toBe(1);
    expect(r.scrollContainers[0].allowed).toBe(true);
  });

  test('a clean page passes the overflow check', async ({ page }) => {
    const base = await fixture(page, { '/': shell('<p>hello</p>') });
    await page.goto(base + '/');
    const r = await measureOverflow(page, adapter.scrollContainersAllowed);
    expect(r).toEqual({ docScroll: 0, overflow: [], scrollContainers: [], clipped: [] });
  });

  test('always-visible: missing, covered and clipped are reported', async ({ page }) => {
    const base = await fixture(page, {
      '/': shell('<p>x</p>', '<div style="position:fixed;top:0;right:0;width:200px;height:40px;background:red"></div>'),
    });
    await page.goto(base + '/');
    const r = await checkAlwaysVisible(page, adapter.alwaysVisible, '/llm/docs/x');
    expect(Object.fromEntries(r.map((v) => [v.name, v.status]))).toEqual({
      navbar: 'ok',
      search: 'missing',
      'theme toggle': 'ok',
      sidebar: 'n/a',
    });
    await page.setViewportSize({ width: 1100, height: 600 });
    expect((await checkAlwaysVisible(page, adapter.alwaysVisible, '/llm/docs/x')).find((v) => v.name === 'sidebar')?.status).toBe('missing');
    await page.setViewportSize({ width: 800, height: 600 });
    await page.addStyleTag({ content: 'button{position:fixed;top:10px;right:20px}' });
    const covered = await checkAlwaysVisible(page, adapter.alwaysVisible, '/llm/');
    expect(covered.find((v) => v.name === 'theme toggle')?.status).toBe('covered');
    await page.addStyleTag({ content: 'button{position:fixed;top:-20px;right:auto;left:0}' });
    const clipped = await checkAlwaysVisible(page, adapter.alwaysVisible, '/llm/');
    expect(clipped.find((v) => v.name === 'theme toggle')?.status).toBe('clipped');
  });

  test('keyboard: a missing skip link and an unreachable target fail', async ({ page }) => {
    const base = await fixture(page, {
      '/': shell('<p>x</p>'),
      '/noskip': shell('<p>x</p>').replace('<a href="#m" class="skip">Skip to main content</a>', ''),
    });
    const targets = adapter.alwaysVisible.filter((a) => ['search', 'navbar'].includes(a.name)).map((a) => ({ name: a.name, locate: a.locate }));
    const ok = await keyboardWalk(page, base + '/', adapter.keyboard.skipLink, targets, 10);
    expect(ok).toEqual({ skipLinkFirst: true, skipMovesFocus: true, reached: { search: false, navbar: true } });
    const bad = await keyboardWalk(page, base + '/noskip', adapter.keyboard.skipLink, targets, 10);
    expect(bad.skipLinkFirst).toBe(false);
  });

  test('screen-only user: follows a visible link, ignores a covered one, and fails when nothing matches', async ({ page }) => {
    const base = await fixture(page, {
      '/': shell('<a href="/decoy">Install guide</a>', '<div style="position:fixed;top:50px;left:0;width:300px;height:40px;background:#000"></div>') .replace('<a href="/">Home</a>', '<a href="/docs">Documentation</a>'),
      '/docs': shell('<a href="/install">Install llm</a>'),
      '/install': shell('<h1 id="install">Install</h1>'),
    });
    const card = { kind: 'task' as const, goal: 'g', persona: 'p', card: 'anchor' as const, start: '/', cues: [{ word: 'install', weight: 3 }] };
    const opts = { base, primary: 'main', hubWords: ['documentation'], maxActions: 6, maxScrolls: 2, settleMs: 0 };
    const r = await runScreenOnly(page, card, { path: '/install', hashes: [] }, opts);
    expect(r.reached).toBe(true);
    expect(r.trace.filter((s) => s.action.startsWith('click')).map((s) => s.action)).toEqual(['click link "Documentation" (hub)', 'click link "Install llm"']);
    const miss = await runScreenOnly(page, { ...card, cues: [{ word: 'kotlin', weight: 3 }] }, { path: '/install', hashes: [] }, { ...opts, hubWords: [] });
    expect(miss.reached).toBe(false);
  });

  test('screen-only user: uses a visible search box when no strong link is in view, in 3 actions', async ({ page }) => {
    const box =
      '<input aria-label="Search" style="position:fixed;top:5px;right:60px;width:150px" oninput="' +
      "document.getElementById('r').innerHTML=this.value?'<div role=option onclick=&quot;location.href=\\'/install\\'&quot;>Install llm</div>':''" +
      '"><div id="r" role="listbox" style="position:fixed;top:40px;right:60px;width:200px;background:#fff"></div>';
    const base = await fixture(page, {
      '/': shell('<p>nothing to follow</p>', box),
      '/install': shell('<h1 id="install">Install</h1>'),
    });
    const card = { kind: 'task' as const, goal: 'g', persona: 'p', card: 'anchor' as const, start: '/', cues: [{ word: 'install', weight: 3 }] };
    const r = await runScreenOnly(page, card, { path: '/install', hashes: [] }, { base, primary: 'main', hubWords: [], maxActions: 6, maxScrolls: 0, settleMs: 100 });
    expect(r.reached).toBe(true);
    expect(r.actions).toBe(3);
    expect(r.trace.map((s) => s.action)).toEqual(['click searchbox "Search"', 'type "install"', 'click option "Install llm" (search)']);
  });
});

// UXF-026: the round runs the regression assets and reports each one, so the next triage can say
// whether the last change set held.
test.describe('round runs the regression assets', () => {
  test('a round runs the L1 specs and l2-findings beside the layout, screen-only and contact-sheet specs', () => {
    for (const f of ['l1-core.spec.ts', 'l2-findings.spec.ts', 'l2-layout.spec.ts', 'l3-screen-only.spec.ts', 'round-shots.spec.ts'])
      expect(specPattern('round').test(f), f).toBe(true);
    expect(specPattern('check').test('round-shots.spec.ts')).toBe(false);
  });

  test('assetResults reads every l1 and l2-findings test from a Playwright report, nested suites included', () => {
    const t = (title: string, status: string) => ({ title, tests: [{ status }] });
    const report = {
      suites: [
        { file: 'l2-layout.spec.ts', specs: [t('L2 page-home 1366x768 light', 'expected')], suites: [] },
        { file: 'l2-findings.spec.ts', specs: [t('UXF-015 a', 'expected'), t('UXF-020 b', 'unexpected')] },
        { file: 'l1-core.spec.ts', specs: [], suites: [{ title: 'core', file: 'l1-core.spec.ts', specs: [t('c', 'expected')] }] },
      ],
    };
    expect(assetResults(report)).toEqual([
      { file: 'l2-findings.spec.ts', title: 'UXF-015 a', ok: true },
      { file: 'l2-findings.spec.ts', title: 'UXF-020 b', ok: false },
      { file: 'l1-core.spec.ts', title: 'core › c', ok: true },
    ]);
  });
});

// UXF-031 (T14): the round names the page of every scroll container at the gating viewports and
// sets it against the last round, so a change set's side effect shows where it lands.
test.describe('round names the pages of its scroll containers', () => {
  const unit = (path: string, viewport: string, theme: string, widths: [number, number][]) => ({
    path,
    viewport,
    theme,
    overflow: { scrollContainers: widths.map(([scrollWidth, clientWidth]) => ({ desc: 'div.b10x-table-wrap', scrollWidth, clientWidth, allowed: false })) },
  });
  const units = [
    unit('/llm/docs/a', '1366x768', 'light', [[900, 768]]),
    unit('/llm/docs/a', '1280x720', 'light', [[900, 703]]),
    unit('/llm/docs/b', '1280x720', 'dark', [[754, 703]]),
    unit('/llm/docs/c', '1366x768', 'light', []),
  ];

  test('scrollContainerPages counts per page, keeps the widest, and reads the last round', () => {
    expect(scrollContainerPages(units, [{ path: '/llm/docs/a', count: 2 }, { path: '/llm/docs/gone', count: 4 }])).toEqual([
      { path: '/llm/docs/a', count: 2, widest: '900 in 703 at 1280x720', last: 2 },
      { path: '/llm/docs/b', count: 1, widest: '754 in 703 at 1280x720', last: 0 },
      { path: '/llm/docs/gone', count: 0, widest: '', last: 4 },
    ]);
    expect(scrollContainerPages(units, null).map((p) => p.last)).toEqual([null, null]);
  });

  test('scrollContainerLines prints one row per page with this round and the last', () => {
    const lines = scrollContainerLines(scrollContainerPages(units, [{ path: '/llm/docs/a', count: 2 }]));
    expect(lines).toEqual([
      '| page | this round | last round | widest (content in box, px) |',
      '|---|---|---|---|',
      '| `/llm/docs/a` | 2 | 2 | 900 in 703 at 1280x720 |',
      '| `/llm/docs/b` | 1 | 0 | 754 in 703 at 1280x720 |',
    ]);
  });
});
