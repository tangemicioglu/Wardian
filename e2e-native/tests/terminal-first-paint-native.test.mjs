import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { By, Key } from "selenium-webdriver";

import {
  createNativeHarness,
  ensureNativeAppBuilt,
  prepareIsolatedHome,
  startNativeSession,
  waitForAppShell,
} from "../lib/harness.mjs";
import {
  focusSurfaceTab,
  openWorkbenchSurface,
} from "../lib/workbench.mjs";

const skipNativeBuild = process.env.WARDIAN_NATIVE_SKIP_BUILD === "1";
const RUN_ID = `${process.pid}-${Date.now()}`;
const ZELLIJ_PRESENTATION_ID = "desktop:zellij-habitat-terminal";
const AGENTS = Array.from({ length: 4 }, (_, index) => ({
  sessionId: `e2e-first-paint-${RUN_ID}-${index + 1}`,
  sessionName: `E2E-First-Paint-${String(index + 1).padStart(2, "0")}-${RUN_ID}`,
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

function createQuietMockScript() {
  const scriptPath = path.join(os.tmpdir(), `wardian-first-paint-${RUN_ID}.cjs`);
  const inputLogPath = path.join(os.tmpdir(), `wardian-first-paint-input-${RUN_ID}.jsonl`);
  fs.rmSync(inputLogPath, { force: true });
  const script = `
"use strict";
const fs = require("node:fs");
const inputLogPath = ${JSON.stringify(inputLogPath)};
const providerSessionId = process.env.WARDIAN_MOCK_SESSION_ID;
if (!providerSessionId) {
  throw new Error("WARDIAN_MOCK_SESSION_ID is required");
}
process.stdout.write(JSON.stringify({
  type: "init",
  session_id: providerSessionId,
  timestamp: new Date().toISOString(),
}) + "\\n");
for (let line = 1; line <= 8; line += 1) {
  process.stdout.write("first-paint-row-" + String(line).padStart(2, "0") + "\\r\\n");
}
setInterval(() => {}, 1000);
process.stdin.setEncoding("utf8");
let pendingInput = "";
process.stdin.on("data", (chunk) => {
  fs.appendFileSync(inputLogPath, JSON.stringify({ session_id: providerSessionId, chunk }) + "\\n");
  pendingInput += chunk;
  const lines = pendingInput.split(/\\r\\n|\\r|\\n/);
  pendingInput = lines.pop() || "";
  for (const line of lines) {
    process.stdout.write("received:" + providerSessionId + ":" + line + "\\r\\n");
  }
});
`;
  fs.writeFileSync(scriptPath, script, "utf8");
  return { inputLogPath, scriptPath };
}

async function spawnAgentsConcurrently(driver, requests) {
  const result = await driver.executeAsyncScript((spawnRequests, done) => {
    Promise.all(spawnRequests.map((req) => window.__TAURI_INTERNALS__.invoke("spawn_agent", { req })))
      .then(
        (agents) => done({ ok: true, agents }),
        (error) => done({ ok: false, error: String(error) }),
      );
  }, requests);
  assert.equal(result.ok, true, `concurrent spawn_agent failed: ${result.error}`);
  return result.agents;
}

async function selectGridMode(driver) {
  const selected = await driver.wait(async () => await driver.executeScript(() => {
    const buttons = [...document.querySelectorAll('[aria-label="Agents mode"] button')];
    const grid = buttons.find((button) => button.textContent?.trim() === "Grid");
    if (!grid) return false;
    grid.click();
    return true;
  }), 20_000, "Timed out locating the Agents Grid mode control");
  assert.equal(selected, true);
  await driver.wait(async () => await driver.executeScript(() => (
    document.querySelector('[data-testid="agent-grid"]')?.getAttribute("data-overview-mode") === "grid"
  )), 20_000, "Timed out selecting explicit Agents Grid mode");
}

async function waitForZellijTerminalGrid(driver, agents) {
  let lastState = null;
  try {
    return await driver.wait(async () => {
      const state = await driver.executeScript((sessionIds) => {
        const live = [...document.querySelectorAll('[data-zellij-presentation="live"]')]
          .map((node) => node.getAttribute("data-zellij-agent-id"));
        const previews = [...document.querySelectorAll('[data-zellij-presentation="preview"]')]
          .map((node) => ({
            sessionId: node.getAttribute("data-zellij-agent-id"),
            text: node.textContent || "",
            disabled: node.getAttribute("aria-disabled") === "true",
          }));
        return {
          live,
          previews,
          xterms: document.querySelectorAll('[data-testid="agent-terminal-host"] .xterm').length,
          liveHost: (() => {
            const host = document.querySelector(
              '[data-zellij-singleton-viewport="true"] [data-testid="agent-terminal-host"]',
            );
            return host ? {
              viewportSessionId: host.closest('[data-zellij-singleton-viewport="true"]')
                ?.getAttribute("data-zellij-agent-id"),
              sessionId: host.getAttribute("data-terminal-session-id"),
              visibility: getComputedStyle(host).visibility,
            } : null;
          })(),
          allAgentsPresent: sessionIds.every((sessionId) => (
            document.querySelector(`[data-zellij-agent-id="${CSS.escape(sessionId)}"]`)
          )),
        };
      }, agents.map((agent) => agent.sessionId));
      lastState = state;
      if (!state.allAgentsPresent || state.live.length !== 1 || state.xterms !== 1) return false;
      if (state.previews.length !== agents.length - 1) return false;
      if (state.previews.some((preview) => (
        preview.disabled || !preview.text.includes("first-paint-row-08")
      ))) return false;
      if (
        state.liveHost?.sessionId !== state.live[0]
        || state.liveHost.viewportSessionId !== state.live[0]
        || state.liveHost.visibility !== "visible"
      ) {
        return false;
      }
      return state;
    }, 40_000, "Timed out waiting for one live Zellij terminal and broker previews");
  } catch (error) {
    throw new Error(`${error.message}; last state: ${JSON.stringify(lastState)}`);
  }
}

test(
  "Agents share one live Zellij renderer and hand it off between broker previews",
  { timeout: 240_000 },
  async (t) => {
    const harness = await createNativeHarness();
    const { inputLogPath, scriptPath: mockScript } = createQuietMockScript();
    const previousMockScript = process.env.WARDIAN_MOCK_SCRIPT;
    let session = null;

    process.env.WARDIAN_MOCK_SCRIPT = mockScript;
    t.after(async () => {
      await session?.close();
      fs.rmSync(mockScript, { force: true });
      fs.rmSync(inputLogPath, { force: true });
      if (previousMockScript === undefined) delete process.env.WARDIAN_MOCK_SCRIPT;
      else process.env.WARDIAN_MOCK_SCRIPT = previousMockScript;
    });

    if (!skipNativeBuild) ensureNativeAppBuilt(harness);
    prepareIsolatedHome(harness);
    session = await startNativeSession(harness);
    const { driver } = session;
    await waitForAppShell(driver, 20_000);
    await driver.manage().window().setRect({ width: 1400, height: 900 });

    const spawned = await spawnAgentsConcurrently(driver, AGENTS.map((agent) => ({
      sessionName: agent.sessionName,
      agentClass: "TestClass",
      folder: harness.repoRoot,
      resumeSession: agent.sessionId,
      isOff: false,
      configOverride: { provider: "mock" },
    })));
    const spawnedAgents = spawned.map((spawnedAgent, index) => {
      const agent = AGENTS[index];
      assert.notEqual(spawnedAgent.session_id, agent.sessionId);
      return {
        ...agent,
        providerSessionId: agent.sessionId,
        sessionId: spawnedAgent.session_id,
      };
    });

    await openWorkbenchSurface(driver, "agents-overview");
    await selectGridMode(driver);
    for (const agent of spawnedAgents) {
      await driver.wait(async () => (
        (await driver.findElements(By.id(`agent-card-${agent.sessionId}`))).length === 1
      ), 20_000, `Timed out locating ${agent.sessionId}`);
    }

    const initial = await waitForZellijTerminalGrid(driver, spawnedAgents);
    const initialLiveAgent = initial.live[0];
    const nextAgent = spawnedAgents.find((agent) => agent.sessionId !== initialLiveAgent);
    assert.ok(nextAgent, "Expected an inactive agent terminal preview");
    await driver.wait(async () => await driver.executeScript(() => (
      document.querySelector('[data-terminal-renderer-instance-id]')
        ?.getAttribute("data-terminal-webgl-attempted") === "true"
    )), 20_000, "Singleton renderer never completed its one WebGL attempt");
    const initialRendererIdentity = await driver.executeScript(() => {
      const xterm = document.querySelector('[data-testid="agent-terminal-host"] .xterm');
      const renderer = document.querySelector('[data-terminal-renderer-instance-id]');
      xterm?.setAttribute("data-e2e-zellij-renderer", "singleton");
      xterm?.querySelector("canvas")?.setAttribute("data-e2e-zellij-canvas", "singleton");
      return renderer ? {
        instanceId: renderer.getAttribute("data-terminal-renderer-instance-id"),
        webglAttemptCount: renderer.getAttribute("data-terminal-webgl-attempt-count"),
        webglActivationCount: renderer.getAttribute("data-terminal-webgl-activation-count"),
      } : null;
    });
    assert.ok(initialRendererIdentity, "native singleton proof requires terminal debug identity");

    const activated = await driver.executeScript((sessionId) => {
      const preview = document.querySelector(
        `[data-zellij-presentation="preview"][data-zellij-agent-id="${CSS.escape(sessionId)}"]`,
      );
      if (!(preview instanceof HTMLElement) || preview.getAttribute("aria-disabled") === "true") {
        return false;
      }
      preview.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
      return true;
    }, nextAgent.sessionId);
    assert.equal(activated, true, `Could not activate preview ${nextAgent.sessionId}`);
    await driver.wait(async () => await driver.executeScript((sessionId) => (
      document.querySelector('[data-zellij-presentation="live"]')
        ?.getAttribute("data-zellij-agent-id") === sessionId
      && document.querySelectorAll('[data-testid="agent-terminal-host"] .xterm').length === 1
      && document.querySelector('[data-testid="agent-terminal-host"] .xterm')
        ?.getAttribute("data-e2e-zellij-renderer") === "singleton"
      && document.activeElement?.classList.contains("xterm-helper-textarea") === true
      && document.activeElement?.closest('[data-testid="agent-terminal-host"]')
        ?.getAttribute("data-terminal-session-id") === sessionId
    ), nextAgent.sessionId), 20_000, "Timed out handing the singleton renderer to the selected pane");

    await driver.actions().sendKeys("focused-handoff", Key.ENTER).perform();
    const focusedInputReceipts = await driver.wait(() => {
      if (!fs.existsSync(inputLogPath)) return false;
      const records = fs.readFileSync(inputLogPath, "utf8")
        .split(/\r?\n/)
        .filter(Boolean)
        .map((line) => JSON.parse(line));
      const selectedInput = records
        .filter((record) => record.session_id === nextAgent.providerSessionId)
        .map((record) => record.chunk)
        .join("");
      return selectedInput.includes("focused-handoff") ? records : false;
    }, 20_000, "Focused singleton xterm did not route the immediate key to the selected pane");
    assert.equal(
      focusedInputReceipts.some((record) => (
        record.session_id !== nextAgent.providerSessionId
        && record.chunk.includes("focused-handoff")
      )),
      false,
    );

    for (let index = 0; index < 20; index += 1) {
      const targetAgent = spawnedAgents[index % spawnedAgents.length];
      await driver.executeScript((sessionId) => {
        const preview = document.querySelector(
          `[data-zellij-presentation="preview"][data-zellij-agent-id="${CSS.escape(sessionId)}"]`,
        );
        preview?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
      }, targetAgent.sessionId);
      await driver.wait(async () => await driver.executeScript((sessionId) => (
        document.querySelector('[data-zellij-presentation="live"]')
          ?.getAttribute("data-zellij-agent-id") === sessionId
      ), targetAgent.sessionId), 20_000, `Timed out focusing ${targetAgent.sessionId}`);
    }

    // Keep the targeted-input assertions below bound to the originally
    // selected agent after the repeated identity stress cycle.
    await driver.executeScript((sessionId) => {
      const preview = document.querySelector(
        `[data-zellij-presentation="preview"][data-zellij-agent-id="${CSS.escape(sessionId)}"]`,
      );
      preview?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    }, nextAgent.sessionId);
    await driver.wait(async () => await driver.executeScript((sessionId) => (
      document.querySelector('[data-zellij-presentation="live"]')
        ?.getAttribute("data-zellij-agent-id") === sessionId
    ), nextAgent.sessionId), 20_000, "Timed out restoring the selected pane after identity stress");

    const finalRendererIdentity = await driver.executeScript(() => {
      const xterm = document.querySelector('[data-testid="agent-terminal-host"] .xterm');
      const renderer = document.querySelector('[data-terminal-renderer-instance-id]');
      return {
        xtermStable: xterm?.getAttribute("data-e2e-zellij-renderer") === "singleton",
        canvasStable: !xterm?.querySelector("canvas")
          || xterm.querySelector("canvas")?.getAttribute("data-e2e-zellij-canvas") === "singleton",
        instanceId: renderer?.getAttribute("data-terminal-renderer-instance-id") ?? null,
        webglAttemptCount: renderer?.getAttribute("data-terminal-webgl-attempt-count") ?? null,
        webglActivationCount:
          renderer?.getAttribute("data-terminal-webgl-activation-count") ?? null,
      };
    });
    assert.equal(finalRendererIdentity.xtermStable, true);
    assert.equal(finalRendererIdentity.canvasStable, true);
    assert.equal(finalRendererIdentity.instanceId, initialRendererIdentity.instanceId);
    assert.equal(
      finalRendererIdentity.webglAttemptCount,
      initialRendererIdentity.webglAttemptCount,
      "card focus must not retry or recreate the singleton WebGL addon",
    );
    assert.equal(
      finalRendererIdentity.webglActivationCount,
      initialRendererIdentity.webglActivationCount,
      "card focus must not recreate the singleton WebGL addon",
    );

    const selectedSnapshot = await invokeTauri(driver, "request_terminal_snapshot", {
      request: { session_id: nextAgent.sessionId },
    });
    let latestBrokerState = null;
    await driver.wait(async () => {
      const updated = await invokeTauri(driver, "update_terminal_presentation", {
        request: {
          presentation_id: ZELLIJ_PRESENTATION_ID,
          session_id: nextAgent.sessionId,
          runtime_generation: selectedSnapshot.runtime_generation,
          desired_geometry: { cols: 120, rows: 40 },
          visibility: "visible",
          render_state: "mounted",
          requested_interaction: "interactive",
          observed_lease_epoch: 0,
        },
      });
      latestBrokerState = updated.broker_state;
      return updated.broker_state.owner_presentation_id === ZELLIJ_PRESENTATION_ID;
    }, 20_000, `Singleton presentation did not own selected pane: ${JSON.stringify(latestBrokerState)}`);

    const inputDecision = await invokeTauri(driver, "send_terminal_presentation_input", {
      request: {
        session_id: nextAgent.sessionId,
        presentation_id: ZELLIJ_PRESENTATION_ID,
        runtime_generation: selectedSnapshot.runtime_generation,
        lease_epoch: latestBrokerState.lease_epoch,
        input: "handoff-probe\r",
      },
    });
    assert.equal(inputDecision.status, "accepted");
    let latestReceipts = [];
    let receipts;
    try {
      receipts = await driver.wait(() => {
        if (!fs.existsSync(inputLogPath)) return false;
        const records = fs.readFileSync(inputLogPath, "utf8")
          .split(/\r?\n/)
          .filter(Boolean)
          .map((line) => JSON.parse(line));
        latestReceipts = records;
        const selectedInput = records
          .filter((record) => record.session_id === nextAgent.providerSessionId)
          .map((record) => record.chunk)
          .join("");
        return selectedInput.includes("handoff-probe") ? records : false;
      }, 20_000, "Selected Zellij provider pane did not receive broker input");
    } catch (error) {
      throw new Error(`${error.message}; provider receipts: ${JSON.stringify(latestReceipts)}`);
    }
    assert.equal(
      receipts.some((record) => (
        record.session_id !== nextAgent.providerSessionId && record.chunk.includes("handoff-probe")
      )),
      false,
    );
    const routedOutput = await driver.wait(async () => {
      const previews = await Promise.all(spawnedAgents.map(async (agent) => ({
        sessionId: agent.sessionId,
        preview: await invokeTauri(driver, "get_zellij_terminal_preview", {
          sessionId: agent.sessionId,
        }),
      })));
      const target = previews.find((entry) => entry.sessionId === nextAgent.sessionId);
      if (!target?.preview.content.includes("handoff-probe")) {
        return false;
      }
      return previews;
    }, 20_000, "Timed out routing input to the activated Zellij pane");
    for (const entry of routedOutput) {
      if (entry.sessionId !== nextAgent.sessionId) {
        assert.equal(entry.preview.content.includes("handoff-probe"), false);
      }
    }

    await openWorkbenchSurface(driver, "workflows");
    for (let cycle = 0; cycle < 3; cycle += 1) {
      await focusSurfaceTab(driver, "workflows");
      await focusSurfaceTab(driver, "agents-overview");
      const returned = await waitForZellijTerminalGrid(driver, spawnedAgents);
      assert.equal(returned.live[0], nextAgent.sessionId);
      const rendererCount = await driver.executeScript(() => (
        document.querySelectorAll('[data-testid="agent-terminal-host"] .xterm').length
      ));
      assert.equal(rendererCount, 1);
    }

    const screenshotDirectory = path.join(
      harness.repoRoot,
      "e2e",
      "screenshots",
      "zellij-terminal",
      "2026-08-24",
    );
    fs.mkdirSync(screenshotDirectory, { recursive: true });
    fs.writeFileSync(
      path.join(screenshotDirectory, "singleton-grid.png"),
      Buffer.from(await driver.takeScreenshot(), "base64"),
    );
  },
);
