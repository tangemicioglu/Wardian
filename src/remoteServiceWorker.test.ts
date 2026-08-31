import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it, vi } from "vitest";

type ListenerMap = Map<string, Array<(event: unknown) => void>>;

function requestKey(request: Request | string): string {
  return typeof request === "string" ? new URL(request, "https://wardian.tailnet.ts.net").href : request.url;
}

function createCaches() {
  const stores = new Map<string, Map<string, Response>>();
  return {
    open: vi.fn(async (name: string) => {
      let store = stores.get(name);
      if (!store) {
        store = new Map();
        stores.set(name, store);
      }
      return {
        add: vi.fn(async (url: string) => {
          store.set(requestKey(url), new Response(`cached:${url}`));
        }),
        addAll: vi.fn(async (urls: string[]) => {
          for (const url of urls) {
            store.set(requestKey(url), new Response(`cached:${url}`));
          }
        }),
        match: vi.fn(async (request: Request | string) => store.get(requestKey(request))?.clone()),
        put: vi.fn(async (request: Request | string, response: Response) => {
          store.set(requestKey(request), response.clone());
        }),
      };
    }),
    keys: vi.fn(async () => [...stores.keys()]),
    delete: vi.fn(async (name: string) => stores.delete(name)),
    match: vi.fn(async (request: Request | string) => {
      for (const store of stores.values()) {
        const response = store.get(requestKey(request));
        if (response) return response.clone();
      }
      return undefined;
    }),
  };
}

function loadRemoteServiceWorker(
  fetchMock: typeof fetch,
  script = readFileSync(join(process.cwd(), "public", "remote-sw.js"), "utf8"),
  caches = createCaches(),
) {
  const listeners: ListenerMap = new Map();
  const selfScope = {
    location: { origin: "https://wardian.tailnet.ts.net" },
    clients: { claim: vi.fn() },
    skipWaiting: vi.fn(),
    addEventListener: vi.fn((type: string, listener: (event: unknown) => void) => {
      listeners.set(type, [...(listeners.get(type) ?? []), listener]);
    }),
  };
  new Function("self", "caches", "fetch", "URL", "Response", script)(
    selfScope,
    caches,
    fetchMock,
    URL,
    Response,
  );

  return { listeners, caches };
}

async function dispatchFetch(listener: (event: unknown) => void, request: Request): Promise<Response | undefined> {
  let response: Promise<Response | undefined> | undefined;
  listener({
    request,
    respondWith: vi.fn((value: Promise<Response | undefined>) => {
      response = value;
    }),
  });
  return response;
}

describe("remote service worker", () => {
  it("pre-caches the remote install icon assets", () => {
    const script = readFileSync(join(process.cwd(), "public", "remote-sw.js"), "utf8");

    expect(script).toContain('"/icon.png"');
    expect(script).toContain('"/icon-maskable.png"');
  });

  it("removes a poisoned v1 shell when a new build activates", async () => {
    const caches = createCaches();
    const oldCache = await caches.open("wardian-remote-app-shell-v1");
    await oldCache.put("/remote", new Response("old-shell"));
    const { listeners } = loadRemoteServiceWorker(
      vi.fn() as unknown as typeof fetch,
      readFileSync(join(process.cwd(), "public", "remote-sw.js"), "utf8")
        .replace(/__WARDIAN_BUILD_VERSION__/g, "build-2"),
      caches,
    );
    const activate = listeners.get("activate")?.[0];
    expect(activate).toBeDefined();
    let activation: Promise<unknown> | undefined;
    activate!({ waitUntil: (promise: Promise<unknown>) => { activation = promise; } } as unknown);
    await activation;

    expect(await caches.keys()).toEqual([]);
  });

  it("falls back to the cached shell when navigation fetch hangs", async () => {
    vi.useFakeTimers();
    try {
      const fetchMock = vi.fn((_request: Request, options?: RequestInit) =>
        new Promise<Response>((_resolve, reject) => {
          options?.signal?.addEventListener("abort", () => reject(new Error("aborted")));
        }),
      ) as unknown as typeof fetch;
      const { listeners, caches } = loadRemoteServiceWorker(fetchMock);
      const shell = await caches.open("wardian-remote-app-shell-__WARDIAN_BUILD_VERSION__");
      await shell.put("/remote", new Response("cached-shell"));
      const fetchListener = listeners.get("fetch")?.[0];
      const request = new Request("https://wardian.tailnet.ts.net/remote");
      Object.defineProperty(request, "mode", { value: "navigate" });
      const responsePromise = dispatchFetch(fetchListener!, request);

      await vi.advanceTimersByTimeAsync(5000);
      expect(await (await responsePromise)?.text()).toBe("cached-shell");
    } finally {
      vi.useRealTimers();
    }
  });

  it("runtime-caches successful remote asset responses for flaky network reuse", async () => {
    const assetUrl = "https://wardian.tailnet.ts.net/assets/index-abcd.js";
    const fetchMock = vi.fn(async () => new Response("asset-v1", { status: 200 })) as unknown as typeof fetch;
    const { listeners } = loadRemoteServiceWorker(fetchMock);
    const fetchListener = listeners.get("fetch")?.[0];
    expect(fetchListener).toBeDefined();

    const onlineResponse = await dispatchFetch(fetchListener!, new Request(assetUrl));
    expect(await onlineResponse?.text()).toBe("asset-v1");

    vi.mocked(fetchMock).mockRejectedValue(new Error("offline"));
    const cachedResponse = await dispatchFetch(fetchListener!, new Request(assetUrl));

    expect(await cachedResponse?.text()).toBe("asset-v1");
  });
});
