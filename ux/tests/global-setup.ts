import { rmSync } from 'node:fs';
import { resolve } from 'node:path';
import { adapter } from '../adapter.ts';
import { serve } from '../core/site.ts';

export default async function globalSetup(): Promise<() => Promise<void>> {
  const out = resolve(import.meta.dirname, '..', process.env.UX_OUT ?? '.out/check');
  rmSync(resolve(out, 'results'), { recursive: true, force: true });
  const root = resolve(import.meta.dirname, '..', adapter.site.buildDir);
  const { url, server } = await serve(root, adapter.site.basePath);
  process.env.UX_BASE_URL = url;
  return async () => {
    await new Promise<void>((ok) => server.close(() => ok()));
  };
}
