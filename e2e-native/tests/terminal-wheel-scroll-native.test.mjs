import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
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
const PROVIDER_SESSION_ID = `e2e-terminal-wheel-${RUN_ID}`;
const SESSION_NAME = `E2E-Terminal-Wheel-${RUN_ID}`;
const POST_RESIZE_MARKER = `WARDIAN_POST_RESIZE_${RUN_ID}`;
// Replicates the stream shape captured live from Claude Code 2.1.173: banner
// rows, then a synchronized-output diff frame that cursor-addresses some rows
// and scrolls the rest in with newlines at the bottom row, hiding the cursor
// throughout and parking it mid-screen afterwards.
const ESC = String.fromCharCode(27);
function claudeLikeFrame() {
  const parts = [];
  for (let line = 1; line <= 9; line += 1) {
    parts.push(`banner-${line}\r\n`);
  }
  parts.push(`${ESC}[?2026h${ESC}[?25l${ESC}[38;2;0;0;0m${ESC}[10;1H●${ESC}[m${ESC}[1C1${ESC}[K`);
  for (let row = 11; row <= 24; row += 1) {
    parts.push(`${ESC}[${row};3H${row - 9}${ESC}[K`);
  }
  for (let value = 16; value <= 70; value += 1) {
    parts.push(`\r\n  wheel-${String(value).padStart(2, "0")}${ESC}[120C`);
  }
  parts.push(`\r\n${ESC}[124C\r\n  status-row${ESC}[K${ESC}[13;3H${ESC}[?25h${ESC}[?2026l`);
  return parts.join("");
}
const RAW_FRAME = claudeLikeFrame();
const TERMINAL_HOST_SELECTOR = '[data-testid="agent-terminal-host"]';

function skipGuidedTour(harness) {
  // The isolated native home starts on the guided tour, whose backdrop blocks
  // the Workbench interaction this terminal regression needs to exercise.
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
}

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

async function readBrokerSnapshot(driver, sessionId) {
  return await invokeTauri(driver, "request_terminal_snapshot", {
    request: { session_id: sessionId },
  });
}

async function focusAgentTerminal(driver, sessionId, presentationId) {
  const focused = await driver.wait(async () => await driver.executeScript((sid, pid) => {
      const host = [...document.querySelectorAll(
        '[data-zellij-singleton-viewport="true"] [data-testid="agent-terminal-host"]',
      )].find((candidate) => (
        candidate.getAttribute("data-terminal-session-id") === sid
        && candidate.getAttribute("data-terminal-presentation-id") === pid
      ));
      if (!host) return false;
      const helper = host.querySelector(".xterm-helper-textarea");
      if (!(helper instanceof HTMLTextAreaElement)) return false;
      helper.focus({ preventScroll: true });
      return document.activeElement === helper;
    }, sessionId, presentationId), 10_000);
  assert.equal(focused, true, `Expected terminal presentation ${presentationId} to receive focus`);
}

async function sendTerminalPresentationInput(driver, sessionId, presentationId, input) {
  const snapshot = await readTerminalDebugSnapshot(driver, presentationId);
  assert.equal(snapshot?.broker?.ownerPresentationId, presentationId,
    `Expected ${presentationId} to own terminal input`);
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

async function selectGridMode(driver) {
  const gridButton = await driver.wait(async () => {
    for (const button of await driver.findElements(By.css('[aria-label="Agents mode"] button'))) {
      if (await button.isDisplayed() && (await button.getText()).trim() === "Grid") {
        return button;
      }
    }
    return false;
  }, 20_000, "Timed out locating the Agents Grid mode control");
  await gridButton.click();
  await driver.wait(async () => await driver.executeScript(() => (
    document.querySelector('[data-testid="agent-grid"]')?.getAttribute("data-overview-mode") === "grid"
  )), 20_000, "Timed out selecting explicit Agents Grid mode");
}

function createScrollbackMockScript() {
  const scriptPath = path.join(os.tmpdir(), `wardian-wheel-mock-${RUN_ID}.cjs`);
  const script = `
"use strict";
const providerSessionId = process.env.WARDIAN_MOCK_SESSION_ID;
if (!providerSessionId) {
  throw new Error("WARDIAN_MOCK_SESSION_ID is required");
}
const init = JSON.stringify({
  type: "init",
  session_id: providerSessionId,
  timestamp: new Date().toISOString(),
}) + "\\n";
process.stdout.write(init);
process.stdout.write(${JSON.stringify(RAW_FRAME)});
// Keep changing one row at a realistic TUI repaint cadence. The singleton
// renderer must remain responsive without multiplying this work per card.
const esc = String.fromCharCode(27);
const spinnerGlyphs = ["*", "+", "x", "."];
let spin = 0;
const timer = setInterval(() => {
  spin += 1;
  const glyph = spinnerGlyphs[spin % spinnerGlyphs.length];
  const frame =
    esc + "[?2026h" + esc + "[?25l" + esc + "[38;2;215;119;87m" + esc + "[10;1H" + glyph +
    esc + "[38;2;102;102;102m" + esc + "[22C(" + spin + "s)" + esc + "[K" +
    esc + "[13;3H" + esc + "[?25h" + esc + "[m" + esc + "[?2026l";
  process.stdout.write(frame.repeat(2));
}, 25);
let pendingInput = "";
process.stdin.on("data", (chunk) => {
  pendingInput += chunk.toString();
  if (pendingInput.includes(${JSON.stringify(POST_RESIZE_MARKER)})) {
    clearInterval(timer);
    process.stdout.write("\\r\\n" + ${JSON.stringify(POST_RESIZE_MARKER)} + "\\r\\n");
  }
});
process.stdin.resume();
`;
  fs.writeFileSync(scriptPath, script, "utf8");
  return scriptPath;
}

async function waitForCanonicalFrame(
  driver,
  sessionId,
  presentationId,
  expectedText,
) {
  const startedAt = Date.now();
  let last = null;
  while (Date.now() - startedAt < 30000) {
    const broker = await readBrokerSnapshot(driver, sessionId);
    const renderer = await readTerminalDebugSnapshot(driver, presentationId);
    const brokerText = [...broker.scrollback, broker.visible_grid].join("\n");
    last = { broker, renderer, brokerText };
    if (
      broker.geometry?.cols === 120
      && broker.geometry?.rows === 40
      && brokerText.includes(expectedText)
      && renderer?.renderer?.ready
    ) {
      return last;
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`Timed out waiting for canonical Zellij frame: ${JSON.stringify(last)}`);
}

test("desktop singleton stays coherent through bursty Zellij frames and viewport resize", { timeout: 180000 }, async (t) => {
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
  skipGuidedTour(harness);

  const mockScript = createScrollbackMockScript();
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
  await waitForAppShell(driver, 20000);
  // Mirror the real-provider rendering audit environment: small terminal font,
  // two-column grid with a fixed row height, and a second (filler) agent.
  await driver.executeScript(() => {
    localStorage.setItem(
      "wardian-settings",
      JSON.stringify({
        state: { theme: "dark", terminalFontSize: 10, terminalFontFamily: "", autoPatchGemini: false },
        version: 0,
      }),
    );
    localStorage.setItem(
      "wardian-layout",
      JSON.stringify({
        state: {
          layout: { column_tracks: [0.5, 0.5], row_height: 420 },
          leftSidebarWidth: 260,
          rightSidebarWidth: 240,
          userTerminalOpen: false,
          userTerminalHeight: 360,
          gridStacked: false,
          previousColumnTracks: null,
        },
        version: 0,
      }),
    );
    location.reload();
  });
  await waitForAppShell(driver, 20000);
  await driver.manage().window().setRect({ width: 1920, height: 1080 });

  const filler = await invokeTauri(driver, "spawn_agent", {
    req: {
      sessionName: `${SESSION_NAME}-filler`,
      agentClass: "TestClass",
      folder: harness.repoRoot,
      resumeSession: `${PROVIDER_SESSION_ID}-filler`,
      isOff: false,
      configOverride: { provider: "mock" },
    },
  });
  assert.ok(filler.session_id, "Expected filler agent to spawn");

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
  assert.match(
    sessionId,
    /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i,
  );
  assert.notEqual(sessionId, PROVIDER_SESSION_ID);

  await openWorkbenchSurface(driver, "agents-overview");
  // The test's fixed two-column layout is an explicit Grid contract. Leaving
  // the surface in Auto can intentionally switch to one presentation when the
  // window narrows, which would test responsive selection instead of wheel IO.
  await selectGridMode(driver);
  const card = await driver.wait(
    until.elementLocated(By.id(`agent-card-${sessionId}`)),
    20000,
  );
  await driver.wait(until.elementIsVisible(card), 20000);
  await driver.executeScript((sid) => {
    const preview = document.querySelector(
      `[data-zellij-presentation="preview"][data-zellij-agent-id="${CSS.escape(sid)}"]`,
    );
    preview?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
  }, sessionId);
  await driver.wait(until.elementLocated(By.css(TERMINAL_HOST_SELECTOR)), 20000);
  if (!(await driver.executeScript(() => Boolean(window.__wardianTerminalDebug?.snapshot)))) {
    if (skipNativeBuild) {
      t.skip("Built Wardian app does not expose terminal debug snapshots; run without WARDIAN_NATIVE_SKIP_BUILD.");
      return;
    }
    assert.fail("Expected terminal debug snapshots in the native build");
  }
  const presentationId = await resolveAgentTerminalPresentationId(driver, sessionId);
  await focusAgentTerminal(driver, sessionId, presentationId);

  const initialFrame = await waitForCanonicalFrame(
    driver,
    sessionId,
    presentationId,
    "wheel-70",
  );
  const beforeSnapshot = initialFrame.renderer;
  assert.equal(
    beforeSnapshot?.broker?.ownerPresentationId,
    presentationId,
    "Expected the visible desktop presentation to own the terminal before resize",
  );
  const rendererInventory = await driver.executeScript(() => ({
    singletonViewports: document.querySelectorAll('[data-zellij-singleton-viewport="true"]').length,
    singletonXterms: document.querySelectorAll('[data-zellij-singleton-viewport="true"] .xterm').length,
    previewXterms: document.querySelectorAll('[data-zellij-presentation="preview"] .xterm').length,
  }));
  assert.deepEqual(rendererInventory, {
    singletonViewports: 1,
    singletonXterms: 1,
    previewXterms: 0,
  });
  const rendererInstanceId = beforeSnapshot?.renderer?.instanceId;
  const webglAttemptCount = beforeSnapshot?.renderer?.webglAttemptCount;
  assert.ok(rendererInstanceId, "Expected the singleton renderer identity");
  assert.equal(webglAttemptCount, 1, "Expected one WebGL attempt for the singleton lifetime");

  // Shrinking the grid may fit the one local renderer, but it must not resize
  // Zellij's canonical frame or create another xterm/WebGL context.
  await driver.manage().window().setRect({ width: 980, height: 980 });
  await driver.wait(async () => await driver.executeScript((sid, pid) => {
    const host = [...document.querySelectorAll(
      '[data-zellij-singleton-viewport="true"] [data-testid="agent-terminal-host"]',
    )].find((candidate) => (
      candidate.getAttribute("data-terminal-session-id") === sid
      && candidate.getAttribute("data-terminal-presentation-id") === pid
    ));
    return Boolean(host?.querySelector(".xterm"));
  }, sessionId, presentationId), 20_000,
  "Timed out waiting for the focused terminal presentation after the narrow resize");
  const narrowFrame = await waitForCanonicalFrame(
    driver,
    sessionId,
    presentationId,
    "wheel-70",
  );
  assert.equal(narrowFrame.renderer?.renderer?.instanceId, rendererInstanceId);
  assert.equal(narrowFrame.renderer?.renderer?.webglAttemptCount, webglAttemptCount);
  assert.equal(narrowFrame.broker.geometry.cols, 120);
  assert.equal(narrowFrame.broker.geometry.rows, 40);

  await driver.manage().window().setRect({ width: 1920, height: 1080 });
  await sendTerminalPresentationInput(
    driver,
    sessionId,
    presentationId,
    `${POST_RESIZE_MARKER}\r`,
  );
  const finalFrame = await waitForCanonicalFrame(
    driver,
    sessionId,
    presentationId,
    POST_RESIZE_MARKER,
  );
  assert.equal(finalFrame.renderer?.renderer?.instanceId, rendererInstanceId);
  assert.equal(finalFrame.renderer?.renderer?.webglAttemptCount, webglAttemptCount);
});
