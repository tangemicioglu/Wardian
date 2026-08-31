import { createServer, type Server } from "node:http";
import { once } from "node:events";
import { expect, test } from "@playwright/test";

const OLD_CACHE = "wardian-remote-app-shell-v1";
const NEW_CACHE = "wardian-remote-app-shell-build-2";

function workerSource(version: 1 | 2): string {
  if (version === 1) {
    return `
      const CACHE_NAME = ${JSON.stringify(OLD_CACHE)};
      self.addEventListener("install", (event) => {
        event.waitUntil(
          caches.open(CACHE_NAME)
            .then((cache) => cache.put("/remote", new Response("legacy shell")))
            .then(() => self.skipWaiting()),
        );
      });
      self.addEventListener("activate", (event) => {
        event.waitUntil(self.clients.claim());
      });
      self.addEventListener("fetch", (event) => {
        if (event.request.mode !== "navigate") return;
        const html =
          "<!doctype html><html><body>" +
          "<p id=\\"legacy-navigation\\">legacy navigation is streaming</p>" +
          "<script>" +
          "window.__loadFired = false;" +
          "window.addEventListener('load', () => { window.__loadFired = true; });" +
          "navigator.serviceWorker.register('/remote-sw.js', { scope: '/remote' }).then((registration) => registration.update()).catch((error) => { window.__registrationError = String(error); });" +
          "</script>";
        event.respondWith(new Response(new ReadableStream({
          start(controller) {
            controller.enqueue(new TextEncoder().encode(html));
          },
        }), { headers: { "content-type": "text/html" } }));
      });
    `;
  }

  return `
    const CACHE_NAME = ${JSON.stringify(NEW_CACHE)};
    const SHELL = "<!doctype html><html><body><p id=\\"replacement-shell\\">replacement shell</p></body></html>";
    self.addEventListener("install", (event) => {
      event.waitUntil(
        caches.open(CACHE_NAME)
          .then((cache) => cache.put("/remote", new Response(SHELL, { headers: { "content-type": "text/html" } })))
          .then(() => self.skipWaiting()),
      );
    });
    self.addEventListener("activate", (event) => {
      event.waitUntil(
        caches.keys()
          .then((keys) => Promise.all(keys.filter((key) => key !== CACHE_NAME).map((key) => caches.delete(key))))
          .then(() => self.clients.claim()),
      );
    });
    self.addEventListener("fetch", (event) => {
      if (event.request.mode !== "navigate") return;
      event.respondWith(caches.match("/remote"));
    });
  `;
}

function pageSource(): string {
  return `
    <!doctype html>
    <html><body>
      <p id="remote-shell">remote shell</p>
      <script>
        window.__loadFired = false;
        window.addEventListener("load", () => { window.__loadFired = true; });
        navigator.serviceWorker.register("/remote-sw.js", { scope: "/remote" }).then((registration) => registration.update()).catch((error) => { window.__registrationError = String(error); });
      </script>
    </body></html>
  `;
}

async function startRecoveryServer(): Promise<{
  server: Server;
  url: string;
  setWorkerVersion: (version: 1 | 2) => void;
  workerRequests: number[];
}> {
  let version: 1 | 2 = 1;
  const workerRequests: number[] = [];
  const server = createServer((request, response) => {
    if (request.url === "/remote-sw.js") {
      workerRequests.push(version);
      response.writeHead(200, {
        "cache-control": "no-store",
        "content-type": "application/javascript",
      });
      response.end(workerSource(version));
      return;
    }

    if (request.url === "/remote") {
      response.writeHead(200, { "content-type": "text/html" });
      response.end(pageSource());
      return;
    }

    response.writeHead(404);
    response.end();
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const address = server.address();
  if (!address || typeof address === "string") {
    throw new Error("recovery test server did not expose a TCP address");
  }

  return {
    server,
    url: `http://127.0.0.1:${address.port}/remote`,
    setWorkerVersion: (nextVersion) => { version = nextVersion; },
    workerRequests,
  };
}

test("recovers an already-controlled v1 client while its navigation never finishes", async ({ page }) => {
  const recovery = await startRecoveryServer();
  try {
    await page.goto(recovery.url);
    await page.waitForFunction(() => navigator.serviceWorker.controller !== null);
    await expect.poll(async () => page.evaluate(async () => caches.keys())).toContain(OLD_CACHE);

    recovery.setWorkerVersion(2);
    await page.goto(recovery.url, { waitUntil: "commit", timeout: 5_000 });
    await expect(page.locator("#legacy-navigation")).toBeVisible();
    await expect
      .poll(() => page.evaluate(() => (window as Window & { __loadFired?: boolean }).__loadFired))
      .toBe(false);
    await expect.poll(() => recovery.workerRequests.filter((version) => version === 2).length).toBeGreaterThan(0);

    await page.goto("about:blank");
    const recoveredPage = await page.context().newPage();
    try {
      await recoveredPage.goto(recovery.url, { waitUntil: "domcontentloaded" });
      await expect(recoveredPage.locator("#replacement-shell")).toBeVisible();
      await expect.poll(async () => recoveredPage.evaluate(async () => caches.keys())).toEqual([NEW_CACHE]);
    } finally {
      await recoveredPage.close();
    }
  } finally {
    await new Promise<void>((resolve, reject) => recovery.server.close((error) => error ? reject(error) : resolve()));
  }
});
