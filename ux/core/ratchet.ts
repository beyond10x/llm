// The ratchet of SKILL.md § 6. Every value is lower-is-better.

export type Tolerance = number | `${number}%`;

export interface Entry {
  value: number;
  tolerance: Tolerance;
  decision?: string;
}

export interface Baseline {
  about: string;
  scenarios: Record<string, Record<string, Entry>>;
  tolerances: Record<string, Tolerance>;
}

export type Measured = Record<string, Record<string, number>>;

/** Tolerance class of a metric key. */
export function toleranceFor(key: string, tolerances: Record<string, Tolerance>): Tolerance {
  if (key.startsWith('layout_shift')) return tolerances.layout_shift ?? 0.01;
  if (key.endsWith('steps')) return tolerances.steps ?? '5%';
  return tolerances.counts ?? 0;
}

export function allowance(base: number, t: Tolerance): number {
  return typeof t === 'number' ? t : (base * parseFloat(t)) / 100;
}

const fmt = (n: number) => String(Math.round(n * 10000) / 10000);

export interface RatchetReport {
  problems: string[];
  improved: string[];
  worse: string[];
}

export function compare(base: Baseline, measured: Measured, main: Baseline | null): RatchetReport {
  const problems: string[] = [];
  const improved: string[] = [];
  const worse: string[] = [];
  for (const [sc, keys] of Object.entries(base.scenarios)) {
    for (const [k, e] of Object.entries(keys)) {
      const id = `${sc} ${k}`;
      const m = measured[sc]?.[k];
      if (m === undefined || Number.isNaN(m)) {
        problems.push(`missing value: ${id} (baseline ${fmt(e.value)})`);
        continue;
      }
      const tol = allowance(e.value, e.tolerance);
      if (m > e.value + tol) {
        worse.push(id);
        problems.push(`worse: ${id} ${fmt(m)} > baseline ${fmt(e.value)} + ${fmt(tol)}`);
      } else if (m < e.value - (typeof e.tolerance === 'number' ? e.tolerance : 0) && m < e.value) {
        improved.push(id);
        problems.push(`improvement not taken: ${id} ${fmt(m)} < baseline ${fmt(e.value)}; run the ratchet with --update in this merge`);
      }
    }
  }
  for (const [sc, keys] of Object.entries(measured))
    for (const k of Object.keys(keys))
      if (base.scenarios[sc]?.[k] === undefined) problems.push(`missing value: ${sc} ${k} measured but not in the baseline; run --update`);
  if (main) {
    for (const [sc, keys] of Object.entries(base.scenarios)) {
      for (const [k, e] of Object.entries(keys)) {
        const was = main.scenarios[sc]?.[k];
        if (!was) continue;
        if (e.value > was.value + allowance(was.value, was.tolerance) && !e.decision)
          problems.push(`baseline made worse without a decision id: ${sc} ${k} ${fmt(was.value)} on main -> ${fmt(e.value)}`);
      }
    }
  }
  return { problems, improved, worse };
}

/** The measured values as the new baseline. A value worse than before needs a decision id. */
export function update(base: Baseline | null, measured: Measured, tolerances: Record<string, Tolerance>, about: string, decision?: string): { baseline: Baseline; refused: string[] } {
  const refused: string[] = [];
  const scenarios: Baseline['scenarios'] = {};
  for (const sc of Object.keys(measured).sort()) {
    scenarios[sc] = {};
    for (const k of Object.keys(measured[sc]).sort()) {
      const v = measured[sc][k];
      const tolerance = toleranceFor(k, tolerances);
      const old = base?.scenarios[sc]?.[k];
      const entry: Entry = { value: v, tolerance };
      if (old && v > old.value + allowance(old.value, old.tolerance)) {
        if (!decision) refused.push(`${sc} ${k}: ${fmt(old.value)} -> ${fmt(v)} is worse; pass --decision <id>`);
        else entry.decision = decision;
      } else if (old?.decision && v >= old.value) entry.decision = old.decision;
      scenarios[sc][k] = entry;
    }
  }
  return { baseline: { about, scenarios, tolerances }, refused };
}
