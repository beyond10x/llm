// A small site for `l1-page-checks.spec.ts`: one clean page and one page per planted defect, each
// the clean page with one thing changed. It is laid out as ux-browser reads this repository: the
// build under `site/`, the checks configuration at `ux/ux-browser.checks.yaml`, one page scenario
// per page under `ux/scenarios/`, and no finding record, so nothing is waived.

import { copyFileSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

/** The site follows the colour scheme the browser asks for, as the docs site does. */
const THEME =
  "<script>const m=matchMedia('(prefers-color-scheme: dark)');" +
  "const a=()=>document.documentElement.setAttribute('data-theme',m.matches?'dark':'light');a();m.addEventListener('change',a);</script>";

const STYLE =
  '<style>body{margin:0;font:16px/1.4 sans-serif}.skip{position:fixed;top:-60px;left:0}.skip:focus{top:0}' +
  '.navbar{position:fixed;top:0;left:0;right:0;height:40px;background:#eee}main{padding:60px 8px 8px}</style>';

const SKIP = '<a class="skip" href="#content">Skip to main content</a>';
const SEARCH = '<input type="search" class="navbar__search-input" aria-label="Search" style="width:120px">';

interface Parts {
  skip?: string;
  search?: string;
  main?: string;
  script?: string;
}

function page(title: string, p: Parts = {}): string {
  return (
    `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">` +
    `<title>${title}</title>${THEME}${STYLE}</head><body>${p.skip ?? SKIP}` +
    `<nav class="navbar" aria-label="Main"><a href="/llm/clean">Home</a> ${p.search ?? SEARCH} ` +
    `<button type="button" aria-label="Switch between dark and light mode">mode</button></nav>` +
    `<main id="content" tabindex="-1"><h1>${title}</h1><p>Some text of the page.</p>${p.main ?? ''}</main>${p.script ?? ''}</body></html>`
  );
}

/** Page name to its HTML. `clean` has no defect; every other page has the one its name says. */
export const PAGES: Record<string, string> = {
  clean: page('Clean'),
  // The document is wider than the viewport, and no block or media element is.
  'page-scroll': page('Page scroll', { main: '<div style="width:3000px;height:4px"></div>' }),
  // A line that does not wrap runs out of its 120 px block; the page itself still fits.
  overflow: page('Overflow', { main: '<div id="narrow" style="width:120px;white-space:nowrap">a line that does not wrap here</div>' }),
  // No search box in the navbar.
  missing: page('Missing', { search: '' }),
  // The first tab stop is a navbar link, not a skip link.
  keyboard: page('Keyboard', { skip: '' }),
  // Content keeps arriving above the text after the page is ready.
  shift: page('Shift', {
    script:
      "<script>addEventListener('load',()=>{let n=0;const t=setInterval(()=>{const d=document.createElement('div');d.style.height='90px';" +
      "d.textContent='late';const m=document.getElementById('content');m.insertBefore(d,m.firstChild);if(++n>=12)clearInterval(t)},60)})</script>",
  }),
};

export const pagePath = (name: string) => `/llm/${name}`;

/** Writes the fixture under `root`, with `config` as its checks configuration, and returns the build directory. */
export function writeFixture(root: string, config: string): string {
  rmSync(root, { recursive: true, force: true });
  const site = join(root, 'site');
  mkdirSync(site, { recursive: true });
  mkdirSync(join(root, 'ux/findings'), { recursive: true });
  copyFileSync(config, join(root, 'ux/ux-browser.checks.yaml'));
  for (const [name, html] of Object.entries(PAGES)) {
    writeFileSync(join(site, `${name}.html`), html);
    mkdirSync(join(root, 'ux/scenarios', `page-${name}`), { recursive: true });
    writeFileSync(join(root, 'ux/scenarios', `page-${name}`, 'page.json'), JSON.stringify({ kind: 'page', path: pagePath(name) }) + '\n');
  }
  writeFileSync(join(site, 'index.html'), PAGES.clean);
  writeFileSync(
    join(site, 'sitemap.xml'),
    '<?xml version="1.0" encoding="UTF-8"?><urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">' +
      Object.keys(PAGES).map((n) => `<url><loc>https://fixture.test${pagePath(n)}</loc></url>`).join('') +
      '</urlset>\n',
  );
  return site;
}
