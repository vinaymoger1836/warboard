import react from '@vitejs/plugin-react';
import { createHash } from 'node:crypto';
import { createReadStream, existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import type { ServerResponse } from 'node:http';
import { join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig, type Connect, type Plugin } from 'vite';

const repoRoot = fileURLToPath(new URL('../..', import.meta.url));
const appRoot = fileURLToPath(new URL('.', import.meta.url));
const packsDir = resolve(repoRoot, 'build', 'packs');

/**
 * Serves build/packs/ at /packs/ in dev and preview, standing in for the release host
 * (doc 06 §9). Supports single byte ranges so a large download can be inspected.
 */
function servePacks(): Plugin {
  const handler: Connect.NextHandleFunction = (req, res: ServerResponse, next) => {
    const url = new URL(req.url ?? '/', 'http://local');
    if (!url.pathname.startsWith('/packs/')) return next();
    const path = resolve(packsDir, decodeURIComponent(url.pathname.slice('/packs/'.length)));
    if (!path.startsWith(packsDir + sep) || !existsSync(path) || !statSync(path).isFile()) {
      res.statusCode = 404;
      res.end('Not found');
      return;
    }
    const size = statSync(path).size;
    res.setHeader('Content-Type', path.endsWith('.json') ? 'application/json' : 'application/octet-stream');
    res.setHeader('Cache-Control', 'no-store');
    res.setHeader('Accept-Ranges', 'bytes');
    const range = /^bytes=(\d+)-(\d*)$/.exec(req.headers.range ?? '');
    if (range) {
      const start = Number(range[1]);
      const end = range[2] ? Math.min(Number(range[2]), size - 1) : size - 1;
      res.statusCode = 206;
      res.setHeader('Content-Range', `bytes ${start}-${end}/${size}`);
      res.setHeader('Content-Length', String(end - start + 1));
      createReadStream(path, { start, end }).pipe(res);
      return;
    }
    res.setHeader('Content-Length', String(size));
    createReadStream(path).pipe(res);
  };
  return {
    name: 'warboard-serve-packs',
    configureServer: (server) => void server.middlewares.use(handler),
    configurePreviewServer: (server) => void server.middlewares.use(handler),
  };
}

function listFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? listFiles(join(dir, e.name)) : [join(dir, e.name)],
  );
}

/**
 * Emits sw.js with the list of every built and public file, so the app shell works offline
 * after the first visit (doc 06 §9). Packs are not precached; they live in OPFS.
 */
function serviceWorker(): Plugin {
  return {
    name: 'warboard-service-worker',
    apply: 'build',
    generateBundle(_options, bundle) {
      const built = Object.keys(bundle);
      const publicDir = resolve(appRoot, 'public');
      const pub = existsSync(publicDir)
        ? listFiles(publicDir).map((f) => relative(publicDir, f).split(sep).join('/'))
        : [];
      const files = ['./', ...[...built, ...pub].filter((f) => !f.endsWith('.map')).sort()].map((f) =>
        f === './' ? f : `./${f.split('/').map(encodeURIComponent).join('/')}`,
      );
      const version = createHash('sha256').update(JSON.stringify(files)).digest('hex').slice(0, 16);
      const template = readFileSync(resolve(appRoot, 'sw', 'sw.js'), 'utf8');
      this.emitFile({
        type: 'asset',
        fileName: 'sw.js',
        source: template
          .replace('__PRECACHE__', JSON.stringify(files))
          .replace('__VERSION__', JSON.stringify(version)),
      });
    },
  };
}

export default defineConfig({
  base: './',
  plugins: [react(), servePacks(), serviceWorker()],
  worker: { format: 'es' },
  // The worker imports scenarios/*.yaml and the wasm package from outside the app folder.
  server: { fs: { allow: [repoRoot] } },
  build: { target: 'es2023', chunkSizeWarningLimit: 2500 },
});
