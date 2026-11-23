// Which specs a run executes, and the regression assets' results read back from Playwright's JSON report.

export type Mode = 'check' | 'round';

/**
 * The spec files of a `task ux-check` run and of a `task ux-round` run. A round runs the regression
 * assets too (L1 and `l2-findings`), so its report can say whether the last change set held (UXF-026).
 */
export function specPattern(mode: Mode): RegExp {
  return mode === 'round'
    ? /(l1-.*|l2-layout|l2-findings|l3-screen-only|round-shots)\.spec\.ts$/
    : /(l1-.*|l2-layout|l2-findings|l3-screen-only)\.spec\.ts$/;
}

export interface AssetResult {
  file: string;
  title: string;
  ok: boolean;
}

interface Suite {
  title?: string;
  file?: string;
  specs?: { title: string; tests?: { status?: string }[] }[];
  suites?: Suite[];
}

const ASSET_FILE = /^(l1-.*|l2-findings)\.spec\.ts$/;

/** Every test of a regression-asset spec (`l1-*`, `l2-findings`) in a Playwright JSON report, in report order. */
export function assetResults(report: unknown): AssetResult[] {
  const out: AssetResult[] = [];
  const walk = (s: Suite, file: string, path: string[]) => {
    for (const spec of s.specs ?? [])
      out.push({ file, title: [...path, spec.title].join(' › '), ok: (spec.tests ?? []).every((t) => t.status === 'expected') });
    for (const child of s.suites ?? []) walk(child, file, [...path, child.title ?? '']);
  };
  for (const top of (report as { suites?: Suite[] } | null)?.suites ?? []) {
    const file = (top.file ?? '').split('/').pop() ?? '';
    if (ASSET_FILE.test(file)) walk(top, file, []);
  }
  return out;
}
