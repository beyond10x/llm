// The `ratchet-evidence/1` document `ux-browser checks` writes, read as the ratchet's five numbers per
// page scenario. A document of another format, another assessor or another ux-browser than the pinned
// one is refused with what was found and what is needed, before any value is taken from it.

import { existsSync, readFileSync } from 'node:fs';

export const EVIDENCE_FORMAT = 'ratchet-evidence/1';
export const EVIDENCE_ASSESSOR = 'ux-browser@1';

/** The five outputs the ratchet reads, the ratchet key of each, and how a page's per-viewport values become one. */
export const PAGE_OUTPUTS = [
  { output: 'layout.page_scroll@1', key: 'layout.page_scroll', over: 'sum' },
  { output: 'layout.overflow@1', key: 'layout.overflow', over: 'sum' },
  { output: 'always_visible.misses@1', key: 'always_visible.misses', over: 'sum' },
  { output: 'keyboard.misses@1', key: 'keyboard.misses', over: 'sum' },
  { output: 'layout_shift.after_ready@1', key: 'layout_shift.after_ready', over: 'max' },
] as const;

export interface Evidence {
  /** `evidence.conditions.tools`: the ux-browser and the Chrome that measured. */
  tools: { uxBrowser: string; chromium: string };
  /** Page path, then ratchet key, then viewport name: the value there. */
  pages: Record<string, Record<string, Record<string, number>>>;
}

/** The five numbers of each page scenario. */
export type PageValues = Record<string, Record<string, number>>;

const shown = (v: unknown) => (v === undefined ? 'absent' : JSON.stringify(v));

/** Reads the text of an evidence document. `where` names it in a refusal; `version` is the pinned ux-browser. */
export function parseEvidence(text: string, where: string, version: string): Evidence {
  let doc: any;
  try {
    doc = JSON.parse(text);
  } catch (e) {
    throw new Error(`${where}: not JSON (${(e as Error).message}); the ratchet reads the ${EVIDENCE_FORMAT} document ux-browser ${version} writes`);
  }
  if (doc?.format !== EVIDENCE_FORMAT) throw new Error(`${where}: format is ${shown(doc?.format)}, the ratchet reads "${EVIDENCE_FORMAT}"`);
  const ev = doc.evidence;
  if (ev?.assessor !== EVIDENCE_ASSESSOR) throw new Error(`${where}: evidence.assessor is ${shown(ev?.assessor)}, needed "${EVIDENCE_ASSESSOR}"`);
  const tools = ev.conditions?.tools ?? {};
  if (tools['ux-browser'] !== version)
    throw new Error(`${where}: written by ux-browser ${shown(tools['ux-browser'])}, the pinned release is "${version}"; run the gate again with the pinned one`);
  if (!Array.isArray(ev.results)) throw new Error(`${where}: evidence.results is ${shown(ev.results)}, needed a list`);

  const keyOf: Record<string, string> = Object.fromEntries(PAGE_OUTPUTS.map((o) => [o.output, o.key]));
  const pages: Evidence['pages'] = {};
  for (const r of ev.results) {
    const key = keyOf[r?.output];
    if (!key) continue;
    const at = `${where}: ${r.output} of ${shown(r.item)}`;
    if (typeof r.item !== 'string') throw new Error(`${at}: the item is no page path`);
    if (r.incomplete) throw new Error(`${at} is incomplete (${r.incomplete.cause}: ${r.incomplete.detail}); an incomplete result is no value`);
    const viewports = r.details?.viewports;
    if (!viewports || typeof viewports !== 'object' || Object.keys(viewports).length === 0) throw new Error(`${at}: details.viewports is ${shown(viewports)}, needed a value per viewport`);
    const page = (pages[r.item] ??= {});
    if (page[key]) throw new Error(`${at}: a second result of that output for the page`);
    page[key] = {};
    for (const [vp, v] of Object.entries(viewports)) {
      if (typeof v !== 'string' || !/^\d+(\.\d+)?$/.test(v)) throw new Error(`${at}: the value at ${vp} is ${shown(v)}, needed a decimal number as text`);
      page[key][vp] = Number(v);
    }
  }
  if (Object.keys(pages).length === 0) throw new Error(`${where}: no result of ${PAGE_OUTPUTS.map((o) => o.output).join(', ')}`);
  for (const [path, page] of Object.entries(pages))
    for (const o of PAGE_OUTPUTS) if (!page[o.key]) throw new Error(`${where}: page ${path} has no result of ${o.output}`);
  return { tools: { uxBrowser: tools['ux-browser'], chromium: String(tools.chromium ?? '') }, pages };
}

/** Reads the evidence document at `file`; a missing file is refused like a document of another kind. */
export function readEvidence(file: string, version: string): Evidence {
  if (!existsSync(file)) throw new Error(`${file}: no such file; the ratchet needs the ${EVIDENCE_FORMAT} document ux-browser ${version} writes there, run \`task ux-check\` first`);
  return parseEvidence(readFileSync(file, 'utf8'), file, version);
}

/**
 * The five numbers of each page scenario over the gating viewports: a count is the sum of its
 * viewports' counts, the layout shift is the largest. Every page of the evidence needs a scenario,
 * every scenario a page, and every page a value at every gating viewport.
 */
export function pageValues(evidence: Evidence, scenarios: { name: string; path: string }[], gating: string[]): PageValues {
  const out: PageValues = {};
  const byPath = new Map(scenarios.map((s) => [s.path, s.name]));
  for (const path of Object.keys(evidence.pages)) if (!byPath.has(path)) throw new Error(`page ${path} of the evidence has no page scenario; run \`task ux-scenarios\``);
  for (const s of scenarios) {
    const page = evidence.pages[s.path];
    if (!page) throw new Error(`page scenario ${s.name} (${s.path}) has no result in the evidence`);
    out[s.name] = {};
    for (const o of PAGE_OUTPUTS) {
      const values = gating.map((vp) => {
        const v = page[o.key][vp];
        if (v === undefined) throw new Error(`page ${s.path} has no ${o.output} at the gating viewport ${vp}; the evidence has ${Object.keys(page[o.key]).join(', ')}`);
        return v;
      });
      out[s.name][o.key] = o.over === 'max' ? Math.max(0, ...values) : values.reduce((a, b) => a + b, 0);
    }
  }
  return out;
}
