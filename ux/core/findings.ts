// Finding records ux/findings/UXF-nnn.md (SKILL.md § 5): a header between --- lines, `key: value` per line.

import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

export const HEADER_FIELDS = [
  'id',
  'title',
  'class',
  'severity',
  'metric',
  'status',
  'found_by',
  'evidence_recording',
  'evidence_screenshot',
  'evidence_lines',
  'regression_asset',
] as const;

export const CLASSES = ['layout', 'cue', 'affordance', 'copy', 'noise', 'engine', 'input'];
export const SEVERITIES = ['S1', 'S2', 'S3'];
export const STATUSES = ['open', 'decided', 'in-progress', 'closed'];

export interface Finding {
  file: string;
  header: Record<string, string>;
  waives: string[];
}

export function parseFinding(file: string, text: string): Finding {
  const m = /^---\n([\s\S]*?)\n---\n/.exec(text);
  if (!m) throw new Error(`${file}: no header between --- lines`);
  const header: Record<string, string> = {};
  for (const line of m[1].split('\n')) {
    const kv = /^([a-z_]+):\s?(.*)$/.exec(line);
    if (!kv) throw new Error(`${file}: header line is not key: value: ${line}`);
    header[kv[1]] = kv[2].trim();
  }
  const waives = (header.waives ?? '')
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean);
  return { file, header, waives };
}

export function loadFindings(dir: string): Finding[] {
  if (!existsSync(dir)) return [];
  return readdirSync(dir)
    .filter((f) => /^UXF-\d{3}\.md$/.test(f))
    .sort()
    .map((f) => parseFinding(f, readFileSync(join(dir, f), 'utf8')));
}

/** Problems with the records; the § 5 rule: no closed S1/S2 record without a regression asset that exists. */
export function findingProblems(findings: Finding[], repoRoot: string): string[] {
  const out: string[] = [];
  const ids = new Set<string>();
  for (const f of findings) {
    const h = f.header;
    for (const k of HEADER_FIELDS) if (!(k in h)) out.push(`${f.file}: header field ${k} missing`);
    if (h.id !== f.file.replace(/\.md$/, '')) out.push(`${f.file}: id ${h.id} does not match the file name`);
    if (ids.has(h.id)) out.push(`${f.file}: duplicate id`);
    ids.add(h.id);
    if (!CLASSES.includes(h.class)) out.push(`${f.file}: class ${h.class} not one of ${CLASSES.join('|')}`);
    if (!SEVERITIES.includes(h.severity)) out.push(`${f.file}: severity ${h.severity} not one of S1|S2|S3`);
    if (!STATUSES.includes(h.status)) out.push(`${f.file}: status ${h.status} not one of ${STATUSES.join('|')}`);
    const asset = h.regression_asset ?? '';
    if (h.status === 'closed' && (h.severity === 'S1' || h.severity === 'S2')) {
      if (asset === '' || asset === 'none') out.push(`${f.file}: closed ${h.severity} without a regression asset`);
      else {
        const path = asset.split('::')[0];
        if (!existsSync(join(repoRoot, path))) out.push(`${f.file}: regression asset ${path} does not exist`);
      }
    }
    if (h.status === 'closed' && f.waives.length) out.push(`${f.file}: a closed finding still waives ${f.waives.join(', ')}`);
  }
  return out;
}

/** `*` matches any run of characters; everything else is literal. */
export function glob(pattern: string, s: string): boolean {
  const re = new RegExp('^' + pattern.split('*').map((p) => p.replace(/[.+?^${}()|[\]\\]/g, '\\$&')).join('.*') + '$');
  return re.test(s);
}

/** The open finding that waives a gate failure key, if any. Keys: gate<n>|<path>|<viewport>|<theme>|<detail>. */
export function waiverFor(findings: Finding[], key: string): string | null {
  for (const f of findings) {
    if (f.header.status === 'closed') continue;
    if (f.waives.some((w) => glob(w, key))) return f.header.id;
  }
  return null;
}
