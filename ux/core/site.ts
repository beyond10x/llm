// A static server for a built site under its base path, and the page list from its sitemap.

import { createServer, type Server } from 'node:http';
import { readFileSync, statSync, existsSync } from 'node:fs';
import { extname, join, normalize, resolve } from 'node:path';

const TYPES: Record<string, string> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.webp': 'image/webp',
  '.ico': 'image/x-icon',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
  '.ttf': 'font/ttf',
  '.xml': 'application/xml',
  '.txt': 'text/plain; charset=utf-8',
};

function isFile(p: string): boolean {
  try {
    return statSync(p).isFile();
  } catch {
    return false;
  }
}

/** Resolve a URL path to a file the way a static host with trailingSlash false does. */
export function resolveFile(root: string, basePath: string, urlPath: string): string | null {
  if (!urlPath.startsWith(basePath) && urlPath + '/' !== basePath) return null;
  const rel = decodeURIComponent(urlPath.slice(basePath.length - 1));
  const safe = normalize(rel).replace(/^(\.\.[/\\])+/, '');
  const base = join(root, safe);
  if (!resolve(base).startsWith(resolve(root))) return null;
  for (const c of [base, base + '.html', join(base, 'index.html')]) if (isFile(c)) return c;
  return null;
}

export async function serve(root: string, basePath: string): Promise<{ url: string; server: Server }> {
  if (!existsSync(join(root, 'index.html'))) throw new Error(`no built site at ${root}: build it first`);
  const server = createServer((req, res) => {
    const u = new URL(req.url ?? '/', 'http://x');
    const file = resolveFile(root, basePath, u.pathname);
    if (!file) {
      const nf = join(root, '404.html');
      res.writeHead(404, { 'content-type': TYPES['.html'] });
      res.end(isFile(nf) ? readFileSync(nf) : 'not found');
      return;
    }
    res.writeHead(200, { 'content-type': TYPES[extname(file)] ?? 'application/octet-stream' });
    res.end(readFileSync(file));
  });
  await new Promise<void>((ok) => server.listen(0, '127.0.0.1', ok));
  const addr = server.address();
  if (!addr || typeof addr === 'string') throw new Error('server has no port');
  return { url: `http://127.0.0.1:${addr.port}`, server };
}

/** The site's pages as paths under the base path, sorted, from sitemap.xml. */
export function sitemapPaths(root: string, sitemap: string): string[] {
  const xml = readFileSync(join(root, sitemap), 'utf8');
  const paths = [...xml.matchAll(/<loc>([^<]+)<\/loc>/g)].map((m) => new URL(m[1]).pathname);
  return [...new Set(paths)].sort();
}

/** A scenario directory name for a page path: page-<slug>. */
export function pageSlug(path: string, basePath: string): string {
  const rest = path.slice(basePath.length).replace(/\/+$/, '');
  return 'page-' + (rest === '' ? 'home' : rest.replace(/[^a-z0-9]+/gi, '-').toLowerCase());
}
