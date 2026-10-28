// App-shell service worker (doc 06 §9). The build fills in the precache list (every built
// and public file) and a version hash of that list; see vite.config.ts.
// Packs are never cached here: the pack manager stores them in OPFS.
const PRECACHE = __PRECACHE__;
const CACHE = `warboard-shell-${__VERSION__}`;

self.addEventListener('install', (event) => {
  event.waitUntil(
    caches
      .open(CACHE)
      .then((cache) => cache.addAll(PRECACHE.map((p) => new URL(p, self.registration.scope).href)))
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((k) => k.startsWith('warboard-shell-') && k !== CACHE).map((k) => caches.delete(k))))
      .then(() => self.clients.claim()),
  );
});

self.addEventListener('fetch', (event) => {
  const req = event.request;
  const url = new URL(req.url);
  if (req.method !== 'GET' || url.origin !== self.location.origin) return;
  if (url.pathname.includes('/packs/')) return;
  if (req.mode === 'navigate') {
    // Network first for the page itself, so a new build is picked up when online.
    event.respondWith(
      fetch(req).catch(() =>
        caches
          .match(new URL('./', self.registration.scope).href, { cacheName: CACHE, ignoreVary: true })
          .then((r) => r ?? Response.error()),
      ),
    );
    return;
  }
  event.respondWith(
    // ignoreVary: hosts send `Vary: Origin`, and module scripts are requested with an Origin
    // header the precache requests did not have; every entry here is our own same-origin file.
    caches.match(req, { cacheName: CACHE, ignoreSearch: true, ignoreVary: true }).then((hit) => hit ?? fetch(req)),
  );
});
