import type { CapacitorConfig } from '@capacitor/cli';

// The Android app (ADR-017, ADR-050): the production build of apps/game in a WebView.
// `webDir` is that build (`pnpm --filter game build`); `pnpm --filter android sync` copies it
// into android/app/src/main/assets/public.
const config: CapacitorConfig = {
  appId: 'dev.warboard.game',
  appName: 'WarBoard',
  webDir: '../game/dist',
  // Served from https://localhost, a secure context: OPFS, the service worker and
  // WebAssembly work as in the browser.
  server: { androidScheme: 'https' },
  android: {
    // The pack manager can fetch from a PC on the same network over http (doc 06 §11); the pack
    // list and every file are still checked against their BLAKE3 hashes. Nothing else loads
    // from the web.
    allowMixedContent: true,
    // Chrome's remote inspector (chrome://inspect) for frame-rate checks on a device:
    // build with WB_ANDROID_DEBUG=1. Off in every other build.
    webContentsDebuggingEnabled: process.env.WB_ANDROID_DEBUG === '1',
  },
};

export default config;
