import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { By } from "selenium-webdriver";

import {
  createNativeHarness,
  ensureNativeAppBuilt,
  prepareIsolatedHome,
  startNativeSession,
  waitForAppShell,
} from "../lib/harness.mjs";
import { openWorkbenchSurface } from "../lib/workbench.mjs";

const skipNativeBuild = process.env.WARDIAN_NATIVE_SKIP_BUILD === "1";
const RUN_ID = `${process.pid}-${Date.now()}`;
const AGENTS = Array.from({ length: 3 }, (_, index) => ({
  providerSessionId: `e2e-vis-${RUN_ID}-${index + 1}`,
  sessionName: `E2E-Visibility-${String(index + 1).padStart(2, "0")}-${RUN_ID}`,
}));

async function invokeTauri(driver, command, args = {}) {
  const result = await driver.executeAsyncScript((cmd, payload, done) => {
    window.__TAURI_INTERNALS__.invoke(cmd, payload).then(
      (value) => done({ ok: true, value }),
      (error) => done({ ok: false, error: String(error) }),
    );
  }, command, args);
  assert.equal(result.ok, true, `${command} failed: ${result.error}`);
  return result.value;
}

function createEchoMockScript() {
  const scriptPath = path.join(os.tmpdir(), `wardian-vis-mock-${RUN_ID}.cjs`);
  const inputLogPath = path.join(os.tmpdir(), `wardian-vis-input-${RUN_ID}.jsonl`);
  fs.rmSync(inputLogPath, { force: true });
  const script = `
"use strict";
const fs = require("node:fs");
const inputLogPath = ${JSON.stringify(inputLogPath)};
const providerSessionId = process.env.WARDIAN_MOCK_SESSION_ID;
if (!providerSessionId) throw new Error("WARDIAN_MOCK_SESSION_ID is required");
process.stdout.write(JSON.stringify({
  type: "init",
  session_id: providerSessionId,
  timestamp: new Date().toISOString(),
}) + "\\n");
for (let line = 1; line <= 12; line += 1) {
  process.stdout.write("visibility-row-" + String(line).padStart(2, "0") + "\\r\\n");
}
process.stdin.setEncoding("utf8");
if (process.stdin.isTTY) process.stdin.setRawMode(true);
process.stdin.on("data", (chunk) => {
  fs.appendFileSync(inputLogPath, JSON.stringify({ session_id: providerSessionId, chunk }) + "\\n");
});
setInterval(() => {}, 1000);
`;
  fs.writeFileSync(scriptPath, script, "utf8");
  return { inputLogPath, scriptPath };
}

async function selectGridMode(driver) {
  await driver.wait(async () => await driver.executeScript(() => {
    const grid = [...document.querySelectorAll('[aria-label="Agents mode"] button')]
      .find((button) => button.textContent?.trim() === "Grid");
    if (!grid) return false;
    grid.click();
    return true;
  }), 20_000, "Timed out locating the Agents Grid mode control");
  await driver.wait(async () => await driver.executeScript(() => (
    document.querySelector('[data-testid="agent-grid"]')?.getAttribute("data-overview-mode") === "grid"
  )), 20_000, "Timed out selecting explicit Agents Grid mode");
}

async function selectPassiveCard(driver, sessionId, key = null) {
  const selected = await driver.executeScript((sid, firstKey) => {
    const card = document.getElementById(`agent-card-${sid}`);
    const preview = card?.querySelector(
      `[data-zellij-presentation="preview"][data-zellij-agent-id="${CSS.escape(sid)}"]`,
    );
    if (!(preview instanceof HTMLElement) || preview.getAttribute("aria-disabled") === "true") {
      return false;
    }
    preview.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    if (firstKey) {
      preview.dispatchEvent(new KeyboardEvent("keydown", {
        key: firstKey,
        bubbles: true,
        cancelable: true,
      }));
    }
    return true;
  }, sessionId, key);
  assert.equal(selected, true, `Expected a usable passive card for ${sessionId}`);
}

test(
  "offscreen cards stay passive while one singleton renderer moves between panes",
  { timeout: 240_000 },
  async (t) => {
    const harness = await createNativeHarness();
    const { inputLogPath, scriptPath } = createEchoMockScript();
    const previousMockScript = process.env.WARDIAN_MOCK_SCRIPT;
    let session = null;

    process.env.WARDIAN_MOCK_SCRIPT = scriptPath;
    t.after(async () => {
      await session?.close();
      fs.rmSync(scriptPath, { force: true });
      fs.rmSync(inputLogPath, { force: true });
      if (previousMockScript === undefined) delete process.env.WARDIAN_MOCK_SCRIPT;
      else process.env.WARDIAN_MOCK_SCRIPT = previousMockScript;
    });

    if (!skipNativeBuild) ensureNativeAppBuilt(harness);
    prepareIsolatedHome(harness);
    session = await startNativeSession(harness);
    const { driver } = session;
    await waitForAppShell(driver, 20_000);
    await driver.manage().window().setRect({ width: 1400, height: 520 });

    const spawned = [];
    for (const agent of AGENTS) {
      const active = await invokeTauri(driver, "spawn_agent", {
        req: {
          sessionName: agent.sessionName,
          agentClass: "TestClass",
          folder: harness.repoRoot,
          resumeSession: agent.providerSessionId,
          isOff: false,
          configOverride: { provider: "mock" },
        },
      });
      spawned.push({ ...agent, sessionId: active.session_id });
    }

    await openWorkbenchSurface(driver, "agents-overview");
    await selectGridMode(driver);
    for (const agent of spawned) {
      await driver.wait(async () => (
        (await driver.findElements(By.id(`agent-card-${agent.sessionId}`))).length === 1
      ), 20_000, `Timed out locating ${agent.sessionId}`);
    }

    const passiveState = await driver.executeScript(() => ({
      cardHosts: [...document.querySelectorAll('[id^="agent-card-"]')]
        .reduce((count, card) => count + card.querySelectorAll('[data-testid="agent-terminal-host"]').length, 0),
      previews: document.querySelectorAll('[data-zellij-presentation="preview"]').length,
      singletonHosts: document.querySelectorAll(
        '[data-zellij-singleton-viewport="true"] [data-testid="agent-terminal-host"]',
      ).length,
      xterms: document.querySelectorAll('[data-testid="agent-terminal-host"] .xterm').length,
    }));
    assert.deepEqual(passiveState, {
      cardHosts: 0,
      previews: spawned.length,
      singletonHosts: 0,
      xterms: 0,
    });

    await driver.executeScript((sid) => {
      document.getElementById(`agent-card-${sid}`)?.scrollIntoView({ block: "center" });
    }, spawned[0].sessionId);
    await selectPassiveCard(driver, spawned[0].sessionId);
    let firstSelectionState = null;
    try {
      await driver.wait(async () => {
        firstSelectionState = await driver.executeScript((sid) => {
          const host = document.querySelector(
            '[data-zellij-singleton-viewport="true"] [data-testid="agent-terminal-host"]',
          );
          const preview = document.querySelector(
            `[data-zellij-presentation="preview"][data-zellij-agent-id="${CSS.escape(sid)}"]`,
          );
          return {
            activeSessionId: host?.getAttribute("data-terminal-session-id") ?? null,
            previewDisabled: preview?.getAttribute("aria-disabled") ?? null,
            previewText: preview?.textContent ?? null,
            viewportCount: document.querySelectorAll('[data-zellij-singleton-viewport="true"]').length,
            xtermCount: document.querySelectorAll('[data-testid="agent-terminal-host"] .xterm').length,
          };
        }, spawned[0].sessionId);
        return firstSelectionState.activeSessionId === spawned[0].sessionId
          && firstSelectionState.xtermCount === 1;
      }, 20_000, "Singleton renderer did not select the first card");
    } catch (error) {
      throw new Error(`${error.message}; last state: ${JSON.stringify(firstSelectionState)}`);
    }

    const rendererIdentity = await driver.executeScript(() => {
      const xterm = document.querySelector('[data-testid="agent-terminal-host"] .xterm');
      const renderer = document.querySelector('[data-terminal-renderer-instance-id]');
      xterm?.setAttribute("data-e2e-singleton-identity", "stable");
      return renderer?.getAttribute("data-terminal-renderer-instance-id") ?? null;
    });
    assert.ok(rendererIdentity, "Expected the singleton renderer debug identity");

    const last = spawned.at(-1);
    await driver.executeScript((sid) => {
      document.getElementById(`agent-card-${sid}`)?.scrollIntoView({ block: "center" });
    }, last.sessionId);
    const cardContract = await driver.executeScript((sid) => {
      const card = document.getElementById(`agent-card-${sid}`);
      return {
        hasHost: Boolean(card?.querySelector('[data-testid="agent-terminal-host"]')),
        hasPreview: Boolean(card?.querySelector('[data-zellij-presentation="preview"]')),
      };
    }, last.sessionId);
    assert.deepEqual(cardContract, { hasHost: false, hasPreview: true });

    await selectPassiveCard(driver, last.sessionId, "v");
    await driver.wait(async () => await driver.executeScript((sid, instanceId) => {
      const hosts = document.querySelectorAll('[data-testid="agent-terminal-host"]');
      const xterms = document.querySelectorAll('[data-testid="agent-terminal-host"] .xterm');
      const host = hosts[0];
      const renderer = document.querySelector('[data-terminal-renderer-instance-id]');
      return hosts.length === 1
        && xterms.length === 1
        && xterms[0].getAttribute("data-e2e-singleton-identity") === "stable"
        && host?.getAttribute("data-terminal-session-id") === sid
        && renderer?.getAttribute("data-terminal-renderer-instance-id") === instanceId;
    }, last.sessionId, rendererIdentity), 20_000, "Singleton renderer identity changed during handoff");

    const receipts = await driver.wait(() => {
      if (!fs.existsSync(inputLogPath)) return false;
      const records = fs.readFileSync(inputLogPath, "utf8")
        .split(/\r?\n/)
        .filter(Boolean)
        .map((line) => JSON.parse(line));
      return records.some((record) => (
        record.session_id === last.providerSessionId && record.chunk.includes("v")
      )) ? records : false;
    }, 20_000, "Buffered handoff input did not reach the selected offscreen pane");
    assert.equal(receipts.some((record) => (
      record.session_id !== last.providerSessionId && record.chunk.includes("v")
    )), false, "Buffered handoff input reached the wrong provider pane");
  },
);
