// The Measure step's output (SKILL.md § 3.2): report.md, round.json, shots/ and ../trend.md, from the
// round run's result files. Deterministic: dates come from the commit, not the clock.

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { basename, join } from 'node:path';
import type { Adapter } from './types.ts';
import type { Baseline } from './ratchet.ts';
import { gateCounts, readResults, toMeasured, type PageResult, type TaskResult } from './results.ts';
import { loadScenarios } from './scenarios.ts';
import { loadFindings } from './findings.ts';
import { assetResults } from './specs.ts';

export function nextRound(roundsDir: string): string {
  const n = existsSync(roundsDir)
    ? readdirSync(roundsDir)
        .map((d) => /^R-(\d+)$/.exec(d)?.[1])
        .filter(Boolean)
        .map(Number)
    : [];
  return `R-${String((n.length ? Math.max(...n) : 0) + 1).padStart(2, '0')}`;
}

const sum = (xs: number[]) => xs.reduce((a, b) => a + b, 0);

export interface ScrollPage {
  path: string;
  count: number;
  widest: string;
  last: number | null;
}

type ScrollUnit = Pick<PageResult, 'path' | 'viewport'> & { overflow: Pick<PageResult['overflow'], 'scrollContainers'> };

/** Scroll containers per page over the given units, the widest of each page, and the last round's
 * count (0 for a page it did not list; null without a last round). Pages of the last round that
 * have none now stay listed with 0, so a fix shows too. */
export function scrollContainerPages(units: ScrollUnit[], last: { path: string; count: number }[] | null): ScrollPage[] {
  const by = new Map<string, { count: number; widest: string; over: number }>();
  for (const u of units) {
    for (const c of u.overflow.scrollContainers) {
      const e = by.get(u.path) ?? { count: 0, widest: '', over: -1 };
      e.count++;
      if (c.scrollWidth - c.clientWidth > e.over) {
        e.over = c.scrollWidth - c.clientWidth;
        e.widest = `${c.scrollWidth} in ${c.clientWidth} at ${u.viewport}`;
      }
      by.set(u.path, e);
    }
  }
  for (const l of last ?? []) if (!by.has(l.path)) by.set(l.path, { count: 0, widest: '', over: -1 });
  return [...by.entries()]
    .map(([path, e]) => ({ path, count: e.count, widest: e.widest, last: last ? (last.find((l) => l.path === path)?.count ?? 0) : null }))
    .sort((a, b) => b.count - a.count || a.path.localeCompare(b.path));
}

export function scrollContainerLines(pages: ScrollPage[]): string[] {
  return [
    '| page | this round | last round | widest (content in box, px) |',
    '|---|---|---|---|',
    ...pages.map((p) => `| \`${p.path}\` | ${p.count} | ${p.last ?? '–'} | ${p.widest || '–'} |`),
  ];
}

export interface RoundInput {
  adapter: Adapter;
  ux: string;
  repo: string;
  id: string;
  dir: string;
  out: string;
  baselineFile: string;
}

function git(repo: string, ...a: string[]): string {
  try {
    return execFileSync('git', a, { cwd: repo, encoding: 'utf8' }).trim();
  } catch {
    return 'unknown';
  }
}

export function writeRound(i: RoundInput): void {
  const { adapter, id, dir } = i;
  mkdirSync(join(dir, 'shots'), { recursive: true });
  const results = readResults(i.out);
  const pages = results.filter((r): r is PageResult => r.kind === 'page');
  const gating = pages.filter((r) => r.gating);
  const tasks = results.filter((r): r is TaskResult => r.kind === 'task');
  const measured = toMeasured(results);
  const gates = gateCounts(results);
  const baseline: Baseline | null = existsSync(i.baselineFile) ? JSON.parse(readFileSync(i.baselineFile, 'utf8')) : null;
  const roundsDir = join(dir, '..');
  const prevId = readdirSync(roundsDir)
    .filter((d) => /^R-\d+$/.test(d) && d < id && existsSync(join(roundsDir, d, 'round.json')))
    .sort()
    .pop();
  const prev = prevId ? JSON.parse(readFileSync(join(roundsDir, prevId, 'round.json'), 'utf8')) : null;
  const commit = git(i.repo, 'rev-parse', '--short', 'HEAD');
  const date = git(i.repo, 'log', '-1', '--format=%cs');
  const scenarios = loadScenarios(join(i.ux, adapter.dirs.scenarios));
  const taskCards = scenarios.filter((s) => s.task);
  const anchors = taskCards.filter((s) => s.task!.card.card === 'anchor').map((s) => s.name);
  const rotating = taskCards.filter((s) => s.task!.card.card === 'rotating').map((s) => s.name);
  const roundNo = Number(id.slice(2));
  const rotatingCard = rotating.length ? rotating[(roundNo - 1) % rotating.length] : null;
  const findings = loadFindings(join(i.ux, adapter.dirs.findings));
  const pwFile = join(i.out, 'playwright.json');
  const assets = existsSync(pwFile) ? assetResults(JSON.parse(readFileSync(pwFile, 'utf8'))) : [];

  // Worst moments: gating results with the most failures, then the most serious/critical axe nodes.
  const worst = gating
    .filter((r) => r.failures.length)
    .map((r) => ({ r, w: r.failures.length * 100 + (r.axe.counts.serious ?? 0) + (r.axe.counts.critical ?? 0) * 2 }))
    .sort((a, b) => b.w - a.w || a.r.scenario.localeCompare(b.r.scenario) || a.r.viewport.localeCompare(b.r.viewport) || a.r.theme.localeCompare(b.r.theme))
    .slice(0, 8);
  const worstOut = worst.map(({ r }) => {
    let shot: string | null = null;
    if (r.shot && existsSync(r.shot)) {
      shot = `shots/${basename(r.shot)}`;
      copyFileSync(r.shot, join(dir, shot));
    }
    return { scenario: r.scenario, path: r.path, viewport: r.viewport, theme: r.theme, failures: r.failures.map((f) => f.key), shot };
  });

  const totals = (key: string) => sum(Object.entries(measured).filter(([s]) => s.startsWith('page-')).map(([, v]) => v[key] ?? 0));
  const pageKeys = ['layout.page_scroll', 'layout.overflow', 'always_visible.misses', 'keyboard.misses', 'axe.critical', 'axe.serious', 'axe.moderate', 'axe.minor'];
  const baseTotal = (key: string) =>
    baseline ? sum(Object.entries(baseline.scenarios).filter(([s]) => s.startsWith('page-')).map(([, v]) => v[key]?.value ?? 0)) : null;
  const maxLoad = Math.max(0, ...gating.map((r) => r.shift.load));
  const maxReady = Math.max(0, ...gating.map((r) => r.shift.afterReady));

  const measuredOnly = adapter.viewports.measured.map((vp) => {
    const rs = pages.filter((r) => r.viewport === vp.name);
    return {
      viewport: vp.name,
      checked: rs.length,
      page_scroll: rs.filter((r) => r.overflow.docScroll > 0).length,
      overflow: sum(rs.map((r) => r.overflow.overflow.length)),
      always_visible_misses: sum(rs.map((r) => [...r.visible.top, ...r.visible.bottom].filter((v) => v.status !== 'ok' && v.status !== 'n/a').length)),
      axe_serious_critical: sum(rs.map((r) => (r.axe.counts.serious ?? 0) + (r.axe.counts.critical ?? 0))),
      scroll_containers: sum(rs.map((r) => r.overflow.scrollContainers.length)),
    };
  });

  const failureKinds = new Map<string, { gate: number; count: number; waivedBy: Set<string>; pages: Set<string> }>();
  for (const r of [...gating, ...tasks]) {
    for (const f of r.failures) {
      const kind = `gate${f.gate} ${f.detail.replace(/^overflow:.*/, 'overflow')}`;
      const e = failureKinds.get(kind) ?? { gate: f.gate, count: 0, waivedBy: new Set(), pages: new Set() };
      e.count++;
      if (f.waivedBy) e.waivedBy.add(f.waivedBy);
      e.pages.add(r.kind === 'page' ? r.path : r.scenario);
      failureKinds.set(kind, e);
    }
  }

  const round = {
    id,
    commit,
    date,
    adapter: adapter.name,
    cards: { anchors, rotating: rotatingCard, all_tasks_run: taskCards.map((s) => s.name) },
    gates: { ...gates, gate3: 'n/a: a docs site has no completion control', gate6: 'n/a: no guidance cue, no L0' },
    totals: { ...Object.fromEntries(pageKeys.map((k) => [k, totals(k)])), 'layout_shift.after_ready_max': maxReady, 'layout_shift.load_max': maxLoad },
    measured,
    measured_only: measuredOnly,
    scroll_containers_gating: sum(gating.map((r) => r.overflow.scrollContainers.length)),
    scroll_container_pages: scrollContainerPages(gating, prev?.scroll_container_pages ?? null),
    failure_kinds: [...failureKinds.entries()].map(([k, v]) => ({ kind: k, count: v.count, waived_by: [...v.waivedBy].sort(), pages: v.pages.size })),
    worst: worstOut,
    screen_only: tasks.map((t) => ({ scenario: t.scenario, ...t.result })),
    regression_assets: {
      passed: assets.filter((a) => a.ok).length,
      failed: assets.filter((a) => !a.ok).length,
      failing: assets.filter((a) => !a.ok).map((a) => `${a.file}::${a.title}`),
      findings_tests: assets.filter((a) => a.file === 'l2-findings.spec.ts').map((a) => ({ title: a.title, ok: a.ok })),
    },
    findings: findings.map((f) => ({ id: f.header.id, severity: f.header.severity, status: f.header.status, title: f.header.title })),
    missing_inputs: [
      'L6 trial sessions and their rrweb recordings: none recorded in this round',
      'questionnaire scores: no sessions, none',
      'L4 keyed replay and L5 discoverability probe: operator-only, not run',
      'review-heuristics.md and review-report.md: the two review agents run after this command (§ 8)',
      tasks.some((t) => t.result.trace.some((s) => s.action.startsWith('click searchbox')))
        ? 'search: the screen-only user uses the visible search box when no strong link is in view'
        : 'search: no task used search (no visible search box, or a strong link was in view first)',
    ],
  };
  writeFileSync(join(dir, 'round.json'), JSON.stringify(round, null, 2) + '\n');

  const L: string[] = [];
  L.push(`# ${id}: ${adapter.name}`, '');
  L.push(`Commit \`${commit}\` (${date}). Generated by \`task ux-round\`; numbers in \`round.json\`.`, '');
  L.push(`Cards: anchors ${anchors.map((a) => `\`${a}\``).join(', ')}; rotating \`${rotatingCard}\`. The screen-only user ran all ${taskCards.length} task cards.`, '');
  L.push(`## Hard gates (gating viewports ${adapter.viewports.gating.map((v) => v.name).join(', ')} × ${adapter.themes.join(', ')})`, '');
  L.push('| gate | checked | pass | fail | waived (open finding) |', '|---|---|---|---|---|');
  const gname: Record<string, string> = { gate1: '1 no horizontal scroll / overflow', gate2: '2 always visible', gate4: '4 screen-only user', gate5: '5 accessibility (axe, keyboard)' };
  for (const g of ['gate1', 'gate2', 'gate4', 'gate5']) {
    const c = gates[g] ?? { checked: 0, pass: 0, fail: 0, waived: 0 };
    L.push(`| ${gname[g]} | ${c.checked} | ${c.pass} | ${c.fail} | ${c.waived} |`);
  }
  L.push('| 3 unmissable completion | n/a | | | |', '| 6 cue truth | n/a (no guidance cue) | | | |', '');
  L.push('## Failures by kind', '', '| kind | units | pages/tasks | waived by |', '|---|---|---|---|');
  for (const f of round.failure_kinds) L.push(`| ${f.kind} | ${f.count} | ${f.pages} | ${f.waived_by.join(', ') || '**none**'} |`);
  L.push('');
  L.push('## Metrics against the bar, the baseline and the last round', '');
  L.push('Page totals over all pages (sums over the gating viewports × themes). Bar: 0 for every gated count.', '');
  L.push('| key | bar | this round | baseline | last round |', '|---|---|---|---|---|');
  for (const k of pageKeys) {
    const bar = k === 'axe.moderate' || k === 'axe.minor' ? '–' : '0';
    L.push(`| ${k} | ${bar} | ${totals(k)} | ${baseTotal(k) ?? '–'} | ${prev?.totals?.[k] ?? '–'} |`);
  }
  L.push(`| layout_shift.after_ready (max) | 0 | ${maxReady} | ${baseline ? Math.max(0, ...Object.values(baseline.scenarios).map((v) => v['layout_shift.after_ready']?.value ?? 0)) : '–'} | ${prev?.totals?.['layout_shift.after_ready_max'] ?? '–'} |`);
  L.push(`| layout_shift.load (max, measured: varies with machine load) | 0.1 | ${maxLoad} | – | ${prev?.totals?.['layout_shift.load_max'] ?? '–'} |`);
  L.push(`| scroll containers (measured, not gated) | – | ${round.scroll_containers_gating} | – | ${prev?.scroll_containers_gating ?? '–'} |`, '');
  L.push(`Screen-only user (${adapter.screenOnly.viewport} ${adapter.screenOnly.theme}, from the home page):`, '');
  L.push('| task | card | reached | actions | looks | steps | baseline actions/looks/steps | path |', '|---|---|---|---|---|---|---|---|');
  for (const t of tasks) {
    const b = baseline?.scenarios[t.scenario];
    const card = anchors.includes(t.scenario) ? 'anchor' : t.scenario === rotatingCard ? 'rotating (this round)' : 'rotating';
    const path = t.result.trace.filter((s) => s.action.startsWith('click')).map((s) => s.action.replace(/^click \w+ /, '')).join(' → ');
    L.push(
      `| ${t.scenario} | ${card} | ${t.result.reached ? 'yes' : '**no**'} | ${t.result.actions} | ${t.result.looks} | ${t.result.steps} | ${b ? `${b['screen_only.actions']?.value}/${b['screen_only.looks']?.value}/${b['screen_only.steps']?.value}` : '–'} | ${path} |`,
    );
  }
  L.push('');
  L.push('## Scroll containers at the gating viewports (measured, not gated)', '');
  if (!prev?.scroll_container_pages) L.push(`The last round${prevId ? ` (${prevId})` : ''} kept no page list, so its column is "–".`, '');
  L.push(...scrollContainerLines(round.scroll_container_pages), '');
  L.push('## Measured-only viewports (reported, not gated)', '');
  L.push('| viewport | checked | page scroll | overflow | always-visible misses | axe serious+critical nodes | scroll containers |', '|---|---|---|---|---|---|---|');
  for (const m of measuredOnly) L.push(`| ${m.viewport} | ${m.checked} | ${m.page_scroll} | ${m.overflow} | ${m.always_visible_misses} | ${m.axe_serious_critical} | ${m.scroll_containers} |`);
  L.push('');
  L.push('## Worst moments', '');
  for (const w of worstOut) {
    L.push(`- \`${w.path}\` ${w.viewport} ${w.theme}: ${w.failures.length} failures (${[...new Set(w.failures.map((k) => k.split('|')[4].split(':')[0]))].join(', ')})${w.shot ? `, [shot](${w.shot})` : ''}`);
  }
  L.push('');
  L.push('## Contact sheets', '');
  for (const s of scenarios.filter((x) => x.page)) {
    const f = `shots/sheet-${s.name}.jpg`;
    if (existsSync(join(dir, f))) L.push(`- [${s.page!.path}](${f})`);
  }
  L.push('');
  L.push('## Regression assets', '');
  L.push(`L1 and \`l2-findings.spec.ts\` in this run: ${round.regression_assets.passed} passed, ${round.regression_assets.failed} failed.`, '');
  L.push('| finding test | result |', '|---|---|');
  for (const a of round.regression_assets.findings_tests) L.push(`| ${a.title} | ${a.ok ? 'pass' : '**fail**'} |`);
  for (const a of round.regression_assets.failing.filter((x) => !x.startsWith('l2-findings'))) L.push(`| ${a} | **fail** |`);
  L.push('');
  L.push('## Findings', '', '| id | severity | status | title |', '|---|---|---|---|');
  for (const f of round.findings) L.push(`| ${f.id} | ${f.severity} | ${f.status} | ${f.title} |`);
  L.push('');
  L.push('## Questionnaire, L5 probe, missing inputs', '');
  for (const m of round.missing_inputs) L.push(`- ${m}`);
  L.push('');
  writeFileSync(join(dir, 'report.md'), L.join('\n'));

  const trend = join(roundsDir, 'trend.md');
  const head = [
    '# UX rounds: trend',
    '',
    'One row per round, written by `task ux-round`. Gate columns: failing units (unwaived + waived) at the gating viewports.',
    '',
    '| round | commit | date | gate1 | gate2 | gate4 | gate5 | axe serious+critical | overflow | screen-only actions | looks | unreached |',
    '|---|---|---|---|---|---|---|---|---|---|---|---|',
  ];
  const rows = existsSync(trend) ? readFileSync(trend, 'utf8').split('\n').filter((l) => /^\| R-\d+ /.test(l) && !l.startsWith(`| ${id} `)) : [];
  const gf = (g: string) => (gates[g] ? gates[g].fail + gates[g].waived : 0);
  const row = `| ${id} | ${commit} | ${date} | ${gf('gate1')} | ${gf('gate2')} | ${gf('gate4')} | ${gf('gate5')} | ${totals('axe.serious') + totals('axe.critical')} | ${totals('layout.overflow')} | ${sum(tasks.map((t) => t.result.actions))} | ${sum(tasks.map((t) => t.result.looks))} | ${tasks.filter((t) => !t.result.reached).length} |`;
  writeFileSync(trend, [...head, ...[...rows, row].sort(), ''].join('\n'));
}

