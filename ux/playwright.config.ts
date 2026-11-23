import { defineConfig } from '@playwright/test';
import { specPattern } from './core/specs.ts';

const round = process.env.UX_MODE === 'round';
const out = process.env.UX_OUT ?? '.out/check';

export default defineConfig({
  testDir: './tests',
  testMatch: specPattern(round ? 'round' : 'check'),
  globalSetup: './tests/global-setup.ts',
  fullyParallel: true,
  workers: process.env.UX_WORKERS ? Number(process.env.UX_WORKERS) : 2,
  retries: 0,
  timeout: 60_000,
  globalTimeout: round ? 1_740_000 : 120_000,
  reporter: [['list'], ['./tests/slowest-reporter.ts'], ['json', { outputFile: out + '/playwright.json' }]],
  outputDir: out + '/test-results',
  use: {
    browserName: 'chromium',
    headless: true,
    reducedMotion: 'reduce',
    serviceWorkers: 'block',
  },
});
