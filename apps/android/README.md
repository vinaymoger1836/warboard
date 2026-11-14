# WarBoard for Android

A Capacitor 8 wrapper of the production build of `apps/game` (ADR-017, ADR-050). The app is the web
build in a WebView at `https://localhost`: the same engine (WebAssembly), packs (OPFS), saves and
service worker as in the browser. Sideloaded, no store.

## What you need

- Node and pnpm as for the rest of the repo, and `pnpm install && pnpm gen` done once.
- **JDK 21** and the **Android SDK** (platform 36, build tools, platform tools). Android Studio installs
  both. Set `ANDROID_HOME` (or write `sdk.dir=...` into `android/local.properties`, which is gitignored).
- A phone with USB debugging, or an emulator.

`pnpm --filter android doctor` checks the toolchain.

## Build and install

```bash
bash scripts/build-apk.sh            # from the repo root: web build, sync, debug APK
adb install -r apps/android/android/app/build/outputs/apk/debug/app-debug.apk
```

The script does, in order:

1. `pnpm --filter game build`: the production web build (`apps/game/dist`). For the hosted pack bucket set
   `VITE_PACKS_URL=<url>`; for the neutral test war on the fixtures use the hosted-build variables in
   `CLAUDE.md`.
2. `pnpm --filter android sync`: copies `dist` into `android/app/src/main/assets/public` (gitignored).
3. `./gradlew assembleDebug` in `apps/android/android`.

## Map packs on the phone

Open **MAP PACKS**. The world pack is 70 MB and the Europe pack 700 MB, so use Wi-Fi.

- From the hosted bucket: its CORS rule must allow the origin `https://localhost` (ADR-040 lists only the
  Pages origin and `http://localhost:5173`); add it once.
- From your PC: run `pnpm --filter game dev --host` on the PC (it serves `build/packs/` at `/packs/`), then in
  the **Pack host** field enter `http://<pc address>:5173/packs/` and press USE. The pack list and every
  file are checked against their BLAKE3 hashes. RESET goes back to the host the app was built with.

## Touch

- The unit card is a bottom sheet with the doc 10 §7 layout: it opens on its header, status rows and order
  buttons; the handle at the top opens the rest.
- Long-press (half a second, one finger, held still) on the 2D map opens the context menu, the right-click.
- Buttons are at least 44 px. Rotate freely; the system bars come back with a swipe from the edge.

## Checking the frame rate on a device (P12 acceptance)

1. Build the 500-unit scene: `WB_SCENARIO=scenarios/bench/view-500.yaml WB_ANDROID_DEBUG=1 bash scripts/build-apk.sh`,
   install, install the world pack, **QUICK START**, speed **×5**, frame the area (zoom about 6.3 at 17.5° E 55.25° N).
2. On the PC open `chrome://inspect`, inspect WarBoard, and paste into the console:

   ```js
   (async () => {
     const s0 = window.__wbMap2d.renderStats(); let f = 0; const t0 = performance.now();
     await new Promise((d) => { const step = () => { f++; performance.now() - t0 < 10000 ? requestAnimationFrame(step) : d(); }; requestAnimationFrame(step); });
     const s1 = window.__wbMap2d.renderStats(), s = (performance.now() - t0) / 1000;
     console.log({ animationFps: f / s, redrawsPerS: (s1.frames - s0.frames) / s, msPerRedraw: (s1.ms - s0.ms) / (s1.frames - s0.frames), achievedSpeed: window.__wbBridge.report.achievedSpeed });
   })();
   ```

   Pass: `animationFps` and `redrawsPerS` both 30 or more with `achievedSpeed` about 5.

Without a device, `WB_E2E_PERF2D=1 pnpm --filter game e2e --project perf2d` runs the same scene in Chrome with
the CPU throttled (`WB_E2E_CPU_SLOWDOWN`, default 4) as a stand-in.

## Files

| Path | What |
|---|---|
| `capacitor.config.ts` | App id `dev.warboard.game`, `webDir`, `https` scheme, mixed content for the pack host, debug inspector switch |
| `android/` | The Gradle project Capacitor generated. Hand-edited: `AndroidManifest.xml` (no backup, user rotation), `MainActivity.java` (system bars, screen on) |
| `android/app/src/main/assets/public/` | The copied web build (generated, gitignored) |
