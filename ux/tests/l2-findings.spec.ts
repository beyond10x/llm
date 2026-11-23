// Regression assets of findings beyond what the generic L2 contract measures (ui-iteration-loop § 5).
// One test per finding; each failed on the R-01 build before its fix.

import { test, expect, type Browser, type Page } from '@playwright/test';
import { base, hermetic } from './env.ts';

async function open(browser: Browser, path: string, width: number, height: number, theme = 'light'): Promise<Page> {
  const context = await browser.newContext({ viewport: { width, height }, colorScheme: theme as 'light' | 'dark', reducedMotion: 'reduce' });
  await hermetic(context);
  const page = await context.newPage();
  const resp = await page.goto(base() + path, { waitUntil: 'load' });
  expect(resp?.status(), `${path} is served`).toBe(200);
  await page.waitForTimeout(300);
  return page;
}

// UXF-001: the navbar search finds the exit codes of the llm command and opens that page.
test('UXF-001 navbar search finds "exit codes" and opens the llm command page', async ({ browser }) => {
  const page = await open(browser, '/llm/docs/getting-started', 1440, 900);
  const box = page.locator('.navbar').getByRole('searchbox').or(page.locator('.navbar .navbar__search-input')).first();
  await expect(box, 'a search box in the navbar').toBeVisible({ timeout: 2_000 });
  await box.click();
  await box.fill('exit codes');
  const hit = page.getByRole('option').filter({ hasText: /exit codes/i }).first();
  await expect(hit).toBeVisible({ timeout: 10_000 });
  await hit.click();
  await expect(page).toHaveURL(/\/llm\/docs\/guides\/use-the-llm-command/);
  await page.context().close();
});

// UXF-003: every link in running text is underlined, in both themes.
for (const theme of ['light', 'dark']) {
  test(`UXF-003 links in running text are underlined (${theme})`, async ({ browser }) => {
    const page = await open(browser, '/llm/docs/getting-started', 1440, 900, theme);
    const bare = await page.evaluate(() =>
      [...document.querySelectorAll('.markdown p a, .markdown li a, .markdown td a')]
        .filter((a) => !getComputedStyle(a).textDecorationLine.includes('underline'))
        .map((a) => (a.textContent ?? '').trim()),
    );
    expect(bare, 'links without an underline').toEqual([]);
    await page.context().close();
  });
}

// UXF-004: on a phone the theme toggle is in the navbar and switches the theme within two taps. The
// toggle cycles system, light, dark, as on a desktop: from "system" in a light scheme the first tap
// selects light, the second dark.
test('UXF-004 the theme toggle is in the navbar at 390x844 and switches the theme within two taps', async ({ browser }) => {
  const page = await open(browser, '/llm/docs/getting-started', 390, 844);
  const toggle = page.locator('.navbar').getByRole('button', { name: /dark and light mode/i }).first();
  await expect(toggle, 'a theme toggle in the navbar').toBeVisible({ timeout: 2_000 });
  const before = await page.evaluate(() => document.documentElement.getAttribute('data-theme'));
  const theme = () => page.evaluate(() => document.documentElement.getAttribute('data-theme'));
  await toggle.click();
  if ((await theme()) === before) await toggle.click();
  await expect.poll(theme).not.toBe(before);
  await page.context().close();
});
