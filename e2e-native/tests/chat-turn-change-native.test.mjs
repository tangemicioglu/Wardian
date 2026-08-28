// @tier nightly — Runs on the nightly schedule; too slow or too broad for every pull request.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { By } from "selenium-webdriver";

import {
  createNativeHarness,
  ensureNativeAppBuilt,
  prepareIsolatedHome,
  startNativeSession,
  waitForAppShell,
} from "../lib/harness.mjs";

/**
 * Captures the chat transcript's file-change surface against the real app.
 *
 * This has to be a native test: the rows are driven by
 * `load_agent_chat_transcript`, which reads a provider log through Tauri IPC.
 * Browser E2E cannot seed one. The mock provider's `file_changes` scenario
 * supplies a deterministic turn — read, edit, write, shell — so the structured
 * edit panel, the work log, and the per-turn change card all render without a
 * provider subscription.
 */

const skipNativeBuild = process.env.WARDIAN_NATIVE_SKIP_BUILD === "1";
const RUN_ID = `${process.pid}-${Date.now()}`;
const SCREENSHOT_DATE = "2026-08-08";

function withEnv(t, key, value) {
  const previous = process.env[key];
  process.env[key] = value;
  t.after(() => {
    if (previous === undefined) delete process.env[key];
    else process.env[key] = previous;
  });
}

/**
 * Spawns the agent and returns the id Wardian assigned it.
 *
 * `spawn_agent` mints its own session id; `resumeSession` is the provider-side
 * identity and is not it. Every later call has to use the returned one.
 */
async function spawnMockAgent(driver, workspace) {
  const result = await driver.executeAsyncScript((folder, done) => {
    window.__TAURI_INTERNALS__.invoke("spawn_agent", {
      req: {
        sessionName: "Chat-Changes",
        agentClass: "Coder",
        folder,
        isOff: false,
        configOverride: { provider: "mock" },
      },
    }).then(
      (agent) => done({ ok: true, agent }),
      (error) => done({ ok: false, error: String(error) }),
    );
  }, workspace);
  assert.equal(result.ok, true, `spawn_agent failed: ${result.error}`);
  const sessionId = result.agent?.session_id ?? result.agent?.sessionId;
  assert.ok(sessionId, `spawn_agent returned no session id: ${JSON.stringify(result.agent)}`);
  return sessionId;
}

/** Polls the real IPC command until the mock's tool calls have been normalized. */
async function waitForTranscript(driver, sessionId, harness) {
  try {
    return await driver.wait(async () => {
      const events = await driver.executeAsyncScript((id, done) => {
        window.__TAURI_INTERNALS__.invoke("load_agent_chat_transcript", { sessionId: id }).then(
          (rows) => done(rows),
          () => done(null),
        );
      }, sessionId);
      if (!Array.isArray(events)) return false;
      // Waits for the second turn so both row shapes are on screen: a
      // collapsed work log and a standalone edit carrying its panel.
      const edits = events.filter((event) => event?.metadata?.tool_name === "Edit").length;
      const users = events.filter((event) => event?.kind === "message" && event?.role === "user").length;
      return edits >= 2 && users >= 2 ? events : false;
    }, 90_000, "mock transcript never produced both turns of the file_changes scenario");
  } catch (error) {
    // The provider log is the whole input to normalization, so say whether it
    // was written at all rather than leaving a bare timeout.
    const log = path.join(harness.isolatedHome, "agents", sessionId, "mock-transcript.jsonl");
    const detail = fs.existsSync(log)
      ? `provider log has ${fs.readFileSync(log, "utf8").trim().split("\n").length} lines`
      : `provider log missing at ${log}`;
    error.message = `${error.message} (${detail})`;
    throw error;
  }
}

async function switchCardToChat(driver, sessionId) {
  const switched = await driver.executeScript((id) => {
    const card = document.getElementById(`agent-card-${id}`);
    if (!card) return "no-card";
    const toggle = Array.from(card.querySelectorAll("button")).find(
      (button) => (button.getAttribute("title") || "").startsWith("Switch to Chat"),
    );
    if (!toggle) return "already-chat";
    toggle.click();
    return "switched";
  }, sessionId);
  assert.notEqual(switched, "no-card", `agent card for ${sessionId} never mounted`);
}

test("chat transcript renders the per-turn change surface", { timeout: 600_000 }, async (t) => {
  let harness;
  try {
    harness = await createNativeHarness();
    if (!skipNativeBuild) ensureNativeAppBuilt(harness);
    assert.ok(harness.appPath);
  } catch (error) {
    t.skip(String(error));
    return;
  }

  // The scenario is read from the environment the app inherits when the
  // provider config does not pin one.
  withEnv(t, "WARDIAN_MOCK_SCENARIO", "file_changes");
  withEnv(t, "WARDIAN_MOCK_DELAY_MS", "40");

  prepareIsolatedHome(harness);
  const workspace = path.join(harness.isolatedHome, `chat-workspace-${RUN_ID}`);
  fs.mkdirSync(workspace, { recursive: true });

  const session = await startNativeSession(harness);
  t.after(async () => { await session?.close(); });
  await waitForAppShell(session.driver, 30_000);

  const sessionId = await spawnMockAgent(session.driver, workspace);
  const events = await waitForTranscript(session.driver, sessionId, harness);

  // The normalization contract the rendering depends on, asserted before any
  // pixels: a screenshot of an empty card would pass a visual check silently.
  const edit = events.find((event) => event?.metadata?.tool_name === "Edit");
  assert.equal(edit.metadata.tool_input.old_string, "const WORK_GROUP_MIN_ENTRIES = 4;");
  assert.equal(edit.metadata.files_written[0], "src/features/chat/chatPresentation.ts");
  const write = events.find((event) => event?.metadata?.tool_name === "Write");
  assert.ok(write.metadata.tool_input.content.includes("Mock spec"), "Write lost its content");

  await switchCardToChat(session.driver, sessionId);

  const transcript = await session.driver.wait(async () => {
    const rows = await session.driver.findElements(By.css('[data-testid="agent-chat-transcript"]'));
    for (const row of rows) if (await row.isDisplayed()) return row;
    return false;
  }, 30_000, "chat transcript never became visible");

  const card = await session.driver.wait(async () => {
    const cards = await session.driver.findElements(By.css('[data-testid="turn-change-card"]'));
    for (const candidate of cards) if (await candidate.isDisplayed()) return candidate;
    return false;
  }, 30_000, "turn change card never rendered");

  const cardText = await card.getText();
  assert.match(cardText, /changed files?/);
  assert.match(cardText, /chatPresentation\.ts/);
  // The write must not claim creation: the mock's tool input proves content,
  // never that the file was new.
  assert.doesNotMatch(cardText, /Created/);

  // The first turn's four adjacent tool calls collapse into a work log, whose
  // entries are one-liners by design.
  const groups = await session.driver.findElements(By.css('[data-testid="chat-work-group"]'));
  assert.ok(groups.length >= 1, "the multi-tool turn did not collapse into a work log");

  const panel = await session.driver.wait(async () => {
    const panels = await session.driver.findElements(By.css('[data-testid="tool-structured-edit"]'));
    for (const candidate of panels) if (await candidate.isDisplayed()) return candidate;
    return false;
  }, 30_000, "structured edit panel never rendered");
  assert.match(await panel.getText(), /Before\/after|New contents/);

  const screenshotDir = path.join(
    harness.repoRoot,
    "e2e",
    "screenshots",
    "chat-turn-change",
    SCREENSHOT_DATE,
  );
  fs.mkdirSync(screenshotDir, { recursive: true });
  fs.writeFileSync(
    path.join(screenshotDir, "turn-change-card.png"),
    await session.driver.takeScreenshot(),
    "base64",
  );

  await session.driver.executeScript((element) => element.scrollIntoView({ block: "center" }), transcript);
  fs.writeFileSync(
    path.join(screenshotDir, "chat-transcript.png"),
    await session.driver.takeScreenshot(),
    "base64",
  );
});
