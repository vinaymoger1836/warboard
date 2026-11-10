// Browser tests (P9, doc 12 §5; P10, doc 09): flows that only a real browser can check, such
// as the hot-seat handover screen and the 3D view. They run against the dev server on the
// installed Chrome (no browser download). Needs `pnpm gen` first, like `pnpm --filter game dev`.
//
// Projects:
//   default  — the dev server with the quick-start war (`pnpm --filter game e2e`).
//   perf3d   — a second dev server whose quick start is the 500-unit 3D benchmark scene
//              (`WB_E2E_PERF=1 pnpm --filter game e2e --project perf3d`).
//   offline  — a production build served by `vite preview`, with the service worker
//              (`WB_E2E_OFFLINE=1 pnpm --filter game e2e --project offline`).
// The extra servers start only when asked for, so the default run builds nothing.
import { defineConfig } from '@playwright/test';

const PORT = 5174;
const PERF_PORT = 5175;
const OFFLINE_PORT = 5176;
const PERF = !!process.env.WB_E2E_PERF;
const OFFLINE = !!process.env.WB_E2E_OFFLINE;
const OFFLINE_DIR = '../../build/e2e-offline';

export default defineConfig({
  testDir: 'e2e',
  timeout: 180_000,
  expect: { timeout: 30_000 },
  fullyParallel: false,
  workers: 1,
  reporter: [['list']],
  use: {
    channel: 'chrome',
    viewport: { width: 1440, height: 900 },
  },
  projects: [
    { name: 'default', testIgnore: [/perf3d\.spec\.ts/, /offline3d\.spec\.ts/], use: { baseURL: `http://localhost:${PORT}` } },
    ...(PERF ? [{
      name: 'perf3d',
      testMatch: /perf3d\.spec\.ts/,
      // Frame rate needs the GPU: headed, so Chrome does not fall back to software WebGL.
      use: { baseURL: `http://localhost:${PERF_PORT}`, headless: false },
    }] : []),
    ...(OFFLINE ? [{ name: 'offline', testMatch: /offline3d\.spec\.ts/, use: { baseURL: `http://localhost:${OFFLINE_PORT}` } }] : []),
  ],
  webServer: [
    {
      command: `pnpm exec vite --port ${PORT} --strictPort`,
      url: `http://localhost:${PORT}`,
      reuseExistingServer: true,
      timeout: 120_000,
    },
    ...(PERF ? [{
      command: `pnpm exec vite --port ${PERF_PORT} --strictPort`,
      url: `http://localhost:${PERF_PORT}`,
      env: { WB_SCENARIO: 'scenarios/bench/view-500.yaml' },
      reuseExistingServer: true,
      timeout: 120_000,
    }] : []),
    ...(OFFLINE ? [{
      command: `pnpm exec vite build --outDir ${OFFLINE_DIR} --emptyOutDir && pnpm exec vite preview --outDir ${OFFLINE_DIR} --port ${OFFLINE_PORT} --strictPort`,
      url: `http://localhost:${OFFLINE_PORT}`,
      reuseExistingServer: true,
      timeout: 300_000,
    }] : []),
  ],
});
