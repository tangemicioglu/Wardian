/**
 * Dev-server lifecycle shared by the documentation captures.
 *
 * Both the still capture and the site media capture need the same thing: an
 * owned Vite server on a private port, seeded with an isolated `WARDIAN_HOME`,
 * warm enough to serve a navigation before the first capture runs.
 */
import { spawn, spawnSync } from "node:child_process";
import http from "node:http";
import path from "node:path";
import process from "node:process";

/**
 * Budget for the first navigation against a cold dev server.
 *
 * Vite answers HTTP in about half a second but does not commit a navigation
 * until its dependency optimizer has finished, and that first optimize pass
 * runs well past Playwright's 30 s default. This is the window the optimizer
 * gets, not a guess at how long the app takes to render.
 */
export const COLD_START_NAVIGATION_TIMEOUT_MS = 180_000;

/**
 * Budget for every navigation after the warm-up.
 *
 * Vite can still discover a new dependency partway through the run — opening a
 * surface that pulls Monaco or the automation canvas for the first time — and
 * re-optimize, so these stay well above the default without being unbounded.
 */
export const NAVIGATION_TIMEOUT_MS = 60_000;

const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * Resolve where a capture run should point, honouring its own env overrides.
 *
 * @param {object} options
 * @param {string} options.root Repository root.
 * @param {string} options.urlEnv Env var naming an already-running app to reuse.
 * @param {string} options.portEnv Env var overriding the owned server's port.
 * @param {number} options.defaultPort Port to use when nothing is set.
 * @param {string} options.homeDirName Directory under `.tmp` for `WARDIAN_HOME`.
 */
export function resolveDevServerTarget({ root, urlEnv, portEnv, defaultPort, homeDirName }) {
  const explicitBaseUrl = process.env[urlEnv];
  const port = Number.parseInt(process.env[portEnv] ?? String(defaultPort), 10);
  return {
    explicitBaseUrl,
    port,
    baseUrl: explicitBaseUrl ?? `http://127.0.0.1:${port}`,
    home: path.join(root, ".tmp", homeDirName),
    portEnv,
    urlEnv,
  };
}

export async function isUrlReady(url) {
  return new Promise((resolve) => {
    const req = http.get(url, (res) => {
      res.resume();
      resolve(res.statusCode && res.statusCode < 500);
    });
    req.on("error", () => resolve(false));
    req.setTimeout(1_000, () => {
      req.destroy();
      resolve(false);
    });
  });
}

export async function waitForServer(baseUrl) {
  for (let i = 0; i < 90; i += 1) {
    if (await isUrlReady(baseUrl)) return;
    await wait(1_000);
  }
  throw new Error(`Timed out waiting for ${baseUrl}`);
}

export async function startOwnedServer(target, root) {
  if (await isUrlReady(target.baseUrl)) {
    throw new Error(
      `${target.baseUrl} is already serving content. Stop that process, set ${target.portEnv}, or set ${target.urlEnv} to opt into capturing an existing app.`,
    );
  }

  return spawn(`npm run vite -- --host 127.0.0.1 --port ${target.port} --strictPort`, {
    cwd: root,
    env: {
      ...process.env,
      WARDIAN_HOME: target.home,
    },
    shell: true,
    stdio: "inherit",
  });
}

export function stopOwnedServer(child) {
  if (!child?.pid) return;
  if (process.platform === "win32") {
    spawnSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore" });
    return;
  }
  child.kill();
}

/**
 * Drive one throwaway navigation so the dev server is warm before any capture.
 *
 * `waitForServer()` only proves Vite answers HTTP, which it does long before it
 * can serve a navigation: the first real page load is what triggers dependency
 * optimization, and the HTML is not committed until that finishes. Polling for
 * an HTTP answer and then navigating immediately races the optimizer, and the
 * capture that loses the race is the first one in the run, so the whole
 * sequence is abandoned before it produces anything.
 *
 * `installMock` is applied here too, so this exercises the same module graph
 * the captures will need rather than warming a subset of it.
 *
 * @param {import("@playwright/test").Browser} browser
 * @param {object} options
 * @param {string} options.baseUrl
 * @param {(page: import("@playwright/test").Page) => Promise<void>} options.installMock
 */
export async function warmUpDevServer(browser, { baseUrl, installMock }) {
  const page = await browser.newPage({
    viewport: { width: 1680, height: 960 },
    deviceScaleFactor: 1,
  });
  try {
    await installMock(page);
    await page.goto(baseUrl, {
      waitUntil: "domcontentloaded",
      timeout: COLD_START_NAVIGATION_TIMEOUT_MS,
    });
    await page
      .locator('[data-testid="app-shell"]')
      .waitFor({ timeout: COLD_START_NAVIGATION_TIMEOUT_MS });
  } finally {
    await page.close();
  }
}
