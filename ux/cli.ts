// node cli.ts <command>: scenarios | ratchet [--update] [--decision <id>] [--out <dir>] | round [--round R-nn] | findings

import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { adapter } from './adapter.ts';
import { findingProblems, loadFindings } from './core/findings.ts';
import { compare, update, type Baseline, type Tolerance } from './core/ratchet.ts';
import { readResults, toMeasured } from './core/results.ts';
import { pageDrift, syncPages } from './core/scenarios.ts';
import { sitemapPaths } from './core/site.ts';
import { nextRound, writeRound } from './core/round.ts';

const UX = import.meta.dirname;
const REPO = resolve(UX, '..');
const SITE = resolve(UX, adapter.site.buildDir);
const BASELINE = join(UX, adapter.dirs.baseline);
const TOLERANCES: Record<string, Tolerance> = { counts: 0, steps: '5%', layout_shift: 0.01 };
const ABOUT =
  'llm docs site (website/) ratchet, ui-iteration-loop § 6. Lower is better. Page scenarios: sums over the gating viewports 1440x900 and 390x844 × light and dark; layout_shift.after_ready is the max of the shifts after load and fonts. Task scenarios: the screen-only user at 1440x900 light. Update with `task ux-ratchet -- --update [--decision <id>]`.';

const args = process.argv.slice(2);
const cmd = args[0];
const flag = (n: string) => args.includes(n);
const opt = (n: string) => {
  const i = args.indexOf(n);
  return i >= 0 ? args[i + 1] : undefined;
};

/** The baseline on main: `origin/main` where it exists (CI fetches it), else the local `main`. */
function mainBaseline(): Baseline | null {
  const rel = join('ux', adapter.dirs.baseline);
  for (const ref of ['origin/main', 'main']) {
    try {
      return JSON.parse(execFileSync('git', ['show', `${ref}:${rel}`], { cwd: REPO, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }));
    } catch {
      continue;
    }
  }
  return null;
}

function ratchet(): number {
  const out = resolve(UX, opt('--out') ?? '.out/check');
  const results = readResults(out);
  if (results.length === 0) {
    console.error(`ratchet: no results in ${out}; run the L2 and L3 specs first`);
    return 1;
  }
  const measured = toMeasured(results);
  const current: Baseline | null = existsSync(BASELINE) ? JSON.parse(readFileSync(BASELINE, 'utf8')) : null;
  if (flag('--update')) {
    const { baseline, refused } = update(current, measured, TOLERANCES, ABOUT, opt('--decision'));
    if (refused.length) {
      for (const r of refused) console.error('ratchet: refused: ' + r);
      return 1;
    }
    writeFileSync(BASELINE, JSON.stringify(baseline, null, 2) + '\n');
    console.log(`ratchet: wrote ${BASELINE}`);
    return 0;
  }
  if (!current) {
    console.error('ratchet: no ux/baseline.json; run with --update to record the first baseline');
    return 1;
  }
  const main = mainBaseline();
  const r = compare(current, measured, main);
  console.log(`ratchet: ${Object.keys(current.scenarios).length} scenarios; main baseline ${main ? 'compared' : 'absent (first baseline)'}`);
  for (const p of r.problems) console.error('ratchet: ' + p);
  if (r.problems.length) return 1;
  console.log('ratchet: no value worse, no improvement left untaken');
  return 0;
}

function scenarios(): number {
  const paths = sitemapPaths(SITE, adapter.site.sitemap);
  const dir = join(UX, adapter.dirs.scenarios);
  for (const w of syncPages(dir, paths, adapter.site.basePath)) console.log('scenarios: wrote ' + w);
  const { stale } = pageDrift(dir, paths);
  for (const s of stale) console.error('scenarios: stale page scenario (not in the sitemap): ' + s);
  return stale.length ? 1 : 0;
}

function findings(): number {
  const p = findingProblems(loadFindings(join(UX, adapter.dirs.findings)), REPO);
  for (const x of p) console.error('findings: ' + x);
  return p.length ? 1 : 0;
}

function round(): number {
  const roundsDir = join(UX, adapter.dirs.rounds);
  const id = opt('--round') ?? nextRound(roundsDir);
  const dir = join(roundsDir, id);
  const out = join(UX, '.out/round');
  const env = { ...process.env, UX_MODE: 'round', UX_OUT: '.out/round', UX_ROUND_DIR: dir };
  const pw = spawnSync('npx', ['playwright', 'test'], { cwd: UX, env, stdio: 'inherit' });
  console.log(`round: playwright exit ${pw.status} (gate failures are reported, not fatal here)`);
  writeRound({ adapter, ux: UX, repo: REPO, id, dir, out, baselineFile: BASELINE });
  console.log(`round: wrote ${dir}/report.md, round.json, shots/ and ${roundsDir}/trend.md`);
  return 0;
}

const commands: Record<string, () => number> = { ratchet, scenarios, findings, round };
if (!cmd || !commands[cmd]) {
  console.error('usage: node cli.ts scenarios | ratchet [--update] [--decision <id>] [--out <dir>] | findings | round [--round R-nn]');
  process.exit(2);
}
process.exit(commands[cmd]());
