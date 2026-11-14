// 2D frame rate and engine speed on the benchmark scene (P12 acceptance, doc 08 §4, doc 06 §11):
// 500 visible units at ×5. The Android target is a mid-range phone; this test stands in for it
// with Chrome's CPU throttling (`WB_E2E_CPU_SLOWDOWN`, default 4×) and checks both that the map
// draws at 30 fps or more and that the engine keeps up with ×5 (achieved speed). The real
// device check is a manual step with the APK (docs/phase-reports/P12.md).
//   WB_E2E_PERF2D=1 pnpm --filter game e2e --project perf2d
//   WB_E2E_PERF2D=1 WB_E2E_CPU_SLOWDOWN=6 pnpm --filter game e2e --project perf2d
import { expect, test } from '@playwright/test';
import { installWorld } from './helpers3d';

const CENTRE: [number, number] = [17.5, 55.25];
const ZOOM = 6.3;
const MEASURE_MS = 10_000;
const SLOWDOWN = Number(process.env.WB_E2E_CPU_SLOWDOWN ?? '4');

type Win = typeof window & {
  __wbMap: { jumpTo: (o: unknown) => void };
  __wbMap2d: { selectables: () => string[] };
  __wbBridge: { report: { tick: number; achievedSpeed: number } | null };
};

test('the 2D map draws 500 visible units at ×5 at 30 fps or more under CPU throttling', async ({ page }) => {
  await page.goto('/');
  await installWorld(page);
  await page.getByRole('button', { name: 'QUICK START' }).click();
  await expect(page.locator('.top-bar')).toBeVisible();
  const hide = page.getByRole('button', { name: 'HIDE' });
  if (await hide.isVisible()) await hide.click();
  await page.evaluate(({ c, z }) => (window as Win).__wbMap.jumpTo({ center: c, zoom: z }), { c: CENTRE, z: ZOOM });
  await expect
    .poll(() => page.evaluate(() => (window as Win).__wbMap2d?.selectables().length ?? 0), { timeout: 60_000 })
    .toBeGreaterThanOrEqual(500);
  await page.getByRole('button', { name: '×5', exact: true }).click();
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Emulation.setCPUThrottlingRate', { rate: SLOWDOWN });
  // Let the speed change and the throttled frames settle, then count.
  await page.waitForTimeout(5_000);
  const result = await page.evaluate(async (ms) => {
    const w = window as Win;
    const t0 = w.__wbBridge.report?.tick ?? 0;
    let frames = 0;
    let worst = 0;
    const longTasks: number[] = [];
    const obs = new PerformanceObserver((list) => longTasks.push(...list.getEntries().map((e) => e.duration)));
    obs.observe({ entryTypes: ['longtask'] });
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
    obs.disconnect();
    const real = (performance.now() - start) / 1000;
    const ticks = (w.__wbBridge.report?.tick ?? 0) - t0;
    return {
      fps: frames / real,
      worstMs: worst,
      longTasks: longTasks.length,
      longestTaskMs: Math.max(0, ...longTasks),
      // 10 ticks per sim second.
      simPerReal: ticks / 10 / real,
      achieved: w.__wbBridge.report?.achievedSpeed ?? 0,
      visible: w.__wbMap2d.selectables().length,
    };
  }, MEASURE_MS);
  await cdp.send('Emulation.setCPUThrottlingRate', { rate: 1 });
  const line = `2D benchmark at ×5, CPU ${SLOWDOWN}× slower: ${result.fps.toFixed(1)} fps, worst frame ${result.worstMs.toFixed(0)} ms, ${result.longTasks} long tasks (longest ${result.longestTaskMs.toFixed(0)} ms), ${result.simPerReal.toFixed(2)} sim s per real s (achieved ${result.achieved.toFixed(2)}), ${result.visible} selectable`;
  test.info().annotations.push({ type: 'perf2d', description: line });
  console.log(line);
  expect(result.visible).toBeGreaterThanOrEqual(500);
  expect(result.fps).toBeGreaterThanOrEqual(30);
  expect(result.simPerReal).toBeGreaterThanOrEqual(4.75);
});
