// Horizontal scroll containers, measured for the round report and never gated, and the ready time
// L2 waits for before its settle time and axe. The five page checks (page scroll, overflow, always
// visible, keyboard, layout shift) are ux-browser's: `ux/ux-browser.checks.yaml`, `core/page-checks.ts`.

import type { Page } from '@playwright/test';

export interface ScrollContainer {
  desc: string;
  scrollWidth: number;
  clientWidth: number;
  allowed: boolean;
}

/** Runs in the page. Plain function, no helpers from outside: Playwright serialises it. */
function scrollContainersInPage(allowed: string): ScrollContainer[] {
  const desc = (el: Element): string => {
    let s = el.tagName.toLowerCase();
    if (el.id) s += '#' + el.id;
    const cls = [...el.classList].filter((c) => !/_[A-Za-z0-9-]{4}$/.test(c)).slice(0, 2);
    if (cls.length) s += '.' + cls.join('.');
    const t = (el.textContent ?? '').replace(/\s+/g, ' ').trim().slice(0, 40);
    return t ? `${s} "${t}"` : s;
  };
  const out: ScrollContainer[] = [];
  for (const el of document.body.querySelectorAll('*')) {
    const cs = getComputedStyle(el);
    if ((cs.overflowX === 'auto' || cs.overflowX === 'scroll') && el.clientWidth > 0 && el.scrollWidth > el.clientWidth + 1 && cs.display !== 'none')
      out.push({ desc: desc(el), scrollWidth: el.scrollWidth, clientWidth: el.clientWidth, allowed: el.matches(allowed) });
  }
  return out;
}

export async function measureScrollContainers(page: Page, allowed: string): Promise<ScrollContainer[]> {
  return page.evaluate(scrollContainersInPage, allowed);
}

/** Load done, every font in and two frames drawn: what L2 waits for before its settle time and axe. */
export async function readyTime(page: Page): Promise<number> {
  return page.evaluate(async () => {
    await document.fonts.ready;
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    return performance.now();
  });
}
