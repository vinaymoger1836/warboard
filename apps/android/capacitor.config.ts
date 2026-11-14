import type { CapacitorConfig } from '@capacitor/cli';

// The Android app (ADR-017, ADR-050): the production build of apps/game in a WebView.
// `webDir` is that build (`pnpm --filter game build`); `pnpm --filter android sync` copies it
// into android/app/src/main/assets/public.
const config: CapacitorConfig = {
  appId: 'dev.warboard.game',
  appName: 'WarBoard',
  webDir: '../game/dist',
  // The app is served from https://localhost, a secure context: OPFS, the service worker and
  // WebAssembly threads' prerequisites work as in the browser.
  server: { androidScheme: 'https' },
  android: {
    // The pack manager can fetch from a PC on the same network over http (doc 06 §11); the pack
    // list and every file are still checked against their BLAKE3 hashes. Nothing else loads
    // from the web.
    allowMixedContent: true,
    // Chrome's remote inspector only in debug builds (`adb` + chrome://inspect).
    webContentsDebuggingEnabled: false,
  },
};

export default config;
