// Prints the run's wall time and its ten slowest tests (the beyond10x test budget: 60 s a test,
// 120 s the step).

import type { FullResult, Reporter, TestCase, TestResult } from '@playwright/test/reporter';

export default class Slowest implements Reporter {
  private times: { title: string; ms: number }[] = [];

  onTestEnd(test: TestCase, result: TestResult): void {
    this.times.push({ title: test.titlePath().slice(1).join(' › '), ms: result.duration });
  }

  onEnd(result: FullResult): void {
    const slow = [...this.times].sort((a, b) => b.ms - a.ms).slice(0, 10);
    console.log(`\nwall time ${(result.duration / 1000).toFixed(1)} s, ${this.times.length} tests; ten slowest:`);
    for (const t of slow) console.log(`  ${(t.ms / 1000).toFixed(1).padStart(6)} s  ${t.title}`);
  }
}
