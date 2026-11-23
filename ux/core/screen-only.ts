// The screen-only user (SKILL.md § 7): deterministic code, no model. It sees only controls fully in the
// viewport and not covered at their centre, picks them by accessible role and name, and counts what it costs.

import type { Page } from '@playwright/test';
import type { Cue, TaskCard, TaskTruth } from './types.ts';

export interface Candidate {
  role: 'link' | 'button' | 'searchbox' | 'option';
  name: string;
  region: 'primary' | 'other';
  current: boolean;
  external: boolean;
  box: { x: number; y: number; width: number; height: number };
}

export interface Step {
  url: string;
  action: string;
  glance: 1 | 2 | 3 | 0;
}

export interface ScreenOnlyResult {
  reached: boolean;
  actions: number;
  looks: number;
  steps: number;
  trace: Step[];
  reason?: string;
}

/** Runs in the page: every interactive control fully inside the viewport and on top at its centre. */
function visibleControls(primary: string): Candidate[] {
  const vw = document.documentElement.clientWidth;
  const vh = window.innerHeight;
  const out: Candidate[] = [];
  const nameOf = (e: Element): string => {
    const label = e.getAttribute('aria-label');
    if (label) return label.trim();
    const by = e.getAttribute('aria-labelledby');
    if (by) return by.split(/\s+/).map((id) => document.getElementById(id)?.textContent ?? '').join(' ').replace(/\s+/g, ' ').trim();
    // Text hidden from assistive technology (`aria-hidden`, an icon's svg) is not part of the
    // accessible name the bot later looks the control up by.
    const parts: string[] = [];
    const w = document.createTreeWalker(e, NodeFilter.SHOW_TEXT);
    for (let n = w.nextNode(); n; n = w.nextNode()) if (!n.parentElement?.closest('svg, [aria-hidden="true"]')) parts.push(n.textContent ?? '');
    const t = parts.join('').replace(/\s+/g, ' ').trim();
    if (t) return t;
    return (e.getAttribute('title') ?? e.querySelector('img[alt]')?.getAttribute('alt') ?? '').trim();
  };
  for (const e of document.querySelectorAll(
    'a[href], button, [role="link"], [role="button"], input[type="search"], [role="searchbox"], input[aria-label*="earch" i], [role="option"]',
  )) {
    const r = e.getBoundingClientRect();
    if (r.width < 1 || r.height < 1) continue;
    if (r.left < 0 || r.top < 0 || r.right > vw || r.bottom > vh) continue;
    const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
    if (!hit || !(hit === e || e.contains(hit))) continue;
    const tag = e.tagName.toLowerCase();
    const role: Candidate['role'] =
      e.getAttribute('role') === 'option'
        ? 'option'
        : tag === 'input' || e.getAttribute('role') === 'searchbox'
          ? 'searchbox'
          : tag === 'a' || e.getAttribute('role') === 'link'
            ? 'link'
            : 'button';
    const href = (e as HTMLAnchorElement).href ?? '';
    // A result option is read as its spans: title, then path, with a space between them.
    const optionName = () => {
      const parts: string[] = [];
      const w = document.createTreeWalker(e, NodeFilter.SHOW_TEXT);
      for (let n = w.nextNode(); n; n = w.nextNode()) if (!n.parentElement?.closest('svg, [aria-hidden="true"]')) parts.push(n.textContent ?? '');
      return parts.join(' ').replace(/\s+/g, ' ').trim();
    };
    out.push({
      role,
      name: role === 'option' ? optionName() : nameOf(e),
      region: e.closest(primary) ? 'primary' : 'other',
      current: e.getAttribute('aria-current') === 'page',
      external: role === 'link' && (e.getAttribute('target') === '_blank' || (href !== '' && new URL(href).origin !== location.origin)),
      box: { x: r.x, y: r.y, width: r.width, height: r.height },
    });
  }
  return out;
}

export function tokens(s: string): string[] {
  return s.toLowerCase().split(/[^a-z0-9]+/).filter(Boolean);
}

/** A score at or above this is a strong match: worth scrolling for before falling back to a hub link. */
export const STRONG = 2;

const SUFFIXES = ['', 's', 'es', 'd', 'ed', 'ing', 'er', 'ers', 'ion', 'ions', 'ation', 'ations'];

/** A name word matches a cue word when it is the cue plus an inflection: "syscalls" matches "syscall", "clipboard" not "cli". */
export function wordMatches(word: string, cue: string): boolean {
  return word.startsWith(cue) && SUFFIXES.includes(word.slice(cue.length));
}

export function score(name: string, cues: Cue[]): number {
  const ts = tokens(name);
  let s = 0;
  for (const c of cues) {
    const cw = tokens(c.word);
    const hit = ts.some((_, i) => cw.every((w, j) => i + j < ts.length && wordMatches(ts[i + j], w)));
    if (hit) s += c.weight;
  }
  return s;
}

/** Highest score first; ties by glance (primary region first), then by position top-left. */
export function pick(cands: Candidate[], cues: Cue[], min: number): { cand: Candidate; score: number } | null {
  let best: { cand: Candidate; score: number } | null = null;
  for (const c of cands) {
    const s = score(c.name, cues);
    if (s < min) continue;
    if (
      !best ||
      s > best.score ||
      (s === best.score && c.region === 'primary' && best.cand.region !== 'primary') ||
      (s === best.score && c.region === best.cand.region && (c.box.y < best.cand.box.y || (c.box.y === best.cand.box.y && c.box.x < best.cand.box.x)))
    )
      best = { cand: c, score: s };
  }
  return best;
}

export async function atTruth(page: Page, base: string, truth: TaskTruth): Promise<boolean> {
  const u = new URL(page.url());
  if (u.pathname.replace(/\/$/, '') !== truth.path.replace(/\/$/, '')) return false;
  if (truth.hashes.length === 0) return true;
  if (truth.hashes.includes(u.hash)) return true;
  return page.evaluate((ids) => {
    for (const h of ids) {
      const el = document.getElementById(h.slice(1));
      if (!el) continue;
      const r = el.getBoundingClientRect();
      if (r.top >= 0 && r.bottom <= window.innerHeight) return true;
    }
    return false;
  }, truth.hashes);
}

export interface BotOptions {
  base: string;
  primary: string;
  hubWords: string[];
  maxActions: number;
  maxScrolls: number;
  settleMs: number;
}

export async function runScreenOnly(page: Page, card: TaskCard, truth: TaskTruth, o: BotOptions): Promise<ScreenOnlyResult> {
  const trace: Step[] = [];
  let actions = 0;
  let looks = 0;
  let steps = 1;
  const visited = new Set<string>();
  const tried = new Set<string>();
  const searched = new Set<string>();
  const hub: Cue[] = o.hubWords.map((w) => ({ word: w, weight: 1 }));
  await page.goto(o.base + card.start);
  await page.waitForTimeout(o.settleMs);
  const here = () => {
    const u = new URL(page.url());
    return u.pathname + u.hash;
  };
  const done = (reached: boolean, reason?: string): ScreenOnlyResult => ({ reached, actions, looks, steps, trace, reason });
  while (actions < o.maxActions) {
    if (await atTruth(page, o.base, truth)) return done(true);
    visited.add(new URL(page.url()).pathname);
    const pageKey = new URL(page.url()).pathname;
    let scrolled = 0;
    let choice: { cand: Candidate; score: number } | null = null;
    let via = '';
    const all = async () => page.evaluate(visibleControls, o.primary);
    const seen = async () =>
      (await all()).filter((c) => !c.current && !c.external && c.role !== 'searchbox' && c.role !== 'option' && !tried.has(pageKey + '|' + c.role + '|' + c.name));
    // In order: a strong match in view, a visible search box, scrolling for one, a hub link in view, a weak match in view.
    for (;;) {
      const cands = await seen();
      choice = pick(cands, card.cues, STRONG);
      if (choice) break;
      if (scrolled === 0 && searched.size === 0) {
        const box = (await all()).find((c) => c.role === 'searchbox');
        if (box) {
          searched.add(pageKey);
          const used = await useSearch(page, box, card, o, (s) => {
            trace.push({ url: here(), ...s });
            actions++;
            steps++;
          });
          looks += used.looks;
          if (used.option) {
            choice = used.option;
            via = ' (search)';
            break;
          }
          continue;
        }
      }
      const atEnd = await page.evaluate(() => window.scrollY + window.innerHeight >= document.documentElement.scrollHeight - 2);
      if (atEnd || scrolled >= o.maxScrolls) {
        choice = pick(cands, hub, 1);
        via = ' (hub)';
        if (!choice) {
          choice = pick(cands, card.cues, 1);
          via = ' (weak)';
        }
        break;
      }
      await page.keyboard.press('PageDown');
      await scrollSettled(page);
      actions++;
      scrolled++;
      steps++;
      trace.push({ url: here(), action: 'PageDown', glance: 0 });
    }
    if (!choice) return done(false, `no visible control matches the task or a hub word on ${pageKey}`);
    const glance: 1 | 2 | 3 = scrolled > 0 ? 3 : choice.cand.region === 'primary' ? 1 : 2;
    looks += glance;
    tried.add(pageKey + '|' + choice.cand.role + '|' + choice.cand.name);
    // A result option's name joins its title and path spans, which the browser and the bot space
    // differently; it is found by its role and the box the bot saw.
    const loc =
      choice.cand.role === 'option' ? page.getByRole('option') : page.getByRole(choice.cand.role, { name: choice.cand.name, exact: true });
    const n = await loc.count();
    let target = null;
    for (let i = 0; i < n; i++) {
      const b = await loc.nth(i).boundingBox();
      if (b && Math.abs(b.x - choice.cand.box.x) < 1 && Math.abs(b.y - choice.cand.box.y) < 1) {
        target = loc.nth(i);
        break;
      }
    }
    if (!target) {
      trace.push({ url: here(), action: `skip ${choice.cand.role} "${choice.cand.name}": not found by role and name`, glance: 0 });
      continue;
    }
    await target.click();
    await page.waitForLoadState('load');
    await page.waitForTimeout(o.settleMs);
    actions++;
    steps++;
    trace.push({ url: here(), action: `click ${choice.cand.role} "${choice.cand.name}"${via}`, glance });
  }
  if (await atTruth(page, o.base, truth)) return done(true);
  return done(false, `not at ${truth.path} after ${actions} actions`);
}

/** A result option: highest score first; on a tie the shortest name (a title or heading hit over a text excerpt), then the higher one. */
export function pickOption(opts: Candidate[], cues: Cue[], min: number): { cand: Candidate; score: number } | null {
  let best: { cand: Candidate; score: number; len: number } | null = null;
  for (const c of opts) {
    const s = score(c.name, cues);
    if (s < min) continue;
    const len = tokens(c.name).length;
    if (!best || s > best.score || (s === best.score && (len < best.len || (len === best.len && c.box.y < best.cand.box.y)))) best = { cand: c, score: s, len };
  }
  return best && { cand: best.cand, score: best.score };
}

/** The query the user types: the card's own, else its heaviest cue word. */
export function searchQuery(card: TaskCard): string {
  if (card.search) return card.search;
  return [...card.cues].sort((a, b) => b.weight - a.weight)[0]?.word ?? '';
}

/**
 * Use a visible search box: click it (one action), type the query (one action: one input, however many
 * characters), and return the best visible result option for the cues, if any; the caller clicks it.
 * Without a matching option it presses Escape (one action) and the user goes on without search.
 */
async function useSearch(
  page: Page,
  box: Candidate,
  card: TaskCard,
  o: BotOptions,
  act: (s: Omit<Step, 'url'>) => void,
): Promise<{ option: { cand: Candidate; score: number } | null; looks: number }> {
  const query = searchQuery(card);
  if (!query) return { option: null, looks: 0 };
  const byRole = page
    .getByRole('searchbox', { name: box.name, exact: true })
    .or(page.getByRole('combobox', { name: box.name, exact: true }))
    .or(page.getByRole('textbox', { name: box.name, exact: true }));
  const n = await byRole.count();
  let input = null;
  for (let i = 0; i < n; i++) {
    const b = await byRole.nth(i).boundingBox();
    if (b && Math.abs(b.x - box.box.x) < 1 && Math.abs(b.y - box.box.y) < 1) {
      input = byRole.nth(i);
      break;
    }
  }
  if (!input) return { option: null, looks: 0 };
  const looks = box.region === 'primary' ? 1 : 2;
  await input.click();
  act({ action: `click searchbox "${box.name}"`, glance: looks as 1 | 2 });
  await input.fill(query);
  act({ action: `type "${query}"`, glance: 0 });
  let option: { cand: Candidate; score: number } | null = null;
  for (let i = 0; i < 40 && !option; i++) {
    await page.waitForTimeout(100);
    const opts = (await page.evaluate(visibleControls, o.primary)).filter((c) => c.role === 'option');
    option = pickOption(opts, card.cues, STRONG) ?? (i >= 20 ? pickOption(opts, card.cues, 1) : null);
  }
  if (!option) {
    await page.keyboard.press('Escape');
    act({ action: 'Escape (no matching result)', glance: 0 });
  }
  return { option, looks };
}

/** Wait until a (possibly smooth) scroll has stopped: the same scroll position on three reads 50 ms apart. */
export async function scrollSettled(page: Page): Promise<void> {
  let last = -1;
  let same = 0;
  for (let i = 0; i < 60 && same < 2; i++) {
    await page.waitForTimeout(50);
    const y = await page.evaluate(() => window.scrollY);
    same = y === last ? same + 1 : 0;
    last = y;
  }
}
