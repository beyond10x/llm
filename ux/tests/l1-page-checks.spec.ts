// L1: the five page checks are the pinned ux-browser's. What is held here: the pin and the Chrome
// build are read from their one place each, the ratchet's five numbers come from the evidence document
// and from nowhere else, a document the ratchet does not read is refused before any comparison, and
// the pinned binary reports each planted defect of a small site and none on its clean page.

import { execFile, spawn, spawnSync } from 'node:child_process';
import { chmodSync, existsSync, linkSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { promisify } from 'node:util';
import { test, expect } from '@playwright/test';
import { adapter } from '../adapter.ts';
import { EVIDENCE_ASSESSOR, EVIDENCE_FORMAT, PAGE_OUTPUTS, pageValues, parseEvidence, readEvidence } from '../core/evidence.ts';
import {
  layoutFailed,
  pageCheckArgs,
  pageCheckFiles,
  pageCheckProblems,
  pageChecksEnded,
  readPageChecks,
  startPageChecks,
  stopPageChecks,
  uxBrowser,
  withPageChecks,
  writePageChecksExit,
  type PageChecks,
} from '../core/page-checks.ts';
import { PIN_WORKFLOW, chromeBuild, pinnedVersion, playwrightChromium } from '../core/pin.ts';
import { compare, type Baseline } from '../core/ratchet.ts';
import { gateCounts, toMeasured, type Failure, type PageResult } from '../core/results.ts';
import { viewportValue } from '../core/round.ts';
import { OUT, REPO, UX } from './env.ts';
import { PAGES, pagePath, writeFixture } from './page-checks-fixture.ts';

const GATING = adapter.viewports.gating.map((v) => v.name);
const KEYS = PAGE_OUTPUTS.map((o) => o.key);
const message = (f: () => unknown): string => {
  try {
    f();
  } catch (e) {
    return (e as Error).message;
  }
  return 'nothing was refused';
};

test.describe('the pin has one place', () => {
  const step = (line: string) => `jobs:\n  ux:\n    steps:\n      - name: Install ux-browser\n        env:\n${line}\n          UX_BROWSER_SHA256: 00aa\n`;

  test('the ux-browser version is the one line of the install step, and any other form is refused', () => {
    expect(pinnedVersion(step('          UX_BROWSER_VERSION: "7.8.9"'))).toBe('7.8.9');
    for (const [text, lines] of [
      [step('          UX_BROWSER_VERSION: 7.8.9'), 0],
      [step("          UX_BROWSER_VERSION: '7.8.9'"), 0],
      [step('          UX_BROWSER_VERSION: "7.8"'), 0],
      [step('          UX_BROWSER_VERSION: "7.8.9-rc1"'), 0],
      [step('          UX_BROWSER_VERSION: >-\n            7.8.9'), 0],
      [step('          UX_BROWSER_SHA256: 00bb'), 0],
      [step('          UX_BROWSER_VERSION: "7.8.9"') + '          UX_BROWSER_VERSION: "7.9.0"\n', 2],
    ] as const)
      expect(message(() => pinnedVersion(text)), text).toBe(`${PIN_WORKFLOW}: ${lines} lines of the form \`UX_BROWSER_VERSION: "<n>.<n>.<n>"\`, needed exactly one`);
  });

  test("this repository's workflow holds a version", () => {
    expect(pinnedVersion(readFileSync(join(REPO, PIN_WORKFLOW), 'utf8'))).toMatch(/^\d+\.\d+\.\d+$/);
  });

  test('the Chrome build is the one line of a checks configuration, and any other form is refused', () => {
    expect(chromeBuild('format: ux-browser-checks/1\nchrome: {build: 1254}\nbase_path: /llm/\n')).toBe(1254);
    for (const [text, lines] of [
      ['format: ux-browser-checks/1\nbase_path: /llm/\n', 0],
      ['chrome: {version: 158}\n', 0],
      ["chrome: {build: '1254'}\n", 0],
      ['chrome:\n  build: 1254\n', 0],
      ['chrome: {build: 0}\n', 0],
      ['chrome: {build: 1254}\nchrome: {build: 1228}\n', 2],
    ] as const)
      expect(message(() => chromeBuild(text)), text).toBe(`${lines} lines of the form \`chrome: {build: <n>}\` in the checks configuration, needed exactly one`);
  });

  test('the Chromium build of Playwright is the revision of its one chromium entry', () => {
    const list = (...bs: object[]) => JSON.stringify({ browsers: bs });
    expect(playwrightChromium(list({ name: 'chromium', revision: '1254' }, { name: 'chromium-headless-shell', revision: '1300' }))).toBe(1254);
    expect(message(() => playwrightChromium(list({ name: 'firefox', revision: '1500' })))).toContain('names 0 chromium revisions');
    expect(message(() => playwrightChromium(list({ name: 'chromium', revision: 'next' })))).toContain('needed exactly one whole number');
  });

  test('both checks configurations pin the Chromium build the installed Playwright carries', () => {
    const installed = playwrightChromium(readFileSync(join(UX, 'node_modules/playwright-core/browsers.json'), 'utf8'));
    for (const config of ['ux-browser.checks.yaml', 'ux-browser.round.checks.yaml']) expect(chromeBuild(readFileSync(join(UX, config), 'utf8')), config).toBe(installed);
  });

  test('a ux-browser that was not handed over as an absolute path is refused', () => {
    expect(uxBrowser({ UX_BROWSER: process.execPath })).toBe(process.execPath);
    expect(message(() => uxBrowser({}))).toMatch(/^UX_BROWSER is unset; the pinned ux-browser is handed over as an absolute path in UX_BROWSER/);
    expect(message(() => uxBrowser({ UX_BROWSER: 'ux-browser' }))).toMatch(/^UX_BROWSER is "ux-browser", which is no absolute path to a file/);
    expect(message(() => uxBrowser({ UX_BROWSER: '/no/such/ux-browser' }))).toMatch(/^UX_BROWSER is "\/no\/such\/ux-browser", which is no absolute path to a file/);
  });
});

/** A document as ux-browser 7.8.9 writes it, for two pages at two gating viewports and one measured-only. */
function evidence(change: (doc: any) => void = () => {}): string {
  const result = (output: string, item: string, viewports: Record<string, string>) => ({ output, kind: 'exact', scope: 'page', item, value_type: 'number', value: '0', details: { viewports } });
  const page = (item: string, v: Record<string, [string, string, string]>) =>
    PAGE_OUTPUTS.map((o) => result(o.output, item, { '1440x900': v[o.key][0], '390x844': v[o.key][1], '768x1024': v[o.key][2] }));
  const doc = {
    format: EVIDENCE_FORMAT,
    evidence: {
      assessor: EVIDENCE_ASSESSOR,
      conditions: { tools: { 'ux-browser': '7.8.9', chromium: 'Chrome/1.2.3.4' } },
      results: [
        ...page('/llm/a', {
          'layout.page_scroll': ['2', '1', '40'],
          'layout.overflow': ['0', '3', '40'],
          'always_visible.misses': ['4', '4', '40'],
          'keyboard.misses': ['2', '0', '40'],
          'layout_shift.after_ready': ['0.0004', '0.02', '0.9'],
        }),
        { output: 'findings', kind: 'exact', scope: 'page', item: '/llm/a', value_type: 'identity_set', items: [] },
        ...page('/llm/b', {
          'layout.page_scroll': ['0', '0', '0'],
          'layout.overflow': ['0', '0', '0'],
          'always_visible.misses': ['0', '0', '0'],
          'keyboard.misses': ['0', '0', '0'],
          'layout_shift.after_ready': ['0.0031', '0', '0'],
        }),
      ],
    },
  };
  change(doc);
  return JSON.stringify(doc);
}

const SCENARIOS = [
  { name: 'page-a', path: '/llm/a' },
  { name: 'page-b', path: '/llm/b' },
];
const FROM_EVIDENCE = {
  'page-a': { 'layout.page_scroll': 3, 'layout.overflow': 3, 'always_visible.misses': 8, 'keyboard.misses': 2, 'layout_shift.after_ready': 0.02 },
  'page-b': { 'layout.page_scroll': 0, 'layout.overflow': 0, 'always_visible.misses': 0, 'keyboard.misses': 0, 'layout_shift.after_ready': 0.0031 },
};

test.describe('the ratchet reads the evidence document', () => {
  test('a scenario gets its five numbers from the results of its page: counts summed over the gating viewports, the shift the largest', () => {
    const read = parseEvidence(evidence(), 'doc', '7.8.9');
    expect(read.tools).toEqual({ uxBrowser: '7.8.9', chromium: 'Chrome/1.2.3.4' });
    expect(pageValues(read, SCENARIOS, GATING)).toEqual(FROM_EVIDENCE);
    // Another choice of gating viewports gives other numbers: nothing is taken from the result's own `value`.
    expect(pageValues(read, SCENARIOS, ['768x1024'])['page-a']).toEqual({ 'layout.page_scroll': 40, 'layout.overflow': 40, 'always_visible.misses': 40, 'keyboard.misses': 40, 'layout_shift.after_ready': 0.9 });
  });

  test('toMeasured takes the five numbers from the evidence and axe from the L2 results, whatever else a result file holds', () => {
    const unit = (scenario: string, path: string, viewport: string, theme: string, serious: number) =>
      ({
        kind: 'page',
        scenario,
        path,
        viewport,
        theme,
        gating: viewport !== '768x1024',
        themeApplied: true,
        scrollContainers: [],
        axe: { counts: { serious }, rules: [] },
        failures: [],
        // What a result file of the harness before held; nothing reads it now.
        overflow: { docScroll: 9, overflow: [{}, {}, {}, {}, {}], scrollContainers: [], clipped: [] },
        visible: { top: [{ name: 'x', status: 'missing' }], bottom: [] },
        keyboard: { skipLinkFirst: false, skipMovesFocus: false, reached: {} },
        shift: { load: 0.5, afterReady: 0.5 },
      }) as unknown as PageResult;
    const results = [unit('page-a', '/llm/a', '1440x900', 'light', 1), unit('page-a', '/llm/a', '390x844', 'dark', 2), unit('page-a', '/llm/a', '768x1024', 'dark', 50), unit('page-b', '/llm/b', '1440x900', 'light', 0)];
    const axe = (serious: number) => ({ 'axe.critical': 0, 'axe.serious': serious, 'axe.moderate': 0, 'axe.minor': 0 });
    expect(toMeasured(results, FROM_EVIDENCE)).toEqual({ 'page-a': { ...FROM_EVIDENCE['page-a'], ...axe(3) }, 'page-b': { ...FROM_EVIDENCE['page-b'], ...axe(0) } });
    // Without a page's evidence its five numbers are missing, and the ratchet reports each as a missing value.
    const none = toMeasured(results, { 'page-b': FROM_EVIDENCE['page-b'] });
    expect(Object.keys(none['page-a']).sort()).toEqual(['axe.critical', 'axe.minor', 'axe.moderate', 'axe.serious']);
  });

  test('a document that is not the evidence of the pinned ux-browser is refused with what was found and what is needed', () => {
    for (const [text, why] of [
      ['<html>', 'doc: not JSON ('],
      [evidence((d) => (d.format = 'ratchet-evidence/2')), 'doc: format is "ratchet-evidence/2", the ratchet reads "ratchet-evidence/1"'],
      [JSON.stringify({ values: {}, units: [] }), 'doc: format is absent, the ratchet reads "ratchet-evidence/1"'],
      [evidence((d) => (d.evidence.assessor = 'ux-static@1')), 'doc: evidence.assessor is "ux-static@1", needed "ux-browser@1"'],
      [evidence((d) => (d.evidence.conditions.tools['ux-browser'] = '7.8.8')), 'doc: written by ux-browser "7.8.8", the pinned release is "7.8.9"'],
      [evidence((d) => delete d.evidence.conditions), 'doc: written by ux-browser absent, the pinned release is "7.8.9"'],
      [evidence((d) => (d.evidence.results = {})), 'doc: evidence.results is {}, needed a list'],
      [evidence((d) => (d.evidence.results = [])), 'doc: no result of layout.page_scroll@1, layout.overflow@1'],
      [
        evidence((d) => Object.assign(d.evidence.results[1], { incomplete: { cause: 'incompatible_target', detail: 'the page was not served' }, details: undefined })),
        'doc: layout.overflow@1 of "/llm/a" is incomplete (incompatible_target: the page was not served)',
      ],
      [evidence((d) => delete d.evidence.results[0].details), 'doc: layout.page_scroll@1 of "/llm/a": details.viewports is absent, needed a value per viewport'],
      [evidence((d) => (d.evidence.results[4].details.viewports['390x844'] = 0.02)), 'doc: layout_shift.after_ready@1 of "/llm/a": the value at 390x844 is 0.02, needed a decimal number as text'],
      [evidence((d) => d.evidence.results.splice(3, 1)), 'doc: page /llm/a has no result of keyboard.misses@1'],
      [evidence((d) => d.evidence.results.push(d.evidence.results[0])), 'doc: layout.page_scroll@1 of "/llm/a": a second result of that output for the page'],
    ] as const)
      expect(message(() => parseEvidence(text, 'doc', '7.8.9')), why).toContain(why);
    // The same document under another pin is another ux-browser's.
    expect(message(() => parseEvidence(evidence(), 'doc', '7.9.0'))).toContain('written by ux-browser "7.8.9", the pinned release is "7.9.0"');
  });

  test('a page without a scenario, a scenario without a page and a gating viewport without a value are refused', () => {
    const read = parseEvidence(evidence(), 'doc', '7.8.9');
    expect(message(() => pageValues(read, SCENARIOS.slice(0, 1), GATING))).toBe('page /llm/b of the evidence has no page scenario; run `task ux-scenarios`');
    expect(message(() => pageValues(read, [...SCENARIOS, { name: 'page-c', path: '/llm/c' }], GATING))).toBe('page scenario page-c (/llm/c) has no result in the evidence');
    expect(message(() => pageValues(read, SCENARIOS, ['1440x900', '1280x800']))).toBe(
      'page /llm/a has no layout.page_scroll@1 at the gating viewport 1280x800; the evidence has 1440x900, 390x844, 768x1024',
    );
  });

  test('`cli.ts ratchet` refuses a missing document, another format and another ux-browser before it compares anything', () => {
    const pinned = pinnedVersion(readFileSync(join(REPO, PIN_WORKFLOW), 'utf8'));
    const out = join(OUT, 'l1-ratchet');
    const file = pageCheckFiles(out).evidence;
    const ratchet = () => spawnSync(process.execPath, ['cli.ts', 'ratchet', '--out', out], { cwd: UX, encoding: 'utf8' });
    rmSync(out, { recursive: true, force: true });
    mkdirSync(out, { recursive: true });
    expect(message(() => readEvidence(file, pinned))).toBe(`${file}: no such file; the ratchet needs the ratchet-evidence/1 document ux-browser ${pinned} writes there, run \`task ux-check\` first`);
    for (const [text, why] of [
      [null, `ratchet: refused: ${file}: no such file; the ratchet needs the ratchet-evidence/1 document ux-browser ${pinned} writes there`],
      [JSON.stringify({ values: {}, units: [] }), `ratchet: refused: ${file}: format is absent, the ratchet reads "ratchet-evidence/1"`],
      [evidence(), `ratchet: refused: ${file}: written by ux-browser "7.8.9", the pinned release is "${pinned}"`],
    ] as const) {
      if (text !== null) writeFileSync(file, text);
      const run = ratchet();
      expect(run.status, why).toBe(1);
      expect(run.stderr, why).toContain(why);
      expect(run.stderr.trim().split('\n'), 'the refusal is all it says').toHaveLength(1);
      expect(run.stdout, 'nothing was compared').toBe('');
    }
  });
});

test.describe("the gate and the round read ux-browser's values file", () => {
  const failure = (gate: Failure['gate'], path: string, viewport: string, theme: string, detail: string, waivedBy: string | null = null): Failure => ({
    gate,
    key: `gate${gate}|${path}|${viewport}|${theme}|${detail}`,
    detail,
    waivedBy,
  });
  const checks: PageChecks = {
    values: {},
    units: [
      { path: '/llm/a', viewport: '390x844', theme: 'light', failures: [failure(1, '/llm/a', '390x844', 'light', 'page-scroll'), failure(2, '/llm/a', '390x844', 'light', 'search:missing', 'UXF-900')] },
      { path: '/llm/a', viewport: '1440x900', theme: 'dark', failures: [failure(5, '/llm/a', '1440x900', 'dark', 'keyboard:skip-link-first', 'UXF-901')] },
    ],
    incomplete: ['/llm/b layout.page_scroll@1: incompatible_target'],
    unmapped: [],
    viewports: { '1440x900': { gating: true, pages: 2, 'layout.overflow': 0, 'layout_shift.load': 0.0312 }, '768x1024': { gating: false, pages: 2, 'layout.overflow': 3 } },
  };
  const unit = (viewport: string, theme: string, failures: Failure[] = []): PageResult => ({
    kind: 'page',
    scenario: 'page-a',
    path: '/llm/a',
    viewport,
    theme,
    gating: true,
    themeApplied: true,
    scrollContainers: [],
    axe: { counts: {}, rules: [] },
    failures,
  });

  test('a unit gets the page checks failures of its page, viewport and theme beside its own, and the gate counts follow', () => {
    const axe = failure(5, '/llm/a', '390x844', 'light', 'axe:color-contrast');
    const merged = withPageChecks([unit('390x844', 'light', [axe]), unit('1440x900', 'dark'), unit('1440x900', 'light')], checks);
    expect(merged.map((r) => r.failures.map((f) => f.detail))).toEqual([['axe:color-contrast', 'page-scroll', 'search:missing'], ['keyboard:skip-link-first'], []]);
    expect(gateCounts(merged)).toEqual({
      gate1: { checked: 3, pass: 2, fail: 1, waived: 0 },
      gate2: { checked: 3, pass: 2, fail: 0, waived: 1 },
      gate5: { checked: 3, pass: 1, fail: 1, waived: 1 },
    });
    expect([...layoutFailed(checks)], 'a keyboard failure is no layout failure').toEqual(['/llm/a|390x844|light']);
  });

  test('what a run left that is no value is named: unwaived failures, waived ones, incomplete and unmapped entries', () => {
    expect(pageCheckProblems(checks)).toEqual({
      unwaived: ['gate1|/llm/a|390x844|light|page-scroll'],
      waived: ['gate2|/llm/a|390x844|light|search:missing by UXF-900', 'gate5|/llm/a|1440x900|dark|keyboard:skip-link-first by UXF-901'],
      incomplete: ['/llm/b layout.page_scroll@1: incompatible_target'],
      unmapped: [],
    });
  });

  test('a number per viewport is read from `viewports`, and one the file does not have is an error, never 0', () => {
    expect(viewportValue(checks, '1440x900', 'layout_shift.load')).toBe(0.0312);
    expect(viewportValue(checks, '768x1024', 'layout.overflow')).toBe(3);
    expect(message(() => viewportValue(checks, '1280x800', 'layout.overflow'))).toBe('the values file has no viewport 1280x800; it has 1440x900, 768x1024');
    expect(message(() => viewportValue(checks, '768x1024', 'layout_shift.load'))).toBe('the values file has no number layout_shift.load at 768x1024; it has gating, pages, layout.overflow');
    expect(message(() => viewportValue({}, '1440x900', 'layout.overflow'))).toBe('the values file has no viewport 1440x900; it has none');
  });

  test('a file that is not a values file of ux-browser is refused', () => {
    const dir = join(OUT, 'l1-values');
    rmSync(dir, { recursive: true, force: true });
    mkdirSync(dir, { recursive: true });
    const file = join(dir, 'page-checks.json');
    expect(message(() => readPageChecks(file))).toBe(`${file}: no such file; ux-browser writes its values file there`);
    writeFileSync(file, JSON.stringify({ values: {}, units: [], incomplete: [] }));
    expect(message(() => readPageChecks(file))).toBe(`${file}: no list \`unmapped\`; it is not a values file of ux-browser`);
    writeFileSync(file, evidence());
    expect(message(() => readPageChecks(file))).toBe(`${file}: no list \`units\`; it is not a values file of ux-browser`);
    writeFileSync(file, JSON.stringify(checks));
    expect(readPageChecks(file)).toEqual(checks);
  });
});

test.describe('the pinned ux-browser reports each planted defect', () => {
  test('one page per defect goes red by its key or its value, and the clean page stays green', async () => {
    const pinned = pinnedVersion(readFileSync(join(REPO, PIN_WORKFLOW), 'utf8'));
    const root = join(OUT, 'l1-page-checks');
    const site = writeFixture(root, join(UX, 'ux-browser.checks.yaml'));
    const files = pageCheckFiles(join(root, 'out'));
    mkdirSync(join(root, 'out'), { recursive: true });
    const run = { config: join(root, 'ux/ux-browser.checks.yaml'), build: site, root, project: 'llm-docs', assessment: 'l1-page-checks', jobs: 2 };
    const ended = await promisify(execFile)(uxBrowser(process.env), pageCheckArgs(run, files), { timeout: 50_000 }).then(
      (ok) => ({ code: 0 as number | string | null | undefined, text: ok.stdout + ok.stderr }),
      (e: { code?: number | string | null; stdout?: string; stderr?: string }) => ({ code: e.code, text: `${e.stdout}${e.stderr}` }),
    );
    expect(ended.code, `ux-browser reports a problem with exit 1:\n${ended.text}`).toBe(1);

    const checks = readPageChecks(files.values);
    expect(checks.incomplete, 'every page was measured').toEqual([]);
    expect(checks.unmapped, 'every page has its scenario').toEqual([]);
    const keys = checks.units.flatMap((u) => u.failures.map((f) => `${f.key}${f.waivedBy ? ` waived by ${f.waivedBy}` : ''}`)).sort();
    const units = (gate: number, name: string, detail: string, themes = adapter.themes) => GATING.flatMap((vp) => themes.map((t) => `gate${gate}|${pagePath(name)}|${vp}|${t}|${detail}`));
    expect(keys).toEqual(
      [
        ...units(1, 'page-scroll', 'page-scroll'),
        ...units(1, 'overflow', 'overflow:div#narrow "a line that does not wrap here"'),
        ...units(2, 'missing', 'search:missing'),
        ...units(5, 'missing', 'keyboard:tab-reaches-search', ['dark']),
        ...units(5, 'keyboard', 'keyboard:skip-link-first', ['dark']),
      ].sort(),
    );

    const scenarios = Object.keys(PAGES).map((name) => ({ name: `page-${name}`, path: pagePath(name) }));
    const read = readEvidence(files.evidence, pinned);
    const values = pageValues(read, scenarios, GATING);
    expect(values, "the ratchet's numbers are the ones ux-browser's own values file has").toEqual(checks.values);
    const zero = Object.fromEntries(KEYS.map((k) => [k, 0]));
    const shift = values['page-shift']['layout_shift.after_ready'];
    expect(shift, 'content that arrives after ready is a layout shift').toBeGreaterThan(0.01);
    expect(values).toEqual({
      'page-clean': zero,
      'page-page-scroll': { ...zero, 'layout.page_scroll': 4 },
      'page-overflow': { ...zero, 'layout.overflow': 4 },
      'page-missing': { ...zero, 'always_visible.misses': 8, 'keyboard.misses': 2 },
      'page-keyboard': { ...zero, 'keyboard.misses': 4 },
      'page-shift': { ...zero, 'layout_shift.after_ready': shift },
    });

    // Against a baseline of zeros the ratchet names every planted value as worse, the layout shift among them.
    const tolerance = (k: string) => (k.startsWith('layout_shift') ? 0.01 : 0);
    const baseline: Baseline = {
      about: 'the fixture before the defects',
      tolerances: { counts: 0, layout_shift: 0.01 },
      scenarios: Object.fromEntries(scenarios.map((s) => [s.name, Object.fromEntries(KEYS.map((k) => [k, { value: 0, tolerance: tolerance(k) }]))])),
    };
    expect(compare(baseline, toMeasured([], values), null).worse.sort()).toEqual(
      ['page-keyboard keyboard.misses', 'page-missing always_visible.misses', 'page-missing keyboard.misses', 'page-overflow layout.overflow', 'page-page-scroll layout.page_scroll', 'page-shift layout_shift.after_ready'].sort(),
    );
  });
});

// The run of ux-browser beside the specs. What is held from here on: what the harness takes as the
// `UX_BROWSER` it was handed, that a reader of the exit file finds a whole exit status or none, what
// `startPageChecks` removes and leaves, that `stopPageChecks` ends the whole process group, and that
// the global setup hands the worker count on as `--jobs` and ends ux-browser in its teardown, on an
// exit without one, on SIGTERM and on SIGHUP. The cases that start something run a shell script in
// the place of ux-browser; nothing is fetched and no page is measured.

/** A script in the place of ux-browser under `dir`: it notes its arguments, then does `body`. */
function standIn(dir: string, body: string): string {
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  const file = join(dir, 'stand-in');
  writeFileSync(file, `#!/bin/sh\nprintf '%s\\n' "$@" > "${dir}/args"\n${body}\n`);
  chmodSync(file, 0o755);
  return file;
}

/** A stand-in that starts a process of its own, notes both process ids in `pids`, and waits. */
const sleeper = (dir: string) => standIn(dir, `sleep 120 &\necho "$$ $!" > "${dir}/pids.tmp" && mv "${dir}/pids.tmp" "${dir}/pids"\nwait`);

/** Whether a process runs: it has an entry under /proc and is no zombie. */
const runs = (pid: number): boolean => {
  try {
    return readFileSync(`/proc/${pid}/stat`, 'utf8').split(') ').pop()![0] !== 'Z';
  } catch {
    return false;
  }
};

const STAND_IN_RUN = { config: '/fixture/checks.yaml', build: '/fixture/site', root: '/fixture', project: 'fixture', assessment: 'stand-in', jobs: 3 };

test('UX_BROWSER that names a directory is refused, as "an absolute path to a file" says', () => {
  expect(message(() => uxBrowser({ UX_BROWSER: UX }))).toMatch(/^UX_BROWSER is ".*", which is no absolute path to a file/);
});

test('a relative UX_BROWSER is refused also when it names a file that is there', () => {
  // The specs run with `ux/` as the working directory, where `package.json` is.
  expect(existsSync('package.json'), 'the working directory is ux/').toBe(true);
  expect(message(() => uxBrowser({ UX_BROWSER: 'package.json' }))).toMatch(/^UX_BROWSER is "package.json", which is no absolute path to a file/);
});

test('an exit file seen between its creation and its content is not read as the end of the run', async () => {
  const dir = join(OUT, 'l1-exit');
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  const files = pageCheckFiles(dir);
  // What a reader in another process sees between the open and the write of `writeFileSync`.
  writeFileSync(files.exit, '');
  const written = setTimeout(() => writeFileSync(files.exit, JSON.stringify({ code: 0, signal: null }) + '\n'), 300);
  try {
    expect(await pageChecksEnded(files, 5_000).catch((e: Error) => `thrown: ${e.message}`)).toEqual({ code: 0, signal: null });
  } finally {
    clearTimeout(written);
  }
});

test('a symlink to a file is taken as the file it names; one to a directory and one that leads nowhere are refused', () => {
  const dir = join(OUT, 'l1-handed');
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(join(dir, 'directory'), { recursive: true });
  writeFileSync(join(dir, 'file'), '');
  const link = (to: string): string => {
    const name = join(dir, `to-${to}`);
    symlinkSync(join(dir, to), name);
    return name;
  };
  expect(uxBrowser({ UX_BROWSER: link('file') })).toBe(join(dir, 'to-file'));
  for (const to of ['directory', 'nothing'])
    expect(message(() => uxBrowser({ UX_BROWSER: link(to) })), to).toBe(
      `UX_BROWSER is ${JSON.stringify(join(dir, `to-${to}`))}, which is no absolute path to a file; the pinned ux-browser is handed over as an absolute path in UX_BROWSER: ` +
        'run the gate as `task ux-check` (a round as `task ux-round`), or set it to what `cargo run --locked -p llm-conformance -- ux-browser` prints at the repository root',
    );
});

test('the exit status is renamed into place: the file a reader holds open is not written to, and no temporary file is left', () => {
  const dir = join(OUT, 'l1-exit-renamed');
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  const files = pageCheckFiles(dir);
  // A second name of the file that is at the exit file's place: a write into that file shows under
  // this name too, a rename of another file over the place does not.
  const before = join(dir, 'the-file-that-was-there');
  writeFileSync(files.exit, 'left by a run before');
  linkSync(files.exit, before);
  writePageChecksExit(files, { code: 1, signal: null });
  expect(readFileSync(files.exit, 'utf8')).toBe('{"code":1,"signal":null}\n');
  expect(readFileSync(before, 'utf8'), 'the file that was there').toBe('left by a run before');
  expect(existsSync(files.exit + '.tmp'), 'the temporary file').toBe(false);
});

test('an exit file that holds no exit status at the deadline is refused with what it holds, and a missing one is no end', async () => {
  const dir = join(OUT, 'l1-exit-partial');
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  const files = pageCheckFiles(dir);
  expect(await pageChecksEnded(files, 0)).toBeNull();
  for (const [text, shown] of [
    ['{"code":0,', '"{\\"code\\":0,"'],
    ['', '""'],
    ['{"signal":null}\n', '"{\\"signal\\":null}\\n"'],
  ]) {
    writeFileSync(files.exit, text);
    expect(await pageChecksEnded(files, 250).catch((e: Error) => e.message)).toBe(
      `${files.exit}: holds ${shown} after 250 ms, which is no exit status; needed is {"code", "signal"} as a run writes it when it ends`,
    );
  }
  writePageChecksExit(files, { code: null, signal: 'SIGKILL' });
  expect(await pageChecksEnded(files, 0)).toEqual({ code: null, signal: 'SIGKILL' });
});

test('startPageChecks removes the temporary exit file a run before left, and leaves none', async () => {
  const dir = join(OUT, 'l1-start-draft');
  const binary = standIn(dir, 'exit 3');
  const files = pageCheckFiles(join(dir, 'out'));
  mkdirSync(join(dir, 'out'), { recursive: true });
  writeFileSync(files.exit + '.tmp', '{"code":0,');
  const child = startPageChecks(binary, STAND_IN_RUN, files);
  try {
    // The stand-in has not ended yet when `startPageChecks` returns: its end is an event of a later turn.
    expect(existsSync(files.exit + '.tmp'), 'left by the run before').toBe(false);
    expect(await pageChecksEnded(files, 20_000)).toEqual({ code: 3, signal: null });
    expect(existsSync(files.exit + '.tmp'), 'left by this run').toBe(false);
  } finally {
    stopPageChecks(child);
  }
});

test('startPageChecks removes what a run before left, starts the binary with the arguments of the run, and leaves its exit status', async () => {
  const dir = join(OUT, 'l1-start');
  const binary = standIn(dir, 'exit 0');
  const files = pageCheckFiles(join(dir, 'out'));
  mkdirSync(files.profile, { recursive: true });
  for (const f of [files.evidence, files.values, files.log, files.exit, join(files.profile, 'left')]) writeFileSync(f, 'left by a run before');
  const child = startPageChecks(binary, STAND_IN_RUN, files);
  try {
    expect([files.evidence, files.values, files.exit, join(files.profile, 'left')].filter((f) => existsSync(f)), 'left by the run before').toEqual([]);
    expect(await pageChecksEnded(files, 20_000)).toEqual({ code: 0, signal: null });
    const args = readFileSync(join(dir, 'args'), 'utf8').trimEnd().split('\n');
    expect(args).toEqual(pageCheckArgs(STAND_IN_RUN, files));
    expect(args.slice(args.indexOf('--jobs'), args.indexOf('--jobs') + 2)).toEqual(['--jobs', '3']);
    expect(readFileSync(files.log, 'utf8'), 'the log is this run\'s').toBe('');
    expect(existsSync(files.evidence), 'the stand-in wrote no evidence, and none of a run before is found').toBe(false);
  } finally {
    stopPageChecks(child);
  }
});

test('stopPageChecks ends ux-browser and what it started, and the exit file names the signal', async () => {
  const dir = join(OUT, 'l1-stop');
  const binary = sleeper(dir);
  const files = pageCheckFiles(join(dir, 'out'));
  const child = startPageChecks(binary, STAND_IN_RUN, files);
  let pids: number[] = [];
  try {
    await expect.poll(() => existsSync(join(dir, 'pids')), { timeout: 20_000 }).toBe(true);
    pids = readFileSync(join(dir, 'pids'), 'utf8').trim().split(' ').map(Number);
    expect(pids[0], 'ux-browser leads its process group').toBe(child.pid);
    expect(pids.map(runs)).toEqual([true, true]);
    expect(await pageChecksEnded(files, 300), 'a run that has not ended has no exit status').toBeNull();
    stopPageChecks(child);
    expect(await pageChecksEnded(files, 20_000)).toEqual({ code: null, signal: 'SIGKILL' });
    await expect.poll(() => pids.map(runs), { timeout: 10_000 }).toEqual([false, false]);
  } finally {
    for (const pid of pids) {
      try {
        process.kill(pid, 'SIGKILL');
      } catch {
        // Gone already.
      }
    }
  }
});

/** What a process runs to stand in for a Playwright run: the global setup, then it waits for a signal. */
const SETUP_RUN =
  "const { default: setup } = await import('./tests/global-setup.ts'); const teardown = await setup({ workers: 3 });" +
  "process.on('SIGUSR1', async () => { await teardown(); console.log('torn down'); setInterval(() => {}, 1000); });" +
  "process.on('SIGUSR2', () => process.exit(1)); console.log('started');";

test('the global setup hands the worker count on as --jobs, and the teardown, an exit without one, SIGTERM and SIGHUP each end ux-browser with what it started', async () => {
  for (const [how, signal, exit] of [
    ['the teardown', 'SIGUSR1', null],
    ['an exit without a teardown', 'SIGUSR2', [1, null]],
    ['SIGTERM', 'SIGTERM', [null, 'SIGTERM']],
    ['SIGHUP', 'SIGHUP', [null, 'SIGHUP']],
  ] as const) {
    const dir = join(OUT, `l1-setup-${signal}`);
    const binary = sleeper(dir);
    const env = { ...process.env, UX_BROWSER: binary, UX_OUT: relative(UX, join(dir, 'out')), UX_MODE: '' };
    const run = spawn(process.execPath, ['--input-type=module', '-e', SETUP_RUN], { cwd: UX, env, stdio: ['ignore', 'pipe', 'pipe'] });
    let said = '';
    run.stdout.on('data', (d) => (said += d));
    run.stderr.on('data', (d) => (said += d));
    const ended = new Promise<[number | null, string | null]>((ok) => run.once('exit', (c, s) => ok([c, s])));
    let pids: number[] = [];
    try {
      await expect.poll(() => said.includes('started') || run.exitCode !== null, { timeout: 30_000 }).toBe(true);
      expect(said, 'the global setup returned').toContain('started');
      await expect.poll(() => existsSync(join(dir, 'pids')), { timeout: 20_000 }).toBe(true);
      pids = readFileSync(join(dir, 'pids'), 'utf8').trim().split(' ').map(Number);
      const args = readFileSync(join(dir, 'args'), 'utf8').trimEnd().split('\n');
      expect(args.slice(args.indexOf('--jobs'), args.indexOf('--jobs') + 2), 'the worker count of the run').toEqual(['--jobs', '3']);
      expect(pids.map(runs)).toEqual([true, true]);
      run.kill(signal);
      if (exit) expect(await ended, `${how}: how the run ends, after it said: ${said}`).toEqual(exit);
      else await expect.poll(() => said.includes('torn down') || run.exitCode !== null, { timeout: 20_000, message: `${how} returns` }).toBe(true);
      await expect.poll(() => pids.map(runs), { timeout: 10_000, message: `after ${how}, ux-browser and what it started are gone` }).toEqual([false, false]);
    } finally {
      run.kill('SIGKILL');
      for (const pid of pids) {
        try {
          process.kill(pid, 'SIGKILL');
        } catch {
          // Gone already.
        }
      }
    }
  }
});

test('a run ended by SIGKILL ends nothing: the ux-browser it started and what that started run on, and no exit status is written', async () => {
  const dir = join(OUT, 'l1-setup-SIGKILL');
  const binary = sleeper(dir);
  const env = { ...process.env, UX_BROWSER: binary, UX_OUT: relative(UX, join(dir, 'out')), UX_MODE: '' };
  const run = spawn(process.execPath, ['--input-type=module', '-e', SETUP_RUN], { cwd: UX, env, stdio: ['ignore', 'pipe', 'pipe'] });
  let said = '';
  run.stdout.on('data', (d) => (said += d));
  run.stderr.on('data', (d) => (said += d));
  const ended = new Promise<[number | null, string | null]>((ok) => run.once('exit', (c, s) => ok([c, s])));
  let pids: number[] = [];
  try {
    await expect.poll(() => said.includes('started') || run.exitCode !== null, { timeout: 30_000 }).toBe(true);
    expect(said, 'the global setup returned').toContain('started');
    await expect.poll(() => existsSync(join(dir, 'pids')), { timeout: 20_000 }).toBe(true);
    pids = readFileSync(join(dir, 'pids'), 'utf8').trim().split(' ').map(Number);
    run.kill('SIGKILL');
    expect(await ended, `how the run ends, after it said: ${said}`).toEqual([null, 'SIGKILL']);
    expect(pids.map(runs), 'ux-browser and what it started').toEqual([true, true]);
    expect(existsSync(pageCheckFiles(join(dir, 'out')).exit), 'an exit status of ux-browser').toBe(false);
  } finally {
    run.kill('SIGKILL');
    for (const pid of pids) {
      try {
        process.kill(pid, 'SIGKILL');
      } catch {
        // Gone already.
      }
    }
  }
});
