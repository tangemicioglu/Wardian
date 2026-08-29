// @tier manual — Needs a real provider or a logged-in CLI. Run it deliberately.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";

import {
  createNativeHarness,
  ensureNativeAppBuilt,
  prepareIsolatedHome,
  startNativeSession,
  waitForAppShell,
} from "../lib/harness.mjs";

const PROVIDERS = ["codex", "claude", "opencode", "antigravity", "pi"];
const runRealHeadlessProviders = process.env.WARDIAN_E2E_REAL_HEADLESS_PROVIDERS === "1";
const allowPartialProviders = process.env.WARDIAN_E2E_HEADLESS_ALLOW_PARTIAL === "1";
const workspacePath = process.env.WARDIAN_E2E_REAL_WORKSPACE || process.cwd();
const skipNativeBuild = process.env.WARDIAN_NATIVE_SKIP_BUILD === "1";

function parseCommaList(value, fallback) {
  const requested = String(value ?? "")
    .split(",")
    .map((item) => item.trim().toLowerCase())
    .filter(Boolean);

  return requested.length > 0 ? requested : [...fallback];
}

function selectedProviders() {
  return parseCommaList(process.env.WARDIAN_E2E_HEADLESS_PROVIDERS, PROVIDERS);
}

function nodeOutputText(output) {
  if (typeof output === "string") {
    return output;
  }
  if (typeof output?.text === "string") {
    return output.text;
  }
  if (typeof output?.response === "string") {
    return output.response;
  }
  return JSON.stringify(output ?? {});
}

async function readDebugTail(harness) {
  try {
    const logPath = path.join(harness.isolatedHome, "wardian_debug.log");
    const content = await fs.readFile(logPath, "utf8");
    return content.split(/\r?\n/).filter(Boolean).slice(-100).join("\n");
  } catch {
    return "No wardian_debug.log found.";
  }
}

async function seedWorkflow(harness, { workflowId, marker }) {
  const workflowsDir = path.join(harness.isolatedHome, "library", "workflows");
  const workflowPath = path.join(workflowsDir, `${workflowId}.md`);
  await fs.mkdir(workflowsDir, { recursive: true });
  await fs.writeFile(
    workflowPath,
    `---
schema: 2
id: ${workflowId}
name: Headless Provider Workflow
nodes:
  - id: trigger
    type: manual_trigger
  - id: provider-turn
    type: task
    fields:
      agent: role:worker
      prompt: Return exactly ${marker} and no other text.
edges:
  - from: trigger
    to: provider-turn
---

# Headless Provider Workflow
`,
    "utf8",
  );
  return workflowPath;
}

async function invokeTemporaryProviderWorkflow(driver, { workflowPath, provider, workspace }) {
  const result = await driver.executeAsyncScript((payload, done) => {
    window.__TAURI_INTERNALS__.invoke("workflow_run", payload).then(
      (value) => done({ ok: true, value }),
      (error) => done({ ok: false, error: String(error) }),
    );
  }, {
    path: workflowPath,
    provider,
    workspace,
    input: {},
    assignments: {
      worker: {
        target_type: "temporary_provider",
        provider,
        workspace,
      },
    },
  });

  assert.equal(result.ok, true, `workflow_run failed: ${result.error}`);
  assert.equal(result.value?.ok, true, `workflow_run did not start: ${JSON.stringify(result.value)}`);
  assert.equal(result.value?.status, "started");
  assert.equal(typeof result.value?.run_dir, "string");
  return result.value;
}

async function readCompletedWorkflow(runDir, timeoutMs = 180000) {
  const statePath = path.join(runDir, "state.json");
  const eventsPath = path.join(runDir, "events.jsonl");
  const startedAt = Date.now();
  let latestState = null;

  while (Date.now() - startedAt < timeoutMs) {
    try {
      const state = JSON.parse(await fs.readFile(statePath, "utf8"));
      latestState = state;
      if (state.status === "completed") {
        const events = (await fs.readFile(eventsPath, "utf8"))
          .trim()
          .split(/\r?\n/)
          .filter(Boolean)
          .map((line) => JSON.parse(line));
        return { state, events };
      }
      if (state.status === "failed") {
        assert.fail(`workflow failed: ${JSON.stringify(state)}`);
      }
    } catch (error) {
      if (error?.code && error.code !== "ENOENT") {
        throw error;
      }
      if (!error?.code && !(error instanceof SyntaxError)) {
        throw error;
      }
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }

  assert.fail(`Timed out waiting for workflow completion: ${JSON.stringify(latestState)}`);
}

test("real temporary-provider workflows launch and return output for every supported provider", { timeout: 1200000 }, async (t) => {
  const providers = selectedProviders();
  const unknown = providers.filter((provider) => !PROVIDERS.includes(provider));
  assert.deepEqual(
    unknown,
    [],
    `Unknown provider(s) in WARDIAN_E2E_HEADLESS_PROVIDERS: ${unknown.join(", ")}`,
  );

  if (!allowPartialProviders) {
    const selected = new Set(providers);
    const missing = PROVIDERS.filter((provider) => !selected.has(provider));
    assert.deepEqual(
      missing,
      [],
      `WARDIAN_E2E_HEADLESS_PROVIDERS must include the full provider matrix unless WARDIAN_E2E_HEADLESS_ALLOW_PARTIAL=1. Missing: ${missing.join(", ")}`,
    );
  }

  if (!runRealHeadlessProviders) {
    t.skip("Set WARDIAN_E2E_REAL_HEADLESS_PROVIDERS=1 to run real temporary-provider workflow validation.");
    return;
  }

  const harness = await createNativeHarness();
  try {
    if (!skipNativeBuild) {
      ensureNativeAppBuilt(harness);
    }
  } catch (error) {
    t.skip(String(error));
    return;
  }

  prepareIsolatedHome(harness);
  const runId = `${process.pid}-${Date.now()}`;
  const codexNonGitWorkspace = path.join(harness.isolatedHome, "codex-non-git-workspace");
  await fs.mkdir(codexNonGitWorkspace, { recursive: true });

  let session;
  try {
    session = await startNativeSession(harness);
  } catch (error) {
    t.skip(String(error));
    return;
  }

  t.after(async () => {
    await session.close();
  });

  await waitForAppShell(session.driver, 20000);

  for (const provider of providers) {
    const marker = `WARDIAN_HEADLESS_WORKFLOW_${provider.toUpperCase()}_${runId}`;
    const workflowId = `wf-headless-${provider}-${runId}`;
    const workspace = provider === "codex" ? codexNonGitWorkspace : workspacePath;
    const workflowPath = await seedWorkflow(harness, { workflowId, marker });

    try {
      const started = await invokeTemporaryProviderWorkflow(session.driver, {
        workflowPath,
        provider,
        workspace,
      });
      const trace = await readCompletedWorkflow(started.run_dir);
      const output = nodeOutputText(trace.state.registry?.nodes?.["provider-turn"]?.output);

      assert.equal(trace.state.nodes?.["provider-turn"], "completed");
      assert.ok(
        trace.events.some((event) => event.kind === "node_completed" && event.node === "provider-turn"),
        `missing provider-turn completion event: ${JSON.stringify(trace.events)}`,
      );
      assert.ok(
        output.includes(marker),
        `${provider} workflow output did not include ${marker}: ${output}`,
      );
    } catch (error) {
      const debugTail = await readDebugTail(harness);
      assert.fail(
        `Real headless workflow failed for ${provider}: ${error.message}\n\n` +
          `--- Wardian debug tail ---\n${debugTail}`,
      );
    }
  }
});
