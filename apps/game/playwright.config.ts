// Browser tests (P9, doc 12 §5): flows that only a real browser can check, such as the hot-seat
// handover screen. They run against the dev server on the installed Chrome (no browser
// download). Needs `pnpm gen` first, like `pnpm --filter game dev`.
import { defineConfig } from '@playwright/test';

const PORT = 5174;

export default defineConfig({
  testDir: 'e2e',
  timeout: 120_000,
  expect: { timeout: 30_000 },
  fullyParallel: false,
  workers: 1,
  reporter: [['list']],
  use: {
    baseURL: `http://localhost:${PORT}`,
    channel: 'chrome',
    viewport: { width: 1440, height: 900 },
  },
  webServer: {
    command: `pnpm exec vite --port ${PORT} --strictPort`,
    url: `http://localhost:${PORT}`,
    reuseExistingServer: true,
    timeout: 120_000,
  },
});
