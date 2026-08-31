export function registerServiceWorker() {
  if (!("serviceWorker" in navigator)) return;

  // Start discovery while the remote shell is still loading. A legacy worker
  // can leave navigation or a subresource pending indefinitely, which would
  // prevent the window load event and strand the client on its old cache.
  void navigator.serviceWorker
    .register("/remote-sw.js", { scope: "/remote" })
    .then((registration) => registration.update())
    .catch((error) => {
      console.warn("Remote service worker registration failed", error);
    });
}
