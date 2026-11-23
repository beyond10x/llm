// Round mode only: a contact sheet per page, every gating viewport in every theme, each frame taken
// settleMs (0.75 s) after load.

import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { test } from '@playwright/test';
import { adapter } from '../adapter.ts';
import { SCENARIOS, base, hermetic } from './env.ts';

const dir = process.env.UX_ROUND_DIR;

for (const sc of SCENARIOS.filter((s) => s.page)) {
  test(`sheet ${sc.name}`, async ({ browser }) => {
    test.skip(!dir, 'UX_ROUND_DIR unset');
    const frames: { label: string; data: string; w: number; h: number }[] = [];
    for (const vp of adapter.viewports.gating) {
      for (const theme of adapter.themes) {
        const context = await browser.newContext({ viewport: { width: vp.width, height: vp.height }, colorScheme: theme as 'light' | 'dark', reducedMotion: 'reduce' });
        await hermetic(context);
        const page = await context.newPage();
        await page.goto(base() + sc.page!.path, { waitUntil: 'load' });
        await page.waitForTimeout(adapter.settleMs);
        const buf = await page.screenshot({ type: 'jpeg', quality: 80 });
        frames.push({ label: `${vp.name} ${theme}`, data: buf.toString('base64'), w: vp.width, h: vp.height });
        await context.close();
      }
    }
    const context = await browser.newContext({ viewport: { width: 1400, height: 900 } });
    const page = await context.newPage();
    const cells = frames
      .map((f) => `<figure><img src="data:image/jpeg;base64,${f.data}" width="${f.w / 2}" height="${f.h / 2}"><figcaption>${f.label}</figcaption></figure>`)
      .join('');
    await page.setContent(
      `<html><body style="margin:0;padding:8px;background:#888;font:14px sans-serif">` +
        `<div style="color:#fff;margin:0 0 6px">${sc.page!.path}</div>` +
        `<div style="display:grid;grid-template-columns:repeat(2,max-content);gap:8px">${cells}</div>` +
        `<style>figure{margin:0;background:#fff}figcaption{padding:2px 4px}img{display:block}</style></body></html>`,
    );
    const shots = join(dir!, 'shots');
    mkdirSync(shots, { recursive: true });
    await page.screenshot({ path: join(shots, `sheet-${sc.name}.jpg`), type: 'jpeg', quality: 70, fullPage: true });
    await context.close();
  });
}
