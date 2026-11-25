// Before the first test: the built site is served, and in a check run the pinned ux-browser starts
// its five page checks over the same build beside the specs. It runs inside this Playwright run, so
// the run's `globalTimeout` is its bound too: `l2-layout.spec.ts` awaits its end and reads what it
// wrote, and when the run ends, in the teardown or without one (a run that timed out skips it), the
// ux-browser that has not ended is killed with its Chromium. A round runs ux-browser before the specs
// (`cli.ts round`).

import { rmSync } from 'node:fs';
import { join, resolve } from 'node:path';
import type { FullConfig } from '@playwright/test';
import { adapter } from '../adapter.ts';
import { pageCheckFiles, startPageChecks, stopPageChecks, uxBrowser } from '../core/page-checks.ts';
import { serve } from '../core/site.ts';

export default async function globalSetup(config: FullConfig): Promise<() => Promise<void>> {
  const ux = resolve(import.meta.dirname, '..');
  const out = resolve(ux, process.env.UX_OUT ?? '.out/check');
  const root = resolve(ux, adapter.site.buildDir);
  // Refused before anything is started or removed: a check run without the ux-browser it was handed.
  const binary = process.env.UX_MODE === 'round' ? null : uxBrowser(process.env);
  rmSync(resolve(out, 'results'), { recursive: true, force: true });
  const { url, server } = await serve(root, adapter.site.basePath);
  process.env.UX_BASE_URL = url;
  const run = { config: join(ux, 'ux-browser.checks.yaml'), build: root, root: resolve(ux, '..'), project: 'llm-docs', assessment: 'page-checks', jobs: config.workers };
  const checks = binary ? startPageChecks(binary, run, pageCheckFiles(out)) : null;
  const stop = () => stopPageChecks(checks);
  process.once('exit', stop);
  // SIGTERM and SIGHUP end this process without an `exit` event, and ux-browser leads a process group
  // of its own, which a signal to this one's group does not reach. Stopped here, then the signal
  // takes its course: Playwright's own handling if it has one, else the default.
  for (const signal of ['SIGTERM', 'SIGHUP'] as const)
    process.once(signal, () => {
      stop();
      if (process.listenerCount(signal) === 0) process.kill(process.pid, signal);
    });
  return async () => {
    stop();
    await new Promise<void>((ok) => server.close(() => ok()));
  };
}
