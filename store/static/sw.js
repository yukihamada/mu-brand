// MU service worker — minimal offline support.
// Strategy: network-first with cache fallback. On install, pre-cache the
// canon pages (/constitution, /dao, /dao/whitepaper, /scan) so the
// 100-year Constitution stays readable even without network.
// Constitution §22 + §23 alignment: brand documents must outlive the device.

const VERSION = 'mu-v3-2026-05-13';
const CACHE = `mu-cache-${VERSION}`;

const PRECACHE = [
  '/',
  '/constitution',
  '/constitution.md',
  '/dao',
  '/dao/whitepaper',
  '/whitepaper_dao.md',
  '/scan',
  '/transparency',
  '/vision',
  '/favicon.svg',
  '/icon-192.png',
  '/icon-512.png',
  '/manifest.json',
];

self.addEventListener('install', (event) => {
  event.waitUntil((async () => {
    const cache = await caches.open(CACHE);
    // Pre-cache best-effort; if any URL fails we still install.
    await Promise.allSettled(PRECACHE.map((u) => cache.add(u)));
    self.skipWaiting();
  })());
});

self.addEventListener('activate', (event) => {
  event.waitUntil((async () => {
    // Purge old cache versions.
    const keys = await caches.keys();
    await Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k)));
    self.clients.claim();
  })());
});

self.addEventListener('fetch', (event) => {
  const req = event.request;
  if (req.method !== 'GET') return;
  const url = new URL(req.url);
  // Same-origin only — let cross-origin requests pass through.
  if (url.origin !== self.location.origin) return;
  // Never cache API or admin or claim flows (state-bearing).
  if (url.pathname.startsWith('/api/')) return;
  if (url.pathname.startsWith('/admin/')) return;
  if (url.pathname.startsWith('/claim/')) return;
  if (url.pathname.startsWith('/checkout')) return;
  // Network-first; cache fallback.
  event.respondWith((async () => {
    try {
      const fresh = await fetch(req);
      if (fresh && fresh.ok) {
        const cache = await caches.open(CACHE);
        cache.put(req, fresh.clone()).catch(() => {});
      }
      return fresh;
    } catch (e) {
      const cached = await caches.match(req);
      if (cached) return cached;
      // Last-resort offline page for navigations.
      if (req.mode === 'navigate') {
        const fallback = await caches.match('/constitution');
        if (fallback) return fallback;
      }
      throw e;
    }
  })());
});
