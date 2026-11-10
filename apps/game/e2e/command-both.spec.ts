// Command both (doc 12 §2, §5): Tab switches sides at once, with no handover screen, and each
// side gets back its own selection and open panels. The umpire view (turned on by the player)
// shows both sides, read-only, with its banner. (Each side's camera is checked by hand in the
// P9 report: the test profile has no world pack, so there is no map.)
import { expect, type Page, test } from '@playwright/test';

async function startCommandBoth(page: Page) {
  await page.goto('/');
  await page.getByRole('button', { name: 'NEW WAR' }).click();
  await page.getByRole('button', { name: 'NEXT' }).click();
  await page.getByRole('group', { name: /^Who plays Coalition B/ }).getByRole('button', { name: 'HUMAN' }).click();
  await page.getByRole('button', { name: 'SAME PERSON (COMMAND BOTH)' }).click();
  await page.getByRole('button', { name: 'NEXT' }).click();
  await page.getByRole('button', { name: 'NEXT' }).click();
  const start = page.getByRole('button', { name: 'START WAR' });
  const without = page.getByLabel('Start without the region pack');
  await expect(start.or(without).first()).toBeVisible();
  if (await without.isVisible()) await without.check();
  await start.click();
  await expect(page.locator('.top-bar')).toBeVisible();
}

const sidePressed = (page: Page, id: 'ALD' | 'VSM') => page.locator('.side-pos', { hasText: id });

test('switching sides keeps each side’s selection and panels', async ({ page }) => {
  await startCommandBoth(page);
  await expect(page.getByTestId('handover')).toHaveCount(0);
  await expect(sidePressed(page, 'ALD')).toHaveAttribute('aria-pressed', 'true');

  // Side A: select its first group and open the reports.
  const firstA = page.locator('.force-row').first();
  const nameA = (await firstA.locator('.force-name').innerText()).split('\n')[0] ?? '';
  await firstA.click();
  const card = page.getByRole('complementary', { name: 'Unit card' });
  await expect(card).toBeVisible();
  await page.getByRole('button', { name: 'REPORTS', exact: true }).click();
  await expect(page.getByRole('button', { name: 'REPORTS', exact: true })).toHaveAttribute('aria-expanded', 'true');

  // Tab: side B at once, its own (empty) selection, reports closed, its own forces.
  await page.locator('body').press('Tab');
  await expect(sidePressed(page, 'VSM')).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByTestId('handover')).toHaveCount(0);
  await expect(card).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'REPORTS', exact: true })).toHaveAttribute('aria-expanded', 'false');
  await expect(page.locator('.force-row').first()).not.toContainText(nameA);

  // Back to A: its selection and reports come back.
  await page.locator('body').press('Tab');
  await expect(sidePressed(page, 'ALD')).toHaveAttribute('aria-pressed', 'true');
  await expect(card).toBeVisible();
  await expect(card).toContainText(nameA.replace(/^\S+ /, '').slice(0, 6));
  await expect(page.getByRole('button', { name: 'REPORTS', exact: true })).toHaveAttribute('aria-expanded', 'true');
});

test('the umpire view is turned on by the player and is read-only', async ({ page }) => {
  await startCommandBoth(page);
  await expect(page.getByRole('button', { name: 'UMPIRE', exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: 'UMPIRE OFF' }).click();
  await expect(page.getByText('Umpire view: both sides visible')).toBeVisible();
  await expect(page.getByRole('button', { name: 'UMPIRE', exact: true })).toHaveAttribute('aria-pressed', 'true');
  // Read-only: no forces panel, no unit card, no side ROE.
  await expect(page.locator('.forces, .unit-card')).toHaveCount(0);
  await expect(page.getByRole('group', { name: 'Side rules of engagement' }).getByRole('button', { name: 'A' })).toBeDisabled();
  // The view really is the umpire view: both sides' units, no contacts.
  const view = () =>
    page.evaluate(() => {
      const v = (window as unknown as { __wbBridge: { frame: { view: { umpire: boolean; contacts: { count: number }; units: { side: Uint8Array } } } | null } }).__wbBridge.frame?.view;
      return v ? { umpire: v.umpire, contacts: v.contacts.count, sides: [...new Set(Array.from(v.units.side))].sort() } : null;
    });
  await expect.poll(view).toEqual({ umpire: true, contacts: 0, sides: [0, 1] });
  // Tab goes back round to side A; turning it off removes the position.
  await page.locator('body').press('Tab');
  await expect(sidePressed(page, 'ALD')).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByText('Umpire view: both sides visible')).toHaveCount(0);
  await page.getByRole('button', { name: 'UMPIRE ON' }).click();
  await expect(page.getByRole('button', { name: 'UMPIRE', exact: true })).toHaveCount(0);
});
