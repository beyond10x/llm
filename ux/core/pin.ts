// What the page checks run with, each read from its one place: the ux-browser release from the
// "Install ux-browser" step of the docs UX workflow, the Chrome build and the Chromium switches from a
// checks configuration, the build again from the list of browsers the installed Playwright carries.
// Each reader takes the file's text and throws on a form it does not read; none returns a default.

/** The workflow that holds the ux-browser pin, from the repository root. */
export const PIN_WORKFLOW = '.github/workflows/docs-ux.yml';

/** The pinned ux-browser version: the one line `UX_BROWSER_VERSION: "<n>.<n>.<n>"` of the workflow's text. */
export function pinnedVersion(workflow: string): string {
  const found = workflow.split('\n').flatMap((line) => /^\s+UX_BROWSER_VERSION: "(\d+\.\d+\.\d+)"\s*$/.exec(line)?.[1] ?? []);
  if (found.length !== 1) throw new Error(`${PIN_WORKFLOW}: ${found.length} lines of the form \`UX_BROWSER_VERSION: "<n>.<n>.<n>"\`, needed exactly one`);
  return found[0];
}

/** The Chrome build a checks configuration pins: its one line `chrome: {build: <n>}`. */
export function chromeBuild(config: string): number {
  const found = config.split('\n').flatMap((line) => /^chrome: \{build: ([1-9]\d*)\}\s*$/.exec(line)?.[1] ?? []);
  if (found.length !== 1) throw new Error(`${found.length} lines of the form \`chrome: {build: <n>}\` in the checks configuration, needed exactly one`);
  return Number(found[0]);
}

/**
 * The switches a checks configuration adds to the ones ux-browser starts Chromium with: its one line
 * `chrome_args: [--<switch>, ...]`. A configuration without the line, with two lines that name
 * `chrome_args`, or with the list in another form (a block list, a quoted switch, no switch) is refused.
 */
export function chromeArgs(config: string): string[] {
  const named = config.split('\n').filter((line) => /^\s*['"]?chrome_args\b/.test(line));
  const found = named.flatMap((line) => {
    const list = /^chrome_args: \[([^\]]*)\]\s*$/.exec(line)?.[1].split(',').map((s) => s.trim());
    return list?.every((s) => /^--[a-z0-9][a-z0-9-]*(=[^\s,'"#[\]{}]+)?$/.test(s)) ? [list] : [];
  });
  if (named.length !== 1 || found.length !== 1)
    throw new Error(`${named.length} lines name \`chrome_args\` in the checks configuration and ${found.length} of them have the form \`chrome_args: [--<switch>, ...]\`, needed exactly one line, of that form`);
  return found[0];
}

/** The Chromium build Playwright installs: the revision of `chromium` in the text of playwright-core's `browsers.json`. */
export function playwrightChromium(browsersJson: string): number {
  const browsers = (JSON.parse(browsersJson) as { browsers?: { name?: string; revision?: string }[] }).browsers ?? [];
  const found = browsers.filter((b) => b.name === 'chromium').map((b) => b.revision ?? '');
  if (found.length !== 1 || !/^[1-9]\d*$/.test(found[0])) throw new Error(`browsers.json names ${found.length} chromium revisions (${found.join(', ')}), needed exactly one whole number`);
  return Number(found[0]);
}
