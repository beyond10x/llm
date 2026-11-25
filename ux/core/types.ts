// The generic core's view of a project. The core never imports the adapter; callers pass it in.

export interface Viewport {
  name: string;
  width: number;
  height: number;
}

export interface Adapter {
  name: string;
  site: {
    buildDir: string;
    basePath: string;
    sitemap: string;
    home: string;
  };
  viewports: { gating: Viewport[]; measured: Viewport[] };
  themes: string[];
  themeAttribute: string;
  primaryRegion: string;
  scrollContainersAllowed: string;
  guidanceCue: null;
  protectedFocus: null;
  axeTags: string[];
  /** Excluded from axe on every page but `axeSample` (large syntax-highlighted blocks make axe run past the test budget). */
  axeExclude?: string;
  /** The page that axe checks whole, `axeExclude` included, in every theme. */
  axeSample?: string;
  /** The axe rules that depend on colour, run in every theme after the first. */
  axeThemeRules?: string[];
  /** Wait after load in the every-merge check; a round waits `settleMs`. */
  checkSettleMs?: number;
  hubWords: string[];
  screenOnly: { maxActions: number; maxScrolls: number; viewport: string; theme: string };
  settleMs: number;
  dirs: { scenarios: string; findings: string; rounds: string; baseline: string };
}

export interface Cue {
  word: string;
  weight: number;
}

export interface PageScenario {
  kind: 'page';
  path: string;
}

export interface TaskCard {
  kind: 'task';
  goal: string;
  persona: string;
  card: 'anchor' | 'rotating';
  start: string;
  cues: Cue[];
  search?: string;
}

export interface TaskTruth {
  path: string;
  hashes: string[];
}

export interface Expectations {
  reach: boolean;
}

export interface Scenario {
  name: string;
  page?: PageScenario;
  task?: { card: TaskCard; truth: TaskTruth; expectations: Expectations };
}
