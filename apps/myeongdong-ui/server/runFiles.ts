import { open, realpath } from 'node:fs/promises';
import { resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import type { Plugin } from 'vite';

const runRoot = fileURLToPath(new URL('../public/runs/', import.meta.url));

export function parseRunPath(url: string): { runId: string; file: string } | null {
  const path = url.split('?', 1)[0];
  const match = /^\/runs\/([A-Za-z0-9][A-Za-z0-9._-]{0,80})\/(run_status\.json|frames\/frame_\d{4}\.bin)$/.exec(path);
  if (!match || match[1].includes('..')) return null;
  return { runId: match[1], file: match[2] };
}

/** Generated files are served without involving Vite's source watcher. */
export function runFilesPlugin(): Plugin {
  return {
    name: 'myeongdong-run-files',
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        if (!req.url?.startsWith('/runs/')) return next();
        const parsed = parseRunPath(req.url);
        if (!parsed || (req.method !== 'GET' && req.method !== 'HEAD')) {
          res.writeHead(404); res.end(); return;
        }
        void (async () => {
          let handle;
          try {
            const root = await realpath(runRoot);
            const target = await realpath(resolve(root, parsed.runId, parsed.file));
            if (!target.startsWith(root + sep)) throw new Error('invalid path');
            handle = await open(target, 'r');
            const info = await handle.stat();
            if (!info.isFile()) throw new Error('not a file');
            res.writeHead(200, {
              'Content-Type': parsed.file.endsWith('.json') ? 'application/json; charset=utf-8' : 'application/octet-stream',
              'Content-Length': info.size,
              'Cache-Control': 'no-store',
              'X-Content-Type-Options': 'nosniff',
            });
            if (req.method === 'HEAD') { await handle.close(); res.end(); return; }
            handle.createReadStream().on('error', () => res.destroy()).pipe(res);
          } catch {
            if (handle) await handle.close().catch(() => {});
            if (!res.headersSent) { res.writeHead(404); res.end(); }
            else res.destroy();
          }
        })();
      });
    },
  };
}
