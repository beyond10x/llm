// The layout contract's measurements: horizontal overflow, always-visible elements, keyboard reach, layout shift.

import type { Locator, Page } from '@playwright/test';
import type { AlwaysVisible, Locate } from './types.ts';

export interface OverflowItem {
  desc: string;
  right: number;
  limit: number;
  against: 'parent' | 'viewport';
}

export interface ScrollContainer {
  desc: string;
  scrollWidth: number;
  clientWidth: number;
  allowed: boolean;
}

export interface OverflowResult {
  docScroll: number;
  overflow: OverflowItem[];
  scrollContainers: ScrollContainer[];
  clipped: string[];
}

/**
 * Runs in the page. Plain function, no helpers from outside: Playwright serialises it.
 *
 * Checks what is drawn, not layout boxes: every text run and every replaced or wide element (img, svg,
 * video, canvas, iframe, form controls, table, pre) must end inside its block container and inside the
 * viewport. Content inside a horizontal scroll container is measured as a scroll container, content
 * inside an overflow-hidden box as clipped, content in a fixed layer (skip links, off-canvas menus) is
 * left to the always-visible check. One failure per block container.
 */
function overflowInPage(allowed: string): OverflowResult {
  const vw = document.documentElement.clientWidth;
  const se = document.scrollingElement ?? document.documentElement;
  const docScroll = Math.max(0, se.scrollWidth - vw);
  const cache = new Map<Element, CSSStyleDeclaration>();
  const st = (e: Element): CSSStyleDeclaration => {
    let s = cache.get(e);
    if (!s) {
      s = getComputedStyle(e);
      cache.set(e, s);
    }
    return s;
  };
  const desc = (el: Element): string => {
    let s = el.tagName.toLowerCase();
    if (el.id) s += '#' + el.id;
    const cls = [...el.classList].filter((c) => !/_[A-Za-z0-9-]{4}$/.test(c)).slice(0, 2);
    if (cls.length) s += '.' + cls.join('.');
    const t = (el.textContent ?? '').replace(/\s+/g, ' ').trim().slice(0, 40);
    return t ? `${s} "${t}"` : s;
  };
  const overflow: OverflowItem[] = [];
  const scrollContainers: ScrollContainer[] = [];
  const clipped: string[] = [];
  for (const el of document.body.querySelectorAll('*')) {
    const cs = st(el);
    if ((cs.overflowX === 'auto' || cs.overflowX === 'scroll') && el.clientWidth > 0 && el.scrollWidth > el.clientWidth + 1 && cs.display !== 'none')
      scrollContainers.push({ desc: desc(el), scrollWidth: el.scrollWidth, clientWidth: el.clientWidth, allowed: el.matches(allowed) });
  }
  const context = (e: Element): { kind: 'fixed' | 'scroll' | 'clip' | null; at?: Element } => {
    for (let a: Element | null = e; a && a !== document.body && a !== document.documentElement; a = a.parentElement) {
      const s = st(a);
      if (s.position === 'fixed') return { kind: 'fixed' };
      if (s.overflowX === 'auto' || s.overflowX === 'scroll') return { kind: 'scroll' };
      if (s.overflowX === 'hidden' || s.overflowX === 'clip') return { kind: 'clip', at: a };
    }
    return { kind: null };
  };
  const blockOf = (e: Element): Element => {
    for (let a: Element | null = e; a && a !== document.body; a = a.parentElement) {
      const d = st(a).display;
      if (!d.startsWith('inline') && d !== 'contents') return a;
    }
    return document.body;
  };
  const failed = new Set<Element>();
  const check = (owner: Element, rect: DOMRect, from: Element) => {
    if (rect.width < 1 || rect.height < 1) return;
    const c = context(from);
    if (c.kind === 'fixed' || c.kind === 'scroll') return;
    if (c.kind === 'clip') {
      const cr = c.at!.getBoundingClientRect();
      if (cr.width > 1 && rect.right > cr.right + 1) clipped.push(desc(owner));
      return;
    }
    const block = blockOf(from);
    const br = block.getBoundingClientRect();
    let against: OverflowItem['against'];
    let limit: number;
    if (rect.right > vw + 1) {
      against = 'viewport';
      limit = vw;
    } else if (rect.right > br.right + 1) {
      against = 'parent';
      limit = br.right;
    } else return;
    if (failed.has(block)) return;
    failed.add(block);
    overflow.push({ desc: desc(block), right: Math.round(rect.right), limit: Math.round(limit), against });
  };
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  const range = document.createRange();
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    const p = n.parentElement;
    if (!p || !(n.textContent ?? '').trim() || st(p).visibility === 'hidden') continue;
    if (p.closest('svg, script, style, noscript, template')) continue;
    range.selectNodeContents(n);
    check(p, range.getBoundingClientRect(), p);
  }
  for (const el of document.body.querySelectorAll('img, svg, video, canvas, iframe, input, select, textarea, table, pre')) {
    if (el.parentElement?.closest('svg')) continue;
    if (st(el).visibility === 'hidden') continue;
    check(el, el.getBoundingClientRect(), el.parentElement ?? el);
  }
  return { docScroll, overflow, scrollContainers, clipped: [...new Set(clipped)] };
}


export async function measureOverflow(page: Page, allowed: string): Promise<OverflowResult> {
  return page.evaluate(overflowInPage, allowed);
}

export async function locate(page: Page, l: Locate): Promise<Locator | null> {
  const opts = l.name === undefined ? {} : { name: l.name, exact: typeof l.name === 'string' };
  const root = l.scope ? page.locator(l.scope).first() : page;
  const candidates = [root.getByRole(l.role, opts)];
  if (l.css) candidates.push(root.locator(l.css));
  for (const loc of candidates) {
    const n = await loc.count();
    for (let i = 0; i < n; i++) if (await loc.nth(i).isVisible()) return loc.nth(i);
  }
  return null;
}

export type VisibleStatus = 'ok' | 'missing' | 'clipped' | 'covered' | 'n/a';

export interface VisibleResult {
  name: string;
  status: VisibleStatus;
  box?: { x: number; y: number; width: number; height: number };
}

export function appliesTo(a: AlwaysVisible, path: string, width: number): boolean {
  if (a.minWidth !== undefined && width < a.minWidth) return false;
  if (a.pathPrefix !== undefined && !path.startsWith(a.pathPrefix)) return false;
  return true;
}

export async function checkAlwaysVisible(page: Page, items: AlwaysVisible[], path: string): Promise<VisibleResult[]> {
  const vp = page.viewportSize() ?? { width: 0, height: 0 };
  const out: VisibleResult[] = [];
  for (const a of items) {
    if (!appliesTo(a, path, vp.width)) {
      out.push({ name: a.name, status: 'n/a' });
      continue;
    }
    const loc = await locate(page, a.locate);
    if (!loc) {
      out.push({ name: a.name, status: 'missing' });
      continue;
    }
    const box = (await loc.boundingBox()) ?? undefined;
    if (!box || box.x < -0.5 || box.y < -0.5 || box.x + box.width > vp.width + 0.5 || box.y + box.height > vp.height + 0.5) {
      out.push({ name: a.name, status: 'clipped', box });
      continue;
    }
    const top = await loc.evaluate((e) => {
      const r = e.getBoundingClientRect();
      const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
      return !!hit && (hit === e || e.contains(hit));
    });
    out.push({ name: a.name, status: top ? 'ok' : 'covered', box });
  }
  return out;
}

export interface KeyboardResult {
  skipLinkFirst: boolean;
  skipMovesFocus: boolean;
  reached: Record<string, boolean>;
}


export async function keyboardWalk(
  page: Page,
  url: string,
  skipLink: RegExp,
  targets: { name: string; locate: Locate }[],
  maxTabs: number,
): Promise<KeyboardResult> {
  await page.goto(url);
  await page.keyboard.press('Tab');
  const first = await page.evaluate(() => {
    const a = document.activeElement as HTMLElement | null;
    return a ? (a.getAttribute('aria-label') ?? a.innerText ?? '').trim() : '';
  });
  const skipLinkFirst = skipLink.test(first);
  let skipMovesFocus = false;
  if (skipLinkFirst) {
    const target = await page.evaluate(() => (document.activeElement as HTMLAnchorElement | null)?.getAttribute('href') ?? '');
    await page.keyboard.press('Enter');
    await page.waitForTimeout(100);
    // Focus is on the skip link's target or inside it.
    const past = () =>
      page.evaluate((href) => {
        const t = href.startsWith('#') ? document.getElementById(href.slice(1)) : null;
        const a = document.activeElement;
        return !!t && !!a && a !== document.body && (t === a || t.contains(a));
      }, target);
    skipMovesFocus = await past();
    if (!skipMovesFocus) {
      await page.keyboard.press('Tab');
      skipMovesFocus = await past();
    }
  }
  await page.goto(url);
  const locs = new Map<string, Locator | null>();
  for (const t of targets) locs.set(t.name, await locate(page, t.locate));
  const reached: Record<string, boolean> = Object.fromEntries(targets.map((t) => [t.name, false]));
  for (let i = 0; i < maxTabs && Object.values(reached).some((v) => !v); i++) {
    await page.keyboard.press('Tab');
    for (const t of targets) {
      const loc = locs.get(t.name);
      if (reached[t.name] || !loc) continue;
      reached[t.name] = await loc.evaluate((e) => !!document.activeElement && (e === document.activeElement || e.contains(document.activeElement)));
    }
  }
  return { skipLinkFirst, skipMovesFocus, reached };
}

export const CLS_INIT = `
window.__uxShifts = [];
try {
  new PerformanceObserver((l) => { for (const e of l.getEntries()) if (!e.hadRecentInput) window.__uxShifts.push([e.startTime, e.value]); })
    .observe({ type: 'layout-shift', buffered: true });
} catch (e) { window.__uxShifts = null; }
`;

/** The time after which a shift counts as one the reader sees while reading: load done and every font in. */
export async function readyTime(page: Page): Promise<number> {
  return page.evaluate(async () => {
    await document.fonts.ready;
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    return performance.now();
  });
}

/**
 * Layout shift: `load` sums every shift since navigation (it depends on how fast fonts arrive, so it
 * varies with machine load and is reported, not ratcheted); `afterReady` sums the shifts after
 * `readyTime`, which a static page keeps at 0 on any machine.
 */
export async function readShift(page: Page, since: number): Promise<{ load: number; afterReady: number }> {
  return page.evaluate((t) => {
    const s = (window as unknown as { __uxShifts: [number, number][] | null }).__uxShifts;
    if (!s) return { load: -1, afterReady: -1 };
    const r = (x: number) => Math.round(x * 10000) / 10000;
    return { load: r(s.reduce((a, [, v]) => a + v, 0)), afterReady: r(s.filter(([at]) => at > t).reduce((a, [, v]) => a + v, 0)) };
  }, since);
}
