import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { By } from "selenium-webdriver";

import {
  createNativeHarness,
  ensureNativeAppBuilt,
  invokeTauri,
  prepareIsolatedHome,
  startNativeSession,
  waitForAppShell,
} from "../lib/harness.mjs";

const runRealLuna = process.env.WARDIAN_E2E_REAL_CODEX_MEMORY === "1";
const skipNativeBuild = process.env.WARDIAN_NATIVE_SKIP_BUILD === "1";
const RUN_ID = `${process.pid}-${Date.now()}`;
const SCREENSHOT_DATE = "2026-08-23";

function commandName(name) {
  return process.platform === "win32" ? `${name}.exe` : name;
}

function buildCli(harness) {
  const result = spawnSync("cargo", ["build", "-p", "wardian-cli", "--bin", "wardian-cli"], {
    cwd: harness.repoRoot,
    encoding: "utf8",
  });
  assert.equal(
    result.status,
    0,
    `cargo build -p wardian-cli failed\nstdout:\n${result.stdout}\nstderr:\n${result.stderr}`,
  );
  return path.join(harness.repoRoot, "target", "debug", commandName("wardian-cli"));
}

function runCli(cliPath, harness, args) {
  const env = { ...process.env, WARDIAN_HOME: harness.isolatedHome };
  delete env.WARDIAN_SESSION_ID;
  return spawnSync(cliPath, args, {
    cwd: harness.repoRoot,
    env,
    encoding: "utf8",
    timeout: 360_000,
  });
}

function runCliOk(cliPath, harness, args) {
  const result = runCli(cliPath, harness, args);
  assert.equal(
    result.status,
    0,
    `wardian ${args.join(" ")} failed\nstdout:\n${result.stdout}\nstderr:\n${result.stderr}`,
  );
  return result;
}

async function enableCodexWorkspaceTrust(harness) {
  const settingsDir = path.join(harness.isolatedHome, "settings");
  fs.mkdirSync(settingsDir, { recursive: true });
  fs.writeFileSync(
    path.join(settingsDir, "shell.json"),
    JSON.stringify({
      schema_version: 2,
      overrides: { codex_runtime_policy: { trust_workspaces: true } },
    }),
  );
}

function runMemoryProbe(cliPath, harness, workflowPath, agentId, expected, forbidden) {
  const execution = JSON.parse(runCliOk(cliPath, harness, [
    "workflow", "exec", workflowPath,
    "--executor", "live",
    "--bind", `verifier=${agentId}`,
  ]).stdout);
  assert.equal(execution.ok, true);
  assert.equal(execution.status, "started");
  const startedAt = Date.now();
  let shown = null;
  while (Date.now() - startedAt < 360_000) {
    const result = runCli(cliPath, harness, [
      "workflow", "run-show", "memory-native-probe", execution.run_id,
    ]);
    if (result.status === 0) {
      shown = JSON.parse(result.stdout);
      if (["completed", "failed", "awaiting_approval"].includes(shown.state?.status)) break;
    }
    Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 500);
  }
  assert.equal(shown?.state?.status, "completed", JSON.stringify(shown));
  const evidence = JSON.stringify(shown);
  assert.match(evidence, new RegExp(expected));
  assert.doesNotMatch(evidence, new RegExp(forbidden));
}

async function waitForLoadedMemory(driver, sessionId, expectedText, minimumCount = 1) {
  return driver.wait(async () => {
    const events = await invokeTauri(driver, "load_agent_chat_transcript", { sessionId });
    if (!Array.isArray(events)) return false;
    const loaded = events.filter(
      (event) => event?.kind === "memory" && event?.metadata?.memory_action === "loaded",
    );
    if (loaded.length < minimumCount) return false;
    const latest = loaded.at(-1);
    return latest?.text?.includes(expectedText) ? { events, loaded, latest } : false;
  }, 90_000, `memory brief containing ${expectedText} never reached the chat transcript`);
}

async function switchCardToChat(driver, sessionId) {
  await driver.wait(async () => {
    const switched = await driver.executeScript((id) => {
      const card = document.getElementById(`agent-card-${id}`);
      if (!card) return false;
      const toggle = Array.from(card.querySelectorAll("button")).find(
        (button) => (button.getAttribute("title") || "").startsWith("Switch to Chat"),
      );
      if (toggle) toggle.click();
      return true;
    }, sessionId);
    return switched;
  }, 30_000, `agent card for ${sessionId} never mounted`);
}

test("temporary GPT-5.6-Luna agents receive isolated startup memory and resume deltas", { timeout: 900_000 }, async (t) => {
  if (!runRealLuna) {
    t.skip("Set WARDIAN_E2E_REAL_CODEX_MEMORY=1 to run the Luna memory acceptance test.");
    return;
  }

  const harness = await createNativeHarness();
  if (!skipNativeBuild) ensureNativeAppBuilt(harness);
  prepareIsolatedHome(harness);
  await enableCodexWorkspaceTrust(harness);
  const workspace = path.join(harness.isolatedHome, `luna-memory-workspace-${RUN_ID}`);
  fs.mkdirSync(workspace, { recursive: true });
  const cliPath = buildCli(harness);
  const session = await startNativeSession(harness);
  t.after(async () => { await session.close(); });
  await waitForAppShell(session.driver, 30_000);

  const agentAName = `Memory-Luna-A-${RUN_ID}`;
  const agentBName = `Memory-Luna-B-${RUN_ID}`;
  const spawn = (name) => JSON.parse(runCliOk(cliPath, harness, [
    "agent", "spawn",
    "--provider", "codex",
    "--class", "Coder",
    "--name", name,
    "--workspace", workspace,
    "--model", "gpt-5.6-luna",
    "--reasoning-effort", "low",
  ]).stdout).agent;
  const agentA = spawn(agentAName);
  const agentB = spawn(agentBName);
  const agentAId = agentA.uuid ?? agentA.session_id;
  const agentBId = agentB.uuid ?? agentB.session_id;
  assert.ok(agentAId && agentBId, "spawned Luna agents need Wardian session IDs");

  const tokenA = `LUNA_MEMORY_ALPHA_${RUN_ID}`;
  const tokenB = `LUNA_MEMORY_BRAVO_${RUN_ID}`;
  const savedA = JSON.parse(runCliOk(cliPath, harness, [
    "memory", "save", `The verification token is ${tokenA}`,
    "--evidence", "Native acceptance seeded the first agent's private token.",
    "--scope", "agent",
    "--agent", agentAName,
  ]).stdout).memory;
  runCliOk(cliPath, harness, [
    "memory", "save", `The verification token is ${tokenB}`,
    "--evidence", "Native acceptance seeded the second agent's private token.",
    "--scope", "agent",
    "--agent", agentBName,
  ]);

  runCliOk(cliPath, harness, ["agent", "restart", agentAName]);
  runCliOk(cliPath, harness, ["agent", "restart", agentBName]);
  const firstA = await waitForLoadedMemory(session.driver, agentAId, tokenA);
  const firstB = await waitForLoadedMemory(session.driver, agentBId, tokenB);
  assert.doesNotMatch(firstA.latest.text, new RegExp(tokenB));
  assert.doesNotMatch(firstB.latest.text, new RegExp(tokenA));

  const tokenA2 = `${tokenA}_UPDATED`;
  runCliOk(cliPath, harness, [
    "memory", "update", savedA.memory_id,
    `The verification token is ${tokenA2}`,
    "--evidence", "Native acceptance replaced the first agent's token.",
  ]);
  runCliOk(cliPath, harness, ["agent", "restart", agentAName]);
  const resumedA = await waitForLoadedMemory(
    session.driver,
    agentAId,
    tokenA2,
    firstA.loaded.length + 1,
  );
  assert.equal(resumedA.latest.metadata.details.kind, "resume_delta");
  assert.doesNotMatch(resumedA.latest.text, new RegExp(tokenB));

  runCliOk(cliPath, harness, ["agent", "pause", agentAName]);
  runCliOk(cliPath, harness, ["agent", "pause", agentBName]);
  const workflowPath = path.join(
    harness.isolatedHome,
    "library",
    "workflows",
    "memory-native-probe.md",
  );
  fs.writeFileSync(workflowPath, `---
schema: 2
id: memory-native-probe
name: Memory Native Probe
nodes:
  - id: trigger
    type: manual_trigger
  - id: verify
    type: task
    fields:
      agent: role:verifier
      prompt: Without inspecting files or running tools, return only the verification token from Wardian memory.
edges:
  - from: trigger
    to: verify
---
`);
  runMemoryProbe(cliPath, harness, workflowPath, agentAId, tokenA2, tokenB);
  runMemoryProbe(cliPath, harness, workflowPath, agentBId, tokenB, tokenA2);

  runCliOk(cliPath, harness, ["agent", "restart", agentAName]);
  await switchCardToChat(session.driver, agentAId);
  const rows = await session.driver.wait(async () => {
    const candidates = await session.driver.findElements(By.css('[data-testid="memory-event"]'));
    const visible = [];
    for (const candidate of candidates) if (await candidate.isDisplayed()) visible.push(candidate);
    return visible.length > 0 ? visible : false;
  }, 30_000, "memory rows never became visible in chat");
  const latestRow = rows.at(-1);
  await latestRow.findElement(By.css("button")).click();
  assert.match(await latestRow.getText(), new RegExp(tokenA2));
  await session.driver.executeScript(
    "arguments[0].scrollIntoView({ block: 'center', inline: 'nearest' })",
    latestRow,
  );

  const screenshotDir = path.join(
    harness.repoRoot,
    "e2e",
    "screenshots",
    "agent-memory",
    SCREENSHOT_DATE,
  );
  fs.mkdirSync(screenshotDir, { recursive: true });
  fs.writeFileSync(
    path.join(screenshotDir, "memory-loaded-expanded.png"),
    await latestRow.takeScreenshot(true),
    "base64",
  );
});
