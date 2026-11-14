// Touch mode and the credits screen (P12, doc 06 §11, ADR-050): on a touch screen the unit card is
// a bottom sheet with the doc 10 §7 layout, hit targets are at least 44 px, a long-press on the
// 2D map opens the context menu, and the credits screen lists the installed pack's sources.
// `?input=touch` forces the mode, so the test needs no device. Needs the world pack from
// build/packs (installed through the pack manager like the 3D tests do).
import { expect, test } from '@playwright/test';
import { installWorld, quickStartPaused } from './helpers3d';

test.use({ hasTouch: true, viewport: { width: 900, height: 420 } });

test('touch mode: bottom-sheet unit card, 44 px targets, long-press menu, credits', async ({ page }) => {
  await page.goto('/?input=touch');
  await installWorld(page);
  await quickStartPaused(page);
  await expect(page.locator('html')).toHaveAttribute('data-input', 'touch');

  // The unit card: along the bottom edge, full width, opening on its header, status rows and buttons.
  await page.locator('.force-row').first().click();
  const card = page.getByRole('complementary', { name: 'Unit card' });
  await expect(card).toBeVisible();
  const box = await card.boundingBox();
  expect(box).not.toBeNull();
  if (!box) return;
  expect(Math.round(box.x)).toBe(0);
  expect(Math.round(box.x + box.width)).toBe(900);
  expect(Math.round(box.y + box.height)).toBe(420);
  await expect(card).toHaveAttribute('data-sheet', 'peek');
  await expect(card.locator('.card-tabs')).toBeHidden();
  await card.getByRole('button', { name: 'Show the full card' }).click();
  await expect(card).toHaveAttribute('data-sheet', 'open');
  await expect(card.locator('.card-tabs')).toBeVisible();

  // Hit targets: the card's buttons and tabs are at least 44 px high.
  for (const sel of ['.card-tabs .tab', '.card-buttons .cmd']) {
    const first = card.locator(sel).first();
    if (await first.count()) {
      const b = await first.boundingBox();
      expect(b?.height ?? 0).toBeGreaterThanOrEqual(44);
    }
  }
  await card.getByRole('button', { name: 'Show less of the card' }).click();

  // Long-press (500 ms, one finger, held still) on the map opens the context menu.
  const map = await page.locator('.map-canvas').boundingBox();
  expect(map).not.toBeNull();
  if (!map) return;
  const x = Math.round(map.x + map.width * 0.7);
  const y = Math.round(map.y + map.height * 0.3);
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x, y }] });
  await page.waitForTimeout(800);
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
  await expect(page.locator('.context-menu')).toBeVisible();
  const item = await page.locator('.context-menu button').first().boundingBox();
  expect(item?.height ?? 0).toBeGreaterThanOrEqual(44);

  // A short tap is not a long-press: the menu closes on a tap elsewhere and does not reopen.
  await page.mouse.click(Math.round(map.x + map.width * 0.5), Math.round(map.y + map.height * 0.25));
  await expect(page.locator('.context-menu')).toBeHidden();

  // Credits: the installed pack's sources and attribution lines come from its manifest.
  await page.getByRole('button', { name: /Credits/ }).click();
  const credits = page.getByRole('dialog', { name: 'Credits' });
  await expect(credits).toBeVisible();
  await expect(credits).toContainText('World');
  await expect(credits).toContainText('Protomaps');
  await expect(credits).toContainText('ODbL');
  await expect(credits).toContainText('fictional');
});
