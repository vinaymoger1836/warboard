// The 3D tactical view (P10, doc 06 §5, doc 09 P10, ADR-047): switching 2D ↔ 3D keeps the
// selection, the sim time and the centre; the same view draws the same set of selectable
// objects in both, and each one is reachable by a click; no Cesium widget and no request to
// Cesium ion; the inspect view opens on an own group and never on an estimated contact.
import { expect, type Page, test } from '@playwright/test';
import { installWorld, open2d, open3d, quickStartPaused, tick, type ViewHooks } from './helpers3d';

const CENTRE: [number, number] = [17.5, 55.0];
/** Between the base (z5) and flight-member (z11) thresholds, and off every label threshold. */
const ZOOM = 6.5;

const selectables = (page: Page, which: '__wbMap2d' | '__wbGlobe') =>
  page.evaluate((w) => (window as unknown as Record<string, ViewHooks>)[w]?.selectables() ?? [], which);

/** Ids whose position is inside the middle of the map (edges differ: Mercator vs perspective). */
async function central(page: Page, which: '__wbMap2d' | '__wbGlobe', ids: string[]): Promise<string[]> {
  const box = await page.locator('.map-root').boundingBox();
  if (!box) return [];
  return page.evaluate(
    ({ w, ids, width, height }) => {
      const h = (window as unknown as Record<string, ViewHooks>)[w];
      return ids.filter((id) => {
        const p = h?.windowPosition(id);
        return !!p && p.x > width * 0.25 && p.x < width * 0.75 && p.y > height * 0.25 && p.y < height * 0.75;
      });
    },
    { w: which, ids, width: box.width, height: box.height },
  );
}

const unpickable = (page: Page, which: '__wbMap2d' | '__wbGlobe', ids: string[]) =>
  page.evaluate(({ w, ids }) => ids.filter((id) => !(window as unknown as Record<string, ViewHooks>)[w]?.pickable(id)), { w: which, ids });

/** An MF-4 flight in the air (the forces list may also hold one parked at its base). */
const airborneFlight = (page: Page) => page.locator('.force-row', { hasText: 'MF-4 Sellis' }).filter({ hasText: 'TRANSIT' }).first();

const cardTitle = (page: Page) => page.locator('.unit-card h2').first().textContent();

test('2D ↔ 3D keeps selection, time and centre, and both draw the same selectable objects', async ({ page }) => {
  const outside: string[] = [];
  page.on('request', (r) => {
    const url = new URL(r.url());
    if (url.hostname !== 'localhost') outside.push(r.url());
  });
  await page.goto('/');
  await installWorld(page);
  await quickStartPaused(page);

  // A fixed 2D view and a selection.
  await page.evaluate(({ c, z }) => (window as unknown as { __wbMap: { jumpTo: (o: unknown) => void } }).__wbMap.jumpTo({ center: c, zoom: z }), { c: CENTRE, z: ZOOM });
  await airborneFlight(page).click();
  await expect(page.locator('.unit-card')).toBeVisible();
  const title = await cardTitle(page);
  // Selecting from the forces list centres the map on the group (a 600 ms ease); then the
  // view is fixed, and that is the centre both views must keep.
  await page.waitForTimeout(1_000);
  const centre = await page.evaluate(() => {
    const m = (window as unknown as { __wbMap: { getCenter: () => { lng: number; lat: number } } }).__wbMap;
    return [m.getCenter().lng, m.getCenter().lat] as [number, number];
  });
  await page.waitForFunction(() => ((window as unknown as { __wbMap2d?: ViewHooks }).__wbMap2d?.selectables().length ?? 0) > 0);
  const ids2d = await selectables(page, '__wbMap2d');
  const mid2d = await central(page, '__wbMap2d', ids2d);
  expect(await unpickable(page, '__wbMap2d', mid2d)).toEqual([]);
  const t0 = await tick(page);

  // 3D: same selection, same time, same centre and scale.
  await open3d(page);
  expect(await cardTitle(page)).toBe(title);
  expect(await tick(page)).toBe(t0);
  const cam = await page.evaluate(() => (window as unknown as { __wbGlobe: { camera: () => { center: [number, number]; zoom: number } | null } }).__wbGlobe.camera());
  expect(cam).not.toBeNull();
  expect(Math.abs((cam?.center[0] ?? 0) - centre[0])).toBeLessThan(0.02);
  expect(Math.abs((cam?.center[1] ?? 0) - centre[1])).toBeLessThan(0.02);
  expect(Math.abs((cam?.zoom ?? 0) - ZOOM)).toBeLessThan(0.05);

  // Parity (doc 09 P10): the same set of selectable objects, and every one in the middle of
  // the view is reached by a click at its screen position.
  const ids3d = await selectables(page, '__wbGlobe');
  expect(ids3d).toEqual(ids2d);
  const mid3d = await central(page, '__wbGlobe', ids3d);
  expect(mid3d.length).toBeGreaterThan(0);
  expect(await unpickable(page, '__wbGlobe', mid3d)).toEqual([]);
  expect([...mid3d].sort()).toEqual([...mid2d].sort());

  // No Cesium widget (doc 10 §12) and nothing fetched from outside (no Cesium ion, no imagery).
  await expect(page.locator('[class*="cesium-viewer"], .cesium-widget-credits, .cesium-credit-logoContainer, .cesium-widget-errorPanel')).toHaveCount(0);
  expect(outside).toEqual([]);

  // Back to 2D: still the same selection, time and centre.
  await open2d(page);
  expect(await cardTitle(page)).toBe(title);
  expect(await tick(page)).toBe(t0);
  const back = await page.evaluate(() => {
    const m = (window as unknown as { __wbMap: { getCenter: () => { lng: number; lat: number }; getZoom: () => number } }).__wbMap;
    return { lng: m.getCenter().lng, lat: m.getCenter().lat, zoom: m.getZoom() };
  });
  expect(Math.abs(back.lng - centre[0])).toBeLessThan(0.02);
  expect(Math.abs(back.lat - centre[1])).toBeLessThan(0.02);
  expect(Math.abs(back.zoom - ZOOM)).toBeLessThan(0.05);
});

test('inspect opens on an own group, follows it, and is never offered on an estimated contact', async ({ page }) => {
  await page.goto('/');
  await installWorld(page);
  await quickStartPaused(page);

  // From the unit card in 2D: INSPECT 3D switches to the 3D view in inspect mode.
  await airborneFlight(page).click();
  await page.locator('.unit-card').getByRole('button', { name: 'INSPECT 3D' }).click();
  await expect(page.getByRole('button', { name: 'OPEN 2D MAP' })).toBeVisible();
  const hud = page.getByRole('status', { name: 'Inspect' });
  await expect(hud).toBeVisible();
  await expect(hud).toContainText('MF-4 Sellis');
  await expect(page.getByRole('button', { name: 'INSPECT', exact: true })).toHaveAttribute('aria-pressed', 'true');
  // The inspected group is drawn as its class model.
  await expect
    .poll(() => page.evaluate(() => (window as unknown as { __wbRenderer?: { models: Map<string, { model: { ready: boolean } | null }> } }).__wbRenderer?.models.size ?? 0))
    .toBeGreaterThan(0);
  await page.keyboard.press('Escape');
  await expect(hud).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'FREE' })).toHaveAttribute('aria-pressed', 'true');

  // Let the picture build up contacts, then check each one's card: INSPECT 3D only when the
  // contact is identified (not estimated, not stale), never on an estimate.
  await page.getByRole('button', { name: 'Speed ×10' }).click();
  await expect
    .poll(
      () => page.evaluate(() => (window as unknown as { __wbRenderer?: { last: { scene: { contacts: unknown[] } } | null } }).__wbRenderer?.last?.scene.contacts.length ?? 0),
      { timeout: 90_000 },
    )
    .toBeGreaterThan(0);
  await page.getByRole('button', { name: 'Pause', exact: true }).click();
  const contacts = await page.evaluate(() =>
    (window as unknown as { __wbRenderer: { last: { scene: { contacts: { id: number; estimated: boolean; identified: boolean; isWeapon: boolean }[] } } } }).__wbRenderer.last.scene.contacts
      .filter((c) => !c.isWeapon)
      .map((c) => ({ id: c.id, estimated: c.estimated, identified: c.identified })),
  );
  expect(contacts.length).toBeGreaterThan(0);
  let estimatedSeen = 0;
  let identifiedSeen = 0;
  for (const c of contacts) {
    // Centre the globe on the contact as our picture places it, then click it there.
    const at = await page.evaluate((id) => (window as unknown as { __wbRenderer: { contact: (id: number) => { position: [number, number] } | null } }).__wbRenderer.contact(id)?.position ?? null, c.id);
    if (!at) continue;
    await page.evaluate(([lon, lat]) => (window as unknown as { __wbGlobe: { lookAt: (a: number, b: number, z: number) => void } }).__wbGlobe.lookAt(lon, lat, 10), at);
    await page.waitForTimeout(500);
    const p = await page.evaluate((id) => (window as unknown as { __wbGlobe: ViewHooks }).__wbGlobe.windowPosition(`c:${id}`), c.id);
    const box = await page.locator('.map-root').boundingBox();
    expect(p).not.toBeNull();
    if (!p || !box) continue;
    await page.mouse.click(box.x + p.x, box.y + p.y);
    const card = page.locator('.unit-card[aria-label="Contact card"]');
    await expect(card).toBeVisible();
    const button = card.getByRole('button', { name: 'INSPECT 3D' });
    if (c.estimated) {
      estimatedSeen++;
      await expect(button).toHaveCount(0);
    } else if (c.identified) {
      identifiedSeen++;
      await expect(button).toBeVisible();
      await button.click();
      await expect(page.getByRole('status', { name: 'Inspect' })).toBeVisible();
      await page.keyboard.press('Escape');
    }
  }
  expect(estimatedSeen + identifiedSeen).toBeGreaterThan(0);
  console.log(`contacts: ${contacts.length}, estimated checked: ${estimatedSeen}, identified checked: ${identifiedSeen}`);
  test.info().annotations.push({ type: 'contacts', description: `${contacts.length} contacts, ${estimatedSeen} estimated, ${identifiedSeen} identified` });
});
