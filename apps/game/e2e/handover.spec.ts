// Hot-seat handover (doc 12 §3, §5): the handover screen renders no map, panels, alerts or toasts,
// and the clock does not advance while it shows; a PIN is asked on Continue; a hot-seat save
// reopens at the active side's handover screen. Wars are set up through the real setup screens.
import { expect, type Page, test } from '@playwright/test';

/** Everything that must not be on screen while the device is being handed over. */
const BOARD = '.map-root, .maplibregl-map, canvas, .top-bar, .dock, .forces, .deploy-panel, .unit-card, .toasts, .toast, .umpire-banner, .context-menu';

/** The sim tick from the bridge's latest clock report (side views only, never truth). */
const tick = (page: Page) =>
  page.evaluate(() => (window as unknown as { __wbBridge?: { report: { tick: number } | null } }).__wbBridge?.report?.tick ?? -1);

/** NEW WAR → Coalitions → Players (both human, two players, optional PIN for B) → Start → Check → START WAR. */
async function startHotSeat(page: Page, pinB?: string) {
  await page.goto('/');
  await page.getByRole('button', { name: 'NEW WAR' }).click();
  await page.getByRole('button', { name: 'NEXT' }).click();
  await page.getByRole('group', { name: /^Who plays Coalition B/ }).getByRole('button', { name: 'HUMAN' }).click();
  await page.getByRole('button', { name: 'TWO PLAYERS (HOT-SEAT)' }).click();
  if (pinB) await page.getByPlaceholder('none').nth(1).fill(pinB);
  await page.getByRole('button', { name: 'NEXT' }).click();
  await page.getByRole('button', { name: 'NEXT' }).click();
  // A fresh browser profile has no region packs: start without them (the check offers this).
  const start = page.getByRole('button', { name: 'START WAR' });
  const without = page.getByLabel('Start without the region pack');
  await expect(start.or(without).first()).toBeVisible();
  if (await without.isVisible()) await without.check();
  await start.click();
}

async function expectHandoverOnly(page: Page, nation: RegExp) {
  await expect(page.getByTestId('handover')).toBeVisible();
  await expect(page.getByText(nation)).toBeVisible();
  await expect(page.locator(BOARD)).toHaveCount(0);
  // No alert or toast text, and no reason for the handover.
  await expect(page.locator('[role="status"], [role="alert"]')).toHaveCount(0);
  // The clock is paused while the screen shows.
  const before = await tick(page);
  await page.waitForTimeout(2_500);
  expect(await tick(page)).toBe(before);
  await expect(page.locator(BOARD)).toHaveCount(0);
}

test('the handover screen shows nothing of the war and holds the clock', async ({ page }) => {
  await startHotSeat(page);
  // A new hot-seat war opens at the first side's handover screen.
  await expectHandoverOnly(page, /Pass the device to the Coalition A player/);

  await page.getByRole('button', { name: 'CONTINUE' }).click();
  await expect(page.locator('.top-bar')).toBeVisible();
  const running = await tick(page);
  await expect.poll(() => tick(page), { timeout: 15_000 }).toBeGreaterThan(running);

  // HAND OVER: everything goes, the clock stops.
  await page.getByRole('button', { name: 'HAND OVER' }).click();
  await expectHandoverOnly(page, /Pass the device to the Coalition B player/);
  await page.getByRole('button', { name: 'CONTINUE' }).click();
  await expect(page.locator('.top-bar')).toBeVisible();
  await expect(page.getByRole('button', { name: /VSM/ })).toHaveAttribute('aria-pressed', 'true');
});

test('a PIN is asked on Continue and a hot-seat save reopens at the handover screen', async ({ page }) => {
  await startHotSeat(page, '4821');
  await page.getByRole('button', { name: 'CONTINUE' }).click();
  await expect(page.locator('.top-bar')).toBeVisible();

  await page.getByRole('button', { name: 'HAND OVER' }).click();
  await expect(page.getByTestId('handover')).toBeVisible();
  const continueButton = page.getByRole('button', { name: 'CONTINUE' });
  await expect(continueButton).toBeDisabled();
  await page.getByLabel('PIN').fill('1111');
  await continueButton.click();
  await expect(page.getByText('That PIN is not right')).toBeVisible();
  await expect(page.locator(BOARD)).toHaveCount(0);
  await page.getByLabel('PIN').fill('4821');
  await continueButton.click();
  await expect(page.locator('.top-bar')).toBeVisible();

  // Save on side B, reload, load: the war reopens at side B's handover screen, PIN kept.
  await page.evaluate(async () => {
    const b = (window as unknown as { __wbBridge: { request: (r: unknown) => Promise<unknown> } }).__wbBridge;
    await b.request({ type: 'save', slot: 'slot-3' });
  });
  await page.reload();
  await expect(page.getByRole('button', { name: 'NEW WAR' })).toBeVisible();
  await page.evaluate(async () => {
    const b = (window as unknown as { __wbBridge: { request: (r: unknown) => Promise<unknown> } }).__wbBridge;
    await b.request({ type: 'load', slot: 'slot-3' });
  });
  await expectHandoverOnly(page, /Pass the device to the Coalition B player/);
  await expect(continueButton).toBeDisabled();
  await page.getByLabel('PIN').fill('4821');
  await continueButton.click();
  await expect(page.locator('.top-bar')).toBeVisible();
});
