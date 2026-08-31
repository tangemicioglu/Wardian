const CACHE_NAME = "wardian-remote-app-shell-__WARDIAN_BUILD_VERSION__";
const APP_SHELL = ["/remote", "/manifest.webmanifest", "/icon.png", "/icon-maskable.png"];
const ASSET_PREFIX = "/assets/";
const NAVIGATION_TIMEOUT_MS = 5000;

function fetchWithTimeout(request) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), NAVIGATION_TIMEOUT_MS);
  return fetch(request, { signal: controller.signal }).finally(() => clearTimeout(timeout));
}

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches.open(CACHE_NAME).then((cache) =>
      Promise.all(APP_SHELL.map((url) => cache.add(url).catch(() => undefined))),
    ),
  );
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(keys.filter((key) => key !== CACHE_NAME).map((key) => caches.delete(key))),
      ),
  );
  self.clients.claim();
});

self.addEventListener("fetch", (event) => {
  const url = new URL(event.request.url);
  if (url.origin !== self.location.origin) return;
  if (url.pathname.startsWith("/remote/api/")) return;
  if (event.request.method !== "GET") return;

  if (event.request.mode === "navigate") {
    event.respondWith(fetchWithTimeout(event.request).catch(() => caches.match("/remote")));
    return;
  }

  if (url.pathname.startsWith(ASSET_PREFIX)) {
    event.respondWith(
      caches.open(CACHE_NAME).then(async (cache) => {
        try {
          const response = await fetch(event.request);
          if (response.ok) {
            await cache.put(event.request, response.clone());
          }
          return response;
        } catch {
          return cache.match(event.request);
        }
      }),
    );
    return;
  }

  event.respondWith(fetch(event.request).catch(() => caches.match(event.request)));
});
