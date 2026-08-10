import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { By } from "selenium-webdriver";

import {
  createNativeHarness,
  ensureNativeAppBuilt,
  invokeTauri,
  prepareIsolatedHome,
  startNativeSession,
  waitForAppShell,
} from "../lib/harness.mjs";
import { openWorkbenchSurface, waitForWorkbenchReady } from "../lib/workbench.mjs";

/**
 * Proves that a browser surface reopens its page after a real app restart.
 *
 * This has to be native, and it has to restart. Browser E2E can prove the
 * gating rules against a mocked client, but the claim here spans a durable
 * workbench document written to disk, a process exit that takes every session
 * with it, and a fresh process that reads the document back and mints a new
 * runtime for it. Nothing below the native layer contains all three.
 */

const skipNativeBuild = process.env.WARDIAN_NATIVE_SKIP_BUILD === "1";
const RESTORE_TIMEOUT_MS = 60_000;
const SCREENSHOT_DATE = "2026-08-09";

const FIXTURE = `<!doctype html>
<html><head><title>Restored page</title></head>
<body style="font-family: system-ui; background:#0d1117; color:#e6edf3; padding:2rem">
<h1 id="marker">Restored page</h1></body></html>`;

/** Serves one page on an ephemeral loopback port. */
async function serveFixture(t) {
  const server = http.createServer((_request, response) => {
    response.writeHead(200, {
      "Content-Type": "text/html; charset=utf-8",
      "Content-Length": Buffer.byteLength(FIXTURE),
    });
    response.end(FIXTURE);
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(() => new Promise((resolve) => server.close(resolve)));
  return `http://127.0.0.1:${server.address().port}/`;
}

/** Polls the durable workbench document until it satisfies `predicate`. */
async function waitForDocument(primaryPath, predicate, what) {
  const deadline = Date.now() + RESTORE_TIMEOUT_MS;
  let last = null;
  while (Date.now() < deadline) {
    try {
      last = JSON.parse(fs.readFileSync(primaryPath, "utf8"));
      if (predicate(last)) return last;
    } catch {
      // A save in flight can be observed mid-write; the next poll sees it whole.
    }
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error(`timed out waiting for ${what}; last document was ${JSON.stringify(last)}`);
}

/** The visible browser surface, once one has rendered. */
async function browserSurface(driver, what) {
  return await driver.wait(async () => {
    const found = await driver.findElements(By.css('[data-testid="browser-surface"]'));
    for (const candidate of found) if (await candidate.isDisplayed()) return candidate;
    return false;
  }, RESTORE_TIMEOUT_MS, what);
}

test("a browser surface reopens its page after the app restarts", async (t) => {
  const harness = await createNativeHarness();
  if (!skipNativeBuild) ensureNativeAppBuilt(harness);

  prepareIsolatedHome(harness);
  // A fresh home shows the guided tour, whose backdrop intercepts every click.
  const onboarding = path.join(harness.isolatedHome, "settings", "onboarding.json");
  fs.mkdirSync(path.dirname(onboarding), { recursive: true });
  fs.writeFileSync(
    onboarding,
    JSON.stringify({
      dismissed_hint_ids: [],
      contextual_tips_enabled: false,
      guided_tour_state: "skipped",
    }),
  );
  const baseUrl = await serveFixture(t);
  const primaryPath = path.join(harness.isolatedHome, "settings", "workbench.json");

  let session = await startNativeSession(harness);
  t.after(async () => { await session?.close(); });
  await waitForAppShell(session.driver, 30_000);

  const engine = await invokeTauri(session.driver, "browser_engine_status");
  assert.equal(
    engine.available,
    true,
    `no Chromium on this host: ${engine.detail ?? "unknown"}`,
  );

  await waitForWorkbenchReady(session.driver, 30_000);
  await openWorkbenchSurface(session.driver, "browser", undefined, { timeoutMs: 40_000 });

  const before = await browserSurface(session.driver, "the browser surface never rendered");
  const originalBrowserId = await before.getAttribute("data-resource-key");
  assert.ok(originalBrowserId, "the surface must carry the session it presents");

  // Drive it to a real page so there is an address worth restoring. The
  // address bar is the surface's own control, which holds the drive lease.
  const address = await session.driver.findElement(
    By.css('[data-testid="browser-surface-address"]'),
  );
  await address.clear();
  await address.sendKeys(baseUrl, "\n");
  await session.driver.wait(async () => {
    const state = await session.driver.findElement(
      By.css('[data-testid="browser-surface-load-state"]'),
    );
    return (await state.getText()) === "Ready";
  }, RESTORE_TIMEOUT_MS, "the page never loaded before the restart");

  // The URL is what survives; the session id means nothing after a restart.
  const persisted = await waitForDocument(
    primaryPath,
    (document) => Object.values(document.surfaces ?? {}).some((surface) => (
      surface.surface_type === "browser" && String(surface.state?.url ?? "").startsWith(baseUrl)
    )),
    "the browser surface to persist its address",
  );
  const persistedSurface = Object.values(persisted.surfaces).find(
    (surface) => surface.surface_type === "browser",
  );
  assert.equal(
    persistedSurface.resource_key,
    originalBrowserId,
    "the persisted surface should still name the session it was bound to",
  );

  // Exiting takes every browser session with it, which is the whole premise:
  // the id in the persisted document resolves to nothing on the way back up.
  await session.close();
  session = await startNativeSession(harness);
  await waitForAppShell(session.driver, 30_000);
  await waitForWorkbenchReady(session.driver, 30_000);

  const after = await browserSurface(session.driver, "the browser surface did not come back");

  // No placeholder, no button: a visible restored tab reopens itself.
  await session.driver.wait(async () => {
    const placeholders = await session.driver.findElements(
      By.css('[data-missing-session="true"]'),
    );
    return placeholders.length === 0;
  }, RESTORE_TIMEOUT_MS, "the restored surface stayed on its unavailable placeholder");

  await session.driver.wait(async () => {
    const state = await session.driver.findElement(
      By.css('[data-testid="browser-surface-load-state"]'),
    );
    return (await state.getText()) === "Ready";
  }, RESTORE_TIMEOUT_MS, "the restored page never reported a completed load");

  const restoredAddress = await session.driver.findElement(
    By.css('[data-testid="browser-surface-address"]'),
  );
  assert.equal(
    await restoredAddress.getAttribute("value"),
    baseUrl,
    "the restored surface should be showing the page it was persisted with",
  );

  // Same page, different runtime. A restore that somehow kept the old id would
  // be presenting a session that no longer exists.
  const restoredBrowserId = await after.getAttribute("data-resource-key");
  assert.notEqual(
    restoredBrowserId,
    originalBrowserId,
    "a restore mints a fresh session rather than resurrecting the dead id",
  );

  const sessions = await invokeTauri(session.driver, "list_browser_sessions");
  assert.equal(sessions.length, 1, `expected exactly one session, got ${JSON.stringify(sessions)}`);
  assert.equal(sessions[0].browser_id, restoredBrowserId);
  assert.equal(sessions[0].url, baseUrl);

  // PR evidence: this is the moment that used to show the unavailable
  // placeholder, captured after the restart rather than staged.
  const screenshotDir = path.join(
    harness.repoRoot,
    "e2e",
    "screenshots",
    "browser-session-restore",
    SCREENSHOT_DATE,
  );
  fs.mkdirSync(screenshotDir, { recursive: true });
  fs.writeFileSync(
    path.join(screenshotDir, "restored-after-restart.png"),
    await session.driver.takeScreenshot(),
    "base64",
  );
});
