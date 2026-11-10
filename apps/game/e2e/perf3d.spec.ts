// 3D frame rate (P10 acceptance, doc 08 §4): ≥ 30 fps with 500 visible units on the benchmark
// scene. Runs in the `perf3d` project, whose dev server boots scenarios/bench/view-500.yaml as
// the quick start, headed on the installed Chrome so WebGL runs on the GPU:
//   WB_E2E_PERF=1 pnpm --filter game e2e --project perf3d
import { expect, test } from '@playwright/test';
import { installWorld, open3d } from './helpers3d';

/** The benchmark area's centre and a zoom that frames all of it. */
const CENTRE: [number, number] = [17.5, 55.25];
const ZOOM = 6.3;
const MEASURE_MS = 10_000;

test('the 3D view draws 500 visible units at 30 fps or more', async ({ page }) => {
  await page.goto('/');
  await installWorld(page);
  await page.getByRole('button', { name: 'QUICK START' }).click();
  await expect(page.locator('.top-bar')).toBeVisible();
  const hide = page.getByRole('button', { name: 'HIDE' });
  if (await hide.isVisible()) await hide.click();
  // Frame the whole benchmark area in 2D, then open the globe at the same view (clock running).
  await page.evaluate(({ c, z }) => (window as unknown as { __wbMap: { jumpTo: (o: unknown) => void } }).__wbMap.jumpTo({ center: c, zoom: z }), {
    c: CENTRE,
    z: ZOOM,
  });
  await open3d(page);
  await expect
    .poll(() => page.evaluate(() => (window as unknown as { __wbGlobe: { visible: () => string[] } }).__wbGlobe.visible().length), { timeout: 60_000 })
    .toBeGreaterThanOrEqual(500);
  // Let terrain and the surface settle, then count frames.
  await page.waitForTimeout(3_000);
  const result = await page.evaluate(async (ms) => {
    const g = (window as unknown as { __wbGlobe: { visible: () => string[] } }).__wbGlobe;
    let frames = 0;
    let worst = 0;
    let last = performance.now();
    const start = last;
    await new Promise<void>((done) => {
      const step = () => {
        const now = performance.now();
        worst = Math.max(worst, now - last);
        last = now;
        frames++;
        if (now - start < ms) requestAnimationFrame(step);
        else done();
      };
      requestAnimationFrame(step);
    });
    return { fps: (frames * 1000) / (performance.now() - start), worstMs: worst, visible: g.visible().length };
  }, MEASURE_MS);
  test.info().annotations.push({ type: 'fps', description: `${result.fps.toFixed(1)} fps, worst frame ${result.worstMs.toFixed(1)} ms, ${result.visible} visible` });
  console.log(`3D benchmark: ${result.fps.toFixed(1)} fps, worst frame ${result.worstMs.toFixed(1)} ms, ${result.visible} visible units`);
  expect(result.visible).toBeGreaterThanOrEqual(500);
  expect(result.fps).toBeGreaterThanOrEqual(30);
});
