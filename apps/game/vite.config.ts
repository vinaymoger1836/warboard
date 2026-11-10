import react from '@vitejs/plugin-react';
import { createHash } from 'node:crypto';
import { createReadStream, existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import type { ServerResponse } from 'node:http';
import { join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig, normalizePath, type Connect, type Plugin } from 'vite';

const repoRoot = fileURLToPath(new URL('../..', import.meta.url));
const appRoot = fileURLToPath(new URL('.', import.meta.url));
const packsDir = resolve(repoRoot, 'build', 'packs');
// CesiumJS's static files (doc 06 §5): web workers, decoders and the few assets the engine
// fetches itself. Self-hosted at ./cesium/, never from a CDN or Cesium ion. The default
// textures (sky box, bundled imagery) and the widget CSS are not shipped: the 3D view draws
// its own globe texture and has no Cesium widgets (doc 10 §12).
const cesiumBuild = resolve(appRoot, 'node_modules', '@cesium', 'engine', 'Build');
const CESIUM_DIRS = ['Workers', 'ThirdParty', 'Assets/Images', 'Assets/IAU2006_XYS', 'Assets/approximateTerrainHeights.json'];

// The war the engine worker opens, and its content pack (paths from the repo root). The
// hosted build sets both to the neutral test war on the fixtures (ADR-040); only the chosen
// scenario is bundled.
const warScenario = normalizePath(resolve(repoRoot, process.env.WB_SCENARIO ?? 'scenarios/quickstart-north-straits.yaml'));
const warContent = normalizePath(resolve(repoRoot, process.env.WB_CONTENT_PACK ?? 'build/content.wbpack'));

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

/** The Cesium files to ship, as paths relative to `cesiumBuild`. */
function cesiumFiles(): string[] {
  return CESIUM_DIRS.flatMap((d) => {
    const full = resolve(cesiumBuild, d);
    if (!existsSync(full)) return [];
    return statSync(full).isDirectory() ? listFiles(full).map((f) => relative(cesiumBuild, f).split(sep).join('/')) : [d];
  });
}

const MIME: Record<string, string> = {
  '.js': 'text/javascript',
  '.json': 'application/json',
  '.wasm': 'application/wasm',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.svg': 'image/svg+xml',
};

/** Serves the Cesium files at ./cesium/ in dev, and emits them into the build (where the
 *  service worker precaches them, so the 3D view works offline). */
function cesiumAssets(): Plugin {
  let files: Set<string> | null = null;
  const handler: Connect.NextHandleFunction = (req, res: ServerResponse, next) => {
    const url = new URL(req.url ?? '/', 'http://local');
    const at = url.pathname.indexOf('/cesium/');
    if (at < 0) return next();
    const rel = decodeURIComponent(url.pathname.slice(at + '/cesium/'.length));
    files ??= new Set(cesiumFiles());
    if (!files.has(rel)) return next();
    res.setHeader('Content-Type', MIME[rel.slice(rel.lastIndexOf('.'))] ?? 'application/octet-stream');
    createReadStream(resolve(cesiumBuild, rel)).pipe(res);
  };
  return {
    name: 'warboard-cesium-assets',
    configureServer: (server) => void server.middlewares.use(handler),
    generateBundle() {
      for (const f of cesiumFiles()) {
        this.emitFile({ type: 'asset', fileName: `cesium/${f}`, source: readFileSync(resolve(cesiumBuild, f)) });
      }
    },
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
  // cesiumAssets() before serviceWorker(): the precache list is taken from the bundle.
  plugins: [react(), servePacks(), cesiumAssets(), serviceWorker()],
  worker: { format: 'es' },
  resolve: {
    // Regexes, so the `?raw` / `?url` query after the name is kept.
    alias: [
      { find: /^@war\/scenario\.yaml/, replacement: warScenario },
      { find: /^@war\/content\.wbpack/, replacement: warContent },
    ],
  },
  // The worker imports scenarios/*.yaml and the wasm package from outside the app folder.
  server: { fs: { allow: [repoRoot] } },
  build: { target: 'es2023', chunkSizeWarningLimit: 2500 },
});
