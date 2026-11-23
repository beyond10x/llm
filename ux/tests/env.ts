// Shared setup for the specs: paths, the served site, the hermetic browser context.

import { resolve } from 'node:path';
import type { BrowserContext } from '@playwright/test';
import { adapter } from '../adapter.ts';
import { loadFindings } from '../core/findings.ts';
import { loadScenarios } from '../core/scenarios.ts';

export const UX = resolve(import.meta.dirname, '..');
export const REPO = resolve(UX, '..');
export const OUT = resolve(UX, process.env.UX_OUT ?? '.out/check');
export const ROUND_MODE = process.env.UX_MODE === 'round';
export const SITE_ROOT = resolve(UX, adapter.site.buildDir);
export const SCENARIOS = loadScenarios(resolve(UX, adapter.dirs.scenarios));
export const FINDINGS = loadFindings(resolve(UX, adapter.dirs.findings));

export function base(): string {
  const b = process.env.UX_BASE_URL;
  if (!b) throw new Error('UX_BASE_URL unset: the global setup did not run');
  return b;
}

/** No request leaves the machine: anything but the local server is aborted. */
export async function hermetic(context: BrowserContext): Promise<void> {
  const origin = new URL(base()).origin;
  await context.route('**/*', (route) => {
    const u = route.request().url();
    if (u.startsWith(origin) || u.startsWith('data:') || u.startsWith('blob:')) return route.continue();
    return route.abort('blockedbyclient');
  });
}
