// Shared steps of the 3D view tests (P10): install the world pack from the dev server into the
// fresh browser profile, start the quick-start war, and reach the map and globe test hooks.
import { expect, type Page } from '@playwright/test';

/** What the 2D map and the 3D view expose for the parity test (UnitLayers, Map3D). */
export interface ViewHooks {
  selectables: () => string[];
  windowPosition: (id: string) => { x: number; y: number } | null;
  pickable: (id: string) => boolean;
}

/** The world pack (needed for any map) from build/packs/, served by the dev server. */
export async function installWorld(page: Page): Promise<void> {
  await page.getByRole('button', { name: 'MAP PACKS' }).click();
  const row = page.locator('.pack-row', { hasText: 'World' });
  const remove = row.getByRole('button', { name: 'REMOVE' });
  if (!(await remove.isVisible())) await row.getByRole('button', { name: 'INSTALL' }).click();
  await expect(remove).toBeVisible({ timeout: 120_000 });
  await page.getByRole('button', { name: 'Close region packs' }).click();
}

/** QUICK START, run until the MF-4 flights are airborne, then pause so the picture holds still. */
export async function quickStartPaused(page: Page): Promise<void> {
  await page.getByRole('button', { name: 'QUICK START' }).click();
  await expect(page.locator('.top-bar')).toBeVisible();
  await expect(page.locator('.map-root')).toBeVisible();
  await expect(page.locator('.force-row', { hasText: 'MF-4 Sellis' }).filter({ hasText: 'TRANSIT' }).first()).toBeVisible({ timeout: 60_000 });
  await page.getByRole('button', { name: 'Pause', exact: true }).click();
  // Hide the dock: the map gets the full height.
  const hide = page.getByRole('button', { name: 'HIDE' });
  if (await hide.isVisible()) await hide.click();
}

export const tick = (page: Page) =>
  page.evaluate(() => (window as unknown as { __wbBridge?: { report: { tick: number } | null } }).__wbBridge?.report?.tick ?? -1);

export async function open3d(page: Page): Promise<void> {
  await page.getByRole('button', { name: 'OPEN 3D TACTICAL VIEW' }).click();
  await expect(page.getByRole('button', { name: 'OPEN 2D MAP' })).toBeVisible();
  await page.waitForFunction(() => !!(window as unknown as { __wbGlobe?: unknown }).__wbGlobe, null, { timeout: 60_000 });
  // A few rendered frames so the renderer has drawn the picture.
  await page.waitForFunction(() => ((window as unknown as { __wbGlobe: ViewHooks }).__wbGlobe.selectables().length > 0), null, { timeout: 60_000 });
}

export async function open2d(page: Page): Promise<void> {
  await page.getByRole('button', { name: 'OPEN 2D MAP' }).click();
  await expect(page.getByRole('button', { name: 'OPEN 3D TACTICAL VIEW' })).toBeVisible();
  await page.waitForFunction(() => !!(window as unknown as { __wbMap2d?: unknown }).__wbMap2d);
}
