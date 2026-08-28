// @tier nightly — Runs on the nightly schedule; too slow or too broad for every pull request.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import {
  createNativeHarness,
  ensureNativeAppBuilt,
  invokeTauri,
  prepareIsolatedHome,
  startNativeSession,
  waitForAppShell,
} from "../lib/harness.mjs";

const skipNativeBuild = process.env.WARDIAN_NATIVE_SKIP_BUILD === "1";
const runId = `${process.pid}-${Date.now()}`;
const sessionName = `Fresh-Claude-Log-${runId}`;
const freshMarker = "FRESH_CLAUDE_LOG_MARKER";
const staleMarker = "STALE_CLAUDE_LOG_MARKER";
const testArgument = "wardian-fresh-log-test";

function withEnv(t, key, value) {
  const previous = process.env[key];
  process.env[key] = value;
  t.after(() => {
    if (previous === undefined) delete process.env[key];
    else process.env[key] = previous;
  });
}

function claudeProjectDir(workspace) {
  return path.join(os.homedir(), ".claude", "projects", workspace.replace(/[:\\/.]/g, "-"));
}

function seedClaudeShim(harness) {
  const binDir = path.join(harness.isolatedHome, "fresh-claude-log-bin");
  fs.mkdirSync(binDir, { recursive: true });
  const source = `
const fs = require("node:fs");
const path = require("node:path");
const argv = process.argv.slice(2);
const targetArgument = process.env.WARDIAN_FRESH_CLAUDE_LOG_TEST_ARGUMENT;
if (argv.includes(targetArgument)) {
  const logDir = process.env.WARDIAN_FRESH_CLAUDE_LOG_DIR;
  const sessionName = process.env.WARDIAN_FRESH_CLAUDE_LOG_SESSION_NAME;
  const wardianSessionId = process.env.WARDIAN_SESSION_ID;
  const providerSessionId = "fresh-provider-" + wardianSessionId;
  fs.mkdirSync(logDir, { recursive: true });
  const lines = [
    { type: "custom-title", customTitle: sessionName, sessionId: providerSessionId },
    { type: "user", message: { role: "user", content: ${JSON.stringify(freshMarker)} } },
    { type: "assistant", message: { id: "fresh-answer", role: "assistant", content: [{ type: "text", text: ${JSON.stringify(freshMarker)} }], stop_reason: "end_turn" } },
  ];
  fs.writeFileSync(
    path.join(logDir, providerSessionId + ".jsonl"),
    lines.map((line) => JSON.stringify(line)).join("\\n") + "\\n",
    "utf8",
  );
}
process.stdout.write("Claude Code\\n❯ Ready\\n");
setInterval(() => {}, 1000);
`;
  const script = path.join(binDir, "claude-fresh-log-recorder.cjs");
  fs.writeFileSync(script, source, "utf8");
  if (process.platform === "win32") {
    fs.writeFileSync(
      path.join(binDir, "claude.cmd"),
      `@ECHO off\r\n"${process.execPath}" "%~dp0claude-fresh-log-recorder.cjs" %*\r\n`,
      "utf8",
    );
  } else {
    fs.writeFileSync(path.join(binDir, "claude"), `#!/bin/sh\nexec node "${script}" "$@"\n`, {
      encoding: "utf8",
      mode: 0o755,
    });
  }
  return binDir;
}

async function waitForTranscript(driver, agentSessionId) {
  return await driver.wait(async () => {
    const events = await invokeTauri(driver, "load_agent_chat_transcript", {
      sessionId: agentSessionId,
    });
    return Array.isArray(events) && events.some((event) => event?.text === freshMarker)
      ? events
      : false;
  }, 20_000, "fresh Claude log never reached the chat transcript");
}

test("fresh Claude resume refreshes the active provider log", { timeout: 120_000 }, async (t) => {
  const harness = await createNativeHarness();
  try {
    if (!skipNativeBuild) ensureNativeAppBuilt(harness);
    assert.ok(harness.appPath);
  } catch (error) {
    t.skip(String(error));
    return;
  }

  prepareIsolatedHome(harness);
  const logDir = claudeProjectDir(harness.repoRoot);
  const staleProviderSession = `stale-provider-${runId}`;
  const staleLogPath = path.join(logDir, `${staleProviderSession}.jsonl`);
  let freshLogPath = null;
  const binDir = seedClaudeShim(harness);
  withEnv(t, "WARDIAN_FRESH_CLAUDE_LOG_DIR", logDir);
  withEnv(t, "WARDIAN_FRESH_CLAUDE_LOG_SESSION_NAME", sessionName);
  withEnv(t, "WARDIAN_FRESH_CLAUDE_LOG_TEST_ARGUMENT", testArgument);

  const previousPath = process.env.PATH;
  const previousPathExt = process.env.PATHEXT;
  process.env.PATH = [binDir, previousPath].filter(Boolean).join(path.delimiter);
  if (process.platform === "win32") process.env.PATHEXT = ".CMD;.EXE;.BAT";

  let session;
  try {
    session = await startNativeSession(harness);
  } finally {
    if (previousPath === undefined) delete process.env.PATH;
    else process.env.PATH = previousPath;
    if (previousPathExt === undefined) delete process.env.PATHEXT;
    else process.env.PATHEXT = previousPathExt;
  }
  t.after(async () => {
    await session?.close();
    fs.rmSync(staleLogPath, { force: true });
    if (freshLogPath) fs.rmSync(freshLogPath, { force: true });
  });

  await waitForAppShell(session.driver, 20_000);
  fs.mkdirSync(logDir, { recursive: true });
  fs.writeFileSync(
    staleLogPath,
    [
      JSON.stringify({ type: "custom-title", customTitle: sessionName, sessionId: staleProviderSession }),
      JSON.stringify({
        type: "assistant",
        message: {
          id: "stale-answer",
          role: "assistant",
          content: [{ type: "text", text: staleMarker }],
          stop_reason: "end_turn",
        },
      }),
    ].join("\n") + "\n",
    "utf8",
  );

  const agent = await invokeTauri(session.driver, "spawn_agent", {
    req: {
      sessionName,
      agentClass: "TestClass",
      folder: harness.repoRoot,
      resumeSession: null,
      isOff: true,
      configOverride: { provider: "claude", provider_config: { type: "claude" } },
    },
  });
  t.after(async () => {
    try {
      await invokeTauri(session.driver, "kill_agent", { sessionId: agent.session_id });
    } catch {
      // The app may already have exited during cleanup.
    }
  });

  const existing = (await invokeTauri(session.driver, "list_agents"))
    .find((entry) => entry.session_id === agent.session_id);
  assert.ok(existing, "fresh Claude agent missing after spawn");
  await invokeTauri(session.driver, "update_agent_config", {
    newConfig: {
      ...existing,
      session_persistence: "fresh",
      resume_session: staleProviderSession,
      provider_config: { type: "claude", append_system_prompt: testArgument },
    },
  });

  freshLogPath = path.join(logDir, `fresh-provider-${agent.session_id}.jsonl`);
  await invokeTauri(session.driver, "resume_agent", { sessionId: agent.session_id });
  const transcript = await waitForTranscript(session.driver, agent.session_id);
  const refreshed = (await invokeTauri(session.driver, "list_agents"))
    .find((entry) => entry.session_id === agent.session_id);

  assert.equal(refreshed.resume_session, `fresh-provider-${agent.session_id}`);
  assert.equal(transcript.some((event) => event?.text === staleMarker), false);
  assert.equal(transcript.some((event) => event?.text === freshMarker), true);
});
