#!/usr/bin/env bash
# Builds the Android app (P12, ADR-050): production web build -> Capacitor sync -> debug APK.
# Needs JDK 21 and the Android SDK (apps/android/README.md). Run from anywhere.
set -euo pipefail
cd "$(dirname "$0")/.."

if ! command -v java >/dev/null 2>&1; then
  echo "java not found: install a JDK 21 (Android Studio ships one)." >&2
  exit 1
fi
if [ -z "${ANDROID_HOME:-}" ] && [ -z "${ANDROID_SDK_ROOT:-}" ] && [ ! -f apps/android/android/local.properties ]; then
  echo "No Android SDK: set ANDROID_HOME or write sdk.dir=... into apps/android/android/local.properties." >&2
  exit 1
fi
if [ ! -f build/content.wbpack ] && [ -z "${WB_CONTENT_PACK:-}" ]; then
  echo "No content pack: run 'pnpm gen' first (or set WB_CONTENT_PACK)." >&2
  exit 1
fi

pnpm --filter game build
pnpm --filter android sync
(cd apps/android/android && ./gradlew assembleDebug)

echo
echo "APK: apps/android/android/app/build/outputs/apk/debug/app-debug.apk"
echo "Install: adb install -r apps/android/android/app/build/outputs/apk/debug/app-debug.apk"
