import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { By, until } from "selenium-webdriver";

import {
  createNativeHarness,
  ensureNativeAppBuilt,
  prepareIsolatedHome,
  startNativeSession,
  waitForAppShell,
} from "../lib/harness.mjs";
import {
  readTerminalDebugSnapshot,
  resolveAgentTerminalPresentationId,
} from "../lib/terminal-debug.mjs";
import { openWorkbenchSurface } from "../lib/workbench.mjs";

const skipNativeBuild = process.env.WARDIAN_NATIVE_SKIP_BUILD === "1";
const RUN_ID = `${process.pid}-${Date.now()}`;
const PROVIDER_SESSION_ID = `e2e-background-resume-${RUN_ID}`;
const SESSION_NAME = `E2E-Background-Resume-${RUN_ID}`;
const BACKGROUND_PREFIX = `BACKGROUND_RESUME_${RUN_ID}`;
const BACKGROUND_MARKER = `${BACKGROUND_PREFIX}_DONE`;

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

async function waitFor(label, timeoutMs, probe) {
  const startedAt = Date.now();
  let last = null;
  while (Date.now() - startedAt < timeoutMs) {
    last = await probe();
    if (last?.ok) {
      return last;
    }
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error(`Timed out waiting for ${label}: ${JSON.stringify(last)}`);
}

function createEchoMockScript() {
  const scriptPath = path.join(os.tmpdir(), `wardian-background-resume-${RUN_ID}.cjs`);
  const script = `
"use strict";
const sessionId = process.env.WARDIAN_MOCK_SESSION_ID;
if (!sessionId) throw new Error("WARDIAN_MOCK_SESSION_ID is required");
process.stdout.write(JSON.stringify({
  type: "init",
  session_id: sessionId,
  timestamp: new Date().toISOString(),
}) + "\\n");
process.stdout.write("background-resume-ready\\r\\n");
let pending = "";
process.stdin.on("data", (chunk) => {
  pending += chunk.toString();
  let newline = pending.search(/[\\r\\n]/);
  while (newline >= 0) {
    const line = pending.slice(0, newline).trim();
    pending = pending.slice(newline + 1);
    if (line) process.stdout.write("echo:" + line + "\\r\\n");
    newline = pending.search(/[\\r\\n]/);
  }
});
process.stdin.resume();
`;
  fs.writeFileSync(scriptPath, script, "utf8");
  return scriptPath;
}

async function activatePresentation(driver, sessionId, presentationId) {
  const clicked = await driver.executeScript((sid, pid) => {
    const card = document.getElementById(`agent-card-${sid}`);
    const host = [...(card?.querySelectorAll('[data-testid="agent-terminal-host"]') ?? [])]
      .find((candidate) => candidate.getAttribute("data-terminal-presentation-id") === pid);
    if (!host) return false;
    host.click();
    return true;
  }, sessionId, presentationId);
  assert.equal(clicked, true, `Expected terminal presentation ${presentationId} to activate`);
  await waitFor("terminal input lease", 20_000, async () => {
    const snapshot = await readTerminalDebugSnapshot(driver, presentationId);
    return {
      ok: snapshot?.broker?.ownerPresentationId === presentationId,
      owner: snapshot?.broker?.ownerPresentationId ?? null,
    };
  });
}

async function sendTerminalInput(driver, sessionId, presentationId, input) {
  const snapshot = await readTerminalDebugSnapshot(driver, presentationId);
  assert.equal(snapshot?.broker?.ownerPresentationId, presentationId, "Expected terminal input owner");
  await invokeTauri(driver, "send_terminal_presentation_input", {
    request: {
      session_id: sessionId,
      presentation_id: presentationId,
      runtime_generation: snapshot.broker.runtimeGeneration,
      lease_epoch: snapshot.broker.leaseEpoch,
      input,
    },
  });
}

async function setDocumentVisibility(driver, value) {
  // The native test app deliberately has no permission to hide its own only
  // window. Drive the supported WebView fallback here; the focused hook unit
  // test separately verifies Tauri's native focus subscription.
  await driver.executeScript((nextValue) => {
    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      get: () => nextValue,
    });
    document.dispatchEvent(new Event("visibilitychange"));
  }, value);
}

function terminalText(snapshot) {
  return [
    ...(snapshot?.lines ?? []),
    ...(snapshot?.allLines ?? []),
    ...(snapshot?.renderer?.lines ?? []),
    ...(snapshot?.renderer?.allLines ?? []),
  ].join("\n");
}

async function captureRestoredScrollbackEvidence(driver, sessionId, presentationId) {
  const screenshotDir = process.env.WARDIAN_E2E_SCREENSHOT_DIR;
  if (!screenshotDir) {
    return null;
  }
  const scrolled = await driver.executeScript((sid, pid) => {
    const card = document.getElementById(`agent-card-${sid}`);
    const host = [...(card?.querySelectorAll('[data-testid="agent-terminal-host"]') ?? [])]
      .find((candidate) => candidate.getAttribute("data-terminal-presentation-id") === pid);
    if (!host) return false;
    host.dispatchEvent(new WheelEvent("wheel", { deltaY: -2400, bubbles: true, cancelable: true }));
    return true;
  }, sessionId, presentationId);
  assert.equal(scrolled, true, "Expected resumed terminal host for screenshot evidence");
  await waitFor("resumed terminal scrollback", 10_000, async () => {
    const snapshot = await readTerminalDebugSnapshot(driver, presentationId);
    return {
      ok: snapshot?.renderer?.viewportY < snapshot?.renderer?.baseY,
      viewportY: snapshot?.renderer?.viewportY ?? null,
      baseY: snapshot?.renderer?.baseY ?? null,
    };
  });
  fs.mkdirSync(screenshotDir, { recursive: true });
  const screenshotPath = path.join(screenshotDir, "authoritative-scrollback-restored.png");
  const card = await driver.findElement(By.id(`agent-card-${sessionId}`));
  fs.writeFileSync(screenshotPath, await card.takeScreenshot(true), "base64");
  return screenshotPath;
}

test(
  "background terminal recovery applies live output without refitting or resizing the renderer",
  { timeout: 240000 },
  async (t) => {
    const harness = await createNativeHarness();
    const previousTerminalDebug = process.env.VITE_WARDIAN_TERMINAL_DEBUG;
    try {
      if (!skipNativeBuild) {
        process.env.VITE_WARDIAN_TERMINAL_DEBUG = "1";
        ensureNativeAppBuilt(harness);
      }
      assert.ok(harness.appPath);
    } catch (error) {
      t.skip(String(error));
      return;
    } finally {
      if (previousTerminalDebug === undefined) {
        delete process.env.VITE_WARDIAN_TERMINAL_DEBUG;
      } else {
        process.env.VITE_WARDIAN_TERMINAL_DEBUG = previousTerminalDebug;
      }
    }

    prepareIsolatedHome(harness);
    const mockScript = createEchoMockScript();
    const previousMockScript = process.env.WARDIAN_MOCK_SCRIPT;
    process.env.WARDIAN_MOCK_SCRIPT = mockScript;
    let session;
    try {
      session = await startNativeSession(harness);
    } catch (error) {
      t.skip(String(error));
      return;
    } finally {
      if (previousMockScript === undefined) {
        delete process.env.WARDIAN_MOCK_SCRIPT;
      } else {
        process.env.WARDIAN_MOCK_SCRIPT = previousMockScript;
      }
    }

    t.after(async () => {
      await session.close();
      fs.rmSync(mockScript, { force: true });
    });

    const { driver } = session;
    await waitForAppShell(driver, 20_000);
    await driver.manage().window().setRect({ width: 1200, height: 820 });
    const agent = await invokeTauri(driver, "spawn_agent", {
      req: {
        sessionName: SESSION_NAME,
        agentClass: "TestClass",
        folder: harness.repoRoot,
        resumeSession: PROVIDER_SESSION_ID,
        isOff: false,
        configOverride: { provider: "mock" },
      },
    });
    const sessionId = agent.session_id;
    assert.notEqual(sessionId, PROVIDER_SESSION_ID);

    await openWorkbenchSurface(driver, "agents-overview");
    await driver.wait(until.elementLocated(By.id(`agent-card-${sessionId}`)), 20_000);
    if (!(await driver.executeScript(() => Boolean(window.__wardianTerminalDebug?.snapshot)))) {
      if (skipNativeBuild) {
        t.skip("Built Wardian app does not expose terminal debug snapshots; run without WARDIAN_NATIVE_SKIP_BUILD.");
        return;
      }
      assert.fail("Expected terminal debug snapshots in the native build");
    }
    const presentationId = await resolveAgentTerminalPresentationId(driver, sessionId);
    await activatePresentation(driver, sessionId, presentationId);
    await waitFor("mock terminal ready", 30_000, async () => {
      const snapshot = await readTerminalDebugSnapshot(driver, presentationId);
      return { ok: terminalText(snapshot).includes("background-resume-ready") };
    });

    const before = await readTerminalDebugSnapshot(driver, presentationId);
    assert.ok(before?.renderer, "Expected a resident terminal renderer before backgrounding");
    await setDocumentVisibility(driver, "hidden");

    const backgroundInput = Array.from(
      { length: 96 },
      (_, index) => `${BACKGROUND_PREFIX}_${String(index).padStart(3, "0")}`,
    ).join("\r") + `\r${BACKGROUND_MARKER}\r`;
    await sendTerminalInput(driver, sessionId, presentationId, backgroundInput);
    await new Promise((resolve) => setTimeout(resolve, 750));

    const whileHidden = await readTerminalDebugSnapshot(driver, presentationId);
    assert.equal(
      terminalText(whileHidden).includes(BACKGROUND_MARKER),
      false,
      "Background output must remain out of the renderer until foreground resynchronization",
    );

    await setDocumentVisibility(driver, "visible");
    const after = await waitFor("background output after foreground snapshot", 30_000, async () => {
      const snapshot = await readTerminalDebugSnapshot(driver, presentationId);
      return { ok: terminalText(snapshot).includes(BACKGROUND_MARKER), snapshot };
    });
    const resumed = after.snapshot;

    assert.equal(resumed.renderer.instanceId, before.renderer.instanceId, "Foregrounding must preserve renderer identity");
    assert.equal(resumed.fitCount, before.fitCount, "Foregrounding must not fit the renderer");
    assert.equal(resumed.resizeCount, before.resizeCount, "Foregrounding must not resize the renderer");
    // Snapshot replay can restore the headless parser to the broker's canonical
    // geometry. The reported PTY and xterm renderer are the native and visible
    // grids that must remain unchanged when the app returns to the foreground.
    assert.deepEqual(
      resumed.lastReportedSize,
      before.lastReportedSize,
      "Foregrounding must preserve reported native terminal geometry",
    );
    assert.deepEqual(
      { cols: resumed.renderer.cols, rows: resumed.renderer.rows },
      { cols: before.renderer.cols, rows: before.renderer.rows },
      "Foregrounding must preserve local renderer geometry",
    );
    assert.equal(
      resumed.broker?.ownerPresentationId,
      presentationId,
      "Foregrounding must preserve terminal ownership",
    );
    const screenshotPath = await captureRestoredScrollbackEvidence(
      driver,
      sessionId,
      presentationId,
    );
    if (screenshotPath) {
      t.diagnostic(`Restored scrollback screenshot: ${screenshotPath}`);
    }
  },
);
