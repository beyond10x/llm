// The llm docs site (Docusaurus, website/) as the ui-iteration-loop sees it.

import type { Adapter } from './core/types.ts';

export const adapter: Adapter = {
  name: 'llm docs site',
  site: {
    buildDir: '../website/build',
    basePath: '/llm/',
    sitemap: 'sitemap.xml',
    home: '/llm/',
  },
  viewports: {
    gating: [
      { name: '1440x900', width: 1440, height: 900 },
      { name: '390x844', width: 390, height: 844 },
    ],
    measured: [
      { name: '1280x800', width: 1280, height: 800 },
      { name: '1024x768', width: 1024, height: 768 },
      { name: '768x1024', width: 768, height: 1024 },
    ],
  },
  themes: ['light', 'dark'],
  themeAttribute: 'data-theme',
  alwaysVisible: [
    { name: 'navbar', locate: { role: 'navigation', name: 'Main' } },
    { name: 'search', locate: { role: 'searchbox', name: /search/i, css: '.navbar__search-input, .navbar__search, .DocSearch-Button, [role="search"]', scope: '.navbar' } },
    { name: 'theme toggle', locate: { role: 'button', name: /dark and light mode/i } },
    { name: 'sidebar', locate: { role: 'navigation', name: 'Docs sidebar' }, minWidth: 997, pathPrefix: '/llm/docs' },
  ],
  primaryRegion: 'main',
  scrollContainersAllowed: 'pre, table, .theme-code-block, [class*="codeBlock"], .table-wrapper, .mermaid, svg',
  guidanceCue: null,
  protectedFocus: null,
  axeTags: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa'],
  axeExclude: '.theme-code-block pre',
  axeSample: '/llm/docs/getting-started',
  axeThemeRules: ['color-contrast', 'link-in-text-block'],
  checkSettleMs: 250,
  keyboard: { skipLink: /skip to (main )?content/i, mustReach: ['search', 'navbar'], maxTabs: 40 },
  hubWords: ['documentation', 'docs', 'overview', 'reference', 'guides'],
  screenOnly: { maxActions: 15, maxScrolls: 6, viewport: '1440x900', theme: 'light' },
  settleMs: 750,
  dirs: { scenarios: 'scenarios', findings: 'findings', rounds: 'rounds', baseline: 'baseline.json' },
};
