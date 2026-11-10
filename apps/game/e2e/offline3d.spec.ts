// The 3D view works offline (P10 acceptance, doc 06 §9): after one online visit the service
// worker holds the app shell, the lazy 3D chunk, Cesium's self-hosted workers and assets and the
// class models; the packs live in OPFS. Runs in the `offline` project against a production
// build served by `vite preview`:
//   WB_E2E_OFFLINE=1 pnpm --filter game e2e --project offline
import { expect, test } from '@playwright/test';
import { installWorld, open3d, quickStartPaused } from './helpers3d';

test('after one visit, the 3D view opens and draws with the network off', async ({ page, context }) => {
  await page.goto('/');
  await installWorld(page);
  // The service worker has installed and cached everything it lists.
  await page.waitForFunction(async () => {
    const reg = await navigator.serviceWorker.getRegistration();
    return !!reg?.active;
  }, null, { timeout: 120_000 });
  await page.reload();
  await page.waitForFunction(() => !!navigator.serviceWorker.controller, null, { timeout: 60_000 });

  await context.setOffline(true);
  await page.reload();
  await quickStartPaused(page);
  await open3d(page);
  // The picture is drawn on the globe and a model loads close up (all from the cache).
  await expect.poll(() => page.evaluate(() => (window as unknown as { __wbGlobe: { visible: () => string[] } }).__wbGlobe.visible().length)).toBeGreaterThan(0);
  await page.locator('.forces').getByText('2 × MF-4 Sellis').click();
  await page.getByRole('button', { name: 'INSPECT', exact: true }).click();
  await expect(page.getByRole('status', { name: 'Inspect' })).toBeVisible();
  await expect(page.locator('.toast', { hasText: /failed/i })).toHaveCount(0);
});
