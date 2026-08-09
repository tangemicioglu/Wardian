import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
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
 * Exercises the browser surface against a real Chromium.
 *
 * This has to be native. Browser E2E can mock the session client, but it
 * cannot prove that a Chromium starts, that CDP drives it, that a screencast
 * frame reaches the surface, or that a ref taken before a navigation is
 * refused afterwards. Those are the claims the feature rests on.
 */

const skipNativeBuild = process.env.WARDIAN_NATIVE_SKIP_BUILD === "1";
const SCREENSHOT_DATE = "2026-08-09";

const FIXTURE = `<!doctype html>
<html><head><title>Wardian Browser Surface</title>
<style>
  body { font-family: system-ui, sans-serif; padding: 2rem; background: #0d1117; color: #e6edf3; }
  input, button { font-size: 1rem; padding: .5rem .75rem; margin-right: .5rem; }
  #out { margin-top: 1rem; color: #7ee787; }
</style></head>
<body>
  <h1 id="heading">Agent browser surface</h1>
  <p>A page an agent drives through <code>wardian browser</code>.</p>
  <input id="q" placeholder="Search" />
  <button id="go" onclick="document.getElementById('out').textContent = 'searched for ' + document.getElementById('q').value">Go</button>
  <p id="out"></p>
  <p><a id="next" href="/second">Second page</a></p>
</body></html>`;

const SECOND = `<!doctype html>
<html><head><title>Second page</title></head>
<body style="font-family: system-ui; background:#0d1117; color:#e6edf3; padding:2rem">
<h1 id="marker">Second page</h1></body></html>`;

/** Serves the fixture on an ephemeral loopback port. */
async function serveFixture(t) {
  const server = http.createServer((request, response) => {
    const body = request.url?.startsWith("/second") ? SECOND : FIXTURE;
    response.writeHead(200, {
      "Content-Type": "text/html; charset=utf-8",
      "Content-Length": Buffer.byteLength(body),
    });
    response.end(body);
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(() => new Promise((resolve) => server.close(resolve)));
  return `http://127.0.0.1:${server.address().port}/`;
}

async function requireInvoke(driver, command, args = {}) {
  return await invokeTauri(driver, command, args);
}

function commandName(name) {
  return process.platform === "win32" ? `${name}.exe` : name;
}

/** Builds `wardian-cli` and returns its path, matching the CLI shared-state test. */
function buildCli(harness) {
  const build = spawnSync("cargo", ["build", "-p", "wardian-cli", "--bin", "wardian-cli"], {
    cwd: harness.repoRoot,
    encoding: "utf8",
  });
  assert.equal(build.status, 0, `cargo build -p wardian-cli failed
${build.stderr}`);
  const local = path.join(harness.repoRoot, "target", "debug", commandName("wardian-cli"));
  if (existsSync(local)) return local;
  const metadata = spawnSync("cargo", ["metadata", "--no-deps", "--format-version", "1"], {
    cwd: harness.repoRoot,
    encoding: "utf8",
  });
  assert.equal(metadata.status, 0, `cargo metadata failed
${metadata.stderr}`);
  const candidate = path.join(
    JSON.parse(metadata.stdout).target_directory,
    "debug",
    commandName("wardian-cli"),
  );
  assert.equal(existsSync(candidate), true, `wardian-cli was not found at ${candidate}`);
  return candidate;
}

/**
 * Runs the CLI against the same isolated home the app is using.
 *
 * Deliberately async: `spawnSync` would block this process's event loop, and
 * this process also serves the fixture the page navigates to. Blocking here
 * would deadlock the CLI against the server it is waiting on.
 */
function runCli(cliPath, harness, args) {
  const env = { ...process.env, WARDIAN_HOME: harness.isolatedHome };
  delete env.WARDIAN_SESSION_ID;
  return new Promise((resolve, reject) => {
    const child = spawn(cliPath, args, { cwd: harness.repoRoot, env });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.on("error", reject);
    child.on("close", (status) => resolve({ status, stdout, stderr }));
  });
}

async function runCliOk(cliPath, harness, args) {
  const result = await runCli(cliPath, harness, args);
  assert.equal(
    result.status,
    0,
    `wardian ${args.join(" ")} failed
stdout:
${result.stdout}
stderr:
${result.stderr}`,
  );
  return result.stdout;
}

test("browser surface drives a real page and refuses stale refs", async (t) => {
  const harness = await createNativeHarness();
  if (!skipNativeBuild) ensureNativeAppBuilt(harness);

  prepareIsolatedHome(harness);
  // A fresh home shows the guided tour, whose backdrop would intercept every
  // workbench click this test makes.
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

  const session = await startNativeSession(harness);
  t.after(async () => { await session?.close(); });
  await waitForAppShell(session.driver, 30_000);
  const { driver } = session;

  const engine = await requireInvoke(driver, "browser_engine_status");
  assert.equal(
    engine.available,
    true,
    `no Chromium on this host: ${engine.detail ?? "unknown"}`,
  );

  // Open through the launcher so the provisioning path is what is under test:
  // the contribution has to create its own session before the surface opens.
  await waitForWorkbenchReady(driver, 30_000);
  await openWorkbenchSurface(driver, "browser", undefined, { timeoutMs: 40_000 });

  const surface = await driver.wait(async () => {
    const found = await driver.findElements(By.css('[data-testid="browser-surface"]'));
    for (const candidate of found) if (await candidate.isDisplayed()) return candidate;
    return false;
  }, 60_000, "the browser surface never rendered");

  const browserId = await surface.getAttribute("data-resource-key");
  assert.ok(browserId, "the surface must carry the session it presents");

  const navigated = await requireInvoke(driver, "navigate_browser_session", {
    browserId,
    action: `${baseUrl}`,
  });
  assert.equal(navigated.browser_id, browserId);

  // A frame proves the screencast path end to end: CDP event, backend
  // broadcast, Tauri event, and the surface's own render.
  await driver.wait(async () => {
    const frames = await driver.findElements(By.css('[data-testid="browser-surface-frame"]'));
    return frames.length > 0 && await frames[0].isDisplayed();
  }, 60_000, "no screencast frame reached the surface");

  await driver.wait(async () => {
    const state = await driver.findElement(By.css('[data-testid="browser-surface-load-state"]'));
    return (await state.getText()) === "Ready";
  }, 60_000, "the page never reported a completed load");

  const address = await driver.findElement(By.css('[data-testid="browser-surface-address"]'));
  assert.match(await address.getAttribute("value"), /127\.0\.0\.1/);

  const shortRef = await driver.findElement(By.css('[data-testid="browser-surface-short-ref"]'));
  assert.match(await shortRef.getText(), /^browser:\d+$/);

  // The agent path: the same CLI a human uses, against the app's own runtime.
  const cliPath = buildCli(harness);
  const listed = await runCliOk(cliPath, harness, ["browser", "list"]);
  assert.match(listed, /browser:\d+/, `browser list did not show the session:
${listed}`);

  const snapshotJson = JSON.parse(
    await runCliOk(cliPath, harness, ["browser", "--json", browserId, "snapshot", "--interactive"]),
  );
  const elements = snapshotJson.snapshot.elements;
  const searchBox = elements.find((element) => element.name === "Search");
  const goButton = elements.find((element) => element.name.trim() === "Go");
  assert.ok(searchBox, `the search box was not in the snapshot: ${JSON.stringify(elements)}`);
  assert.ok(goButton, `the button was not in the snapshot: ${JSON.stringify(elements)}`);
  assert.equal(
    Object.hasOwn(searchBox, "checked"),
    false,
    "a text input must not report a checked state",
  );

  await runCliOk(cliPath, harness, ["browser", browserId, "fill", searchBox.element_ref, "wardian"]);
  await runCliOk(cliPath, harness, ["browser", browserId, "click", goButton.element_ref]);
  const outcome = await runCliOk(cliPath, harness, ["browser", browserId, "get", "text", "#out"]);
  assert.match(outcome, /searched for wardian/, `the page handler did not run:
${outcome}`);

  // A ref minted before a navigation must be refused, not applied to whatever
  // now occupies that position.
  const staleRef = goButton.element_ref;
  await runCliOk(cliPath, harness, ["browser", browserId, "navigate", `${baseUrl}second`]);
  await runCliOk(cliPath, harness, [
    "browser", browserId, "wait", "--url-contains", "/second", "--timeout-ms", "10000",
  ]);
  const stale = await runCli(cliPath, harness, [
    "browser", "--json", browserId, "click", staleRef,
  ]);
  assert.notEqual(stale.status, 0, "a stale ref must not succeed");
  assert.equal(
    JSON.parse(stale.stdout || stale.stderr).error.code,
    "snapshot_stale",
    `expected snapshot_stale, got:
${stale.stdout}
${stale.stderr}`,
  );

  await runCliOk(cliPath, harness, ["browser", browserId, "navigate", baseUrl]);
  await driver.wait(async () => {
    const state = await driver.findElement(By.css('[data-testid="browser-surface-load-state"]'));
    return (await state.getText()) === "Ready";
  }, 30_000, "the page never reloaded after the stale-ref check");

  const screenshotDir = path.join(
    harness.repoRoot,
    "e2e",
    "screenshots",
    "agent-browser-surface",
    SCREENSHOT_DATE,
  );
  fs.mkdirSync(screenshotDir, { recursive: true });
  fs.writeFileSync(
    path.join(screenshotDir, "browser-surface.png"),
    await driver.takeScreenshot(),
    "base64",
  );
});
