// Scenario directories: page-<slug>/page.json for each sitemap page, task-<name>/{card,truth,ux-expectations}.json.

import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import type { Scenario } from './types.ts';
import { pageSlug } from './site.ts';

function readJson<T>(p: string): T {
  return JSON.parse(readFileSync(p, 'utf8')) as T;
}

export function loadScenarios(dir: string): Scenario[] {
  const out: Scenario[] = [];
  for (const name of readdirSync(dir).sort()) {
    const d = join(dir, name);
    if (existsSync(join(d, 'page.json'))) out.push({ name, page: readJson(join(d, 'page.json')) });
    else if (existsSync(join(d, 'card.json')))
      out.push({
        name,
        task: {
          card: readJson(join(d, 'card.json')),
          truth: readJson(join(d, 'truth.json')),
          expectations: readJson(join(d, 'ux-expectations.json')),
        },
      });
    else throw new Error(`scenario ${name} has neither page.json nor card.json`);
  }
  return out;
}

/** Page scenarios the sitemap has and the scenarios dir lacks, and the reverse. */
export function pageDrift(dir: string, paths: string[]): { missing: string[]; stale: string[] } {
  const have = new Set(loadScenarios(dir).flatMap((s) => (s.page ? [s.page.path] : [])));
  const want = new Set(paths);
  return {
    missing: paths.filter((p) => !have.has(p)),
    stale: [...have].filter((p) => !want.has(p)).sort(),
  };
}

/** Write a page scenario for every sitemap path that has none. Stale ones are reported, not deleted. */
export function syncPages(dir: string, paths: string[], basePath: string): string[] {
  const written: string[] = [];
  for (const p of paths) {
    const d = join(dir, pageSlug(p, basePath));
    if (existsSync(join(d, 'page.json'))) continue;
    mkdirSync(d, { recursive: true });
    writeFileSync(join(d, 'page.json'), JSON.stringify({ kind: 'page', path: p }, null, 2) + '\n');
    written.push(d);
  }
  return written;
}
