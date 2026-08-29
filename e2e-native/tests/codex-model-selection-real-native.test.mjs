// @tier manual — Needs a real provider or a logged-in CLI. Run it deliberately.
import test from "node:test";
import assert from "node:assert/strict";

import {
  createNativeHarness,
  ensureNativeAppBuilt,
  invokeTauri,
  invokeTauriResult,
  prepareIsolatedHome,
  startNativeSession,
  waitForAppShell,
} from "../lib/harness.mjs";

const runRealCodexModelSelection = process.env.WARDIAN_E2E_REAL_CODEX_MODEL_SELECTION === "1";
const skipNativeBuild = process.env.WARDIAN_NATIVE_SKIP_BUILD === "1";
const workspacePath = process.env.WARDIAN_E2E_REAL_WORKSPACE || process.cwd();

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function waitForCodexReady(driver, sessionId, modelIds, timeoutMs = 60000) {
  const startedAt = Date.now();
  let lastText = "";
  let acceptedWorkspaceTrust = false;
  while (Date.now() - startedAt < timeoutMs) {
    try {
      const snapshot = await invokeTauri(driver, "request_terminal_snapshot", {
        request: { session_id: sessionId },
      });
      lastText = snapshot.visible_grid ?? "";
      if (!acceptedWorkspaceTrust && lastText.includes("Do you trust the contents of this directory?")) {
        await invokeTauri(driver, "inject_session_input", {
          sessionId,
          text: "\r",
        });
        acceptedWorkspaceTrust = true;
      }
      if (modelIds.some((modelId) => lastText.includes(modelId))) {
        return snapshot;
      }
    } catch {
      // The terminal runtime may not be registered during the first polls.
    }
    await sleep(250);
  }
  throw new Error(`Codex did not become ready. Last visible grid:\n${lastText}`);
}

test("native Codex model selection drives the interactive model and effort pickers", { timeout: 240000 }, async (t) => {
  if (!runRealCodexModelSelection) {
    t.skip("Set WARDIAN_E2E_REAL_CODEX_MODEL_SELECTION=1 to run real Codex model selection.");
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

  let session;
  try {
    session = await startNativeSession(harness);
  } catch (error) {
    t.skip(String(error));
    return;
  }

  let sessionId = null;
  t.after(async () => {
    if (sessionId) {
      await invokeTauriResult(session.driver, "kill_agent", { sessionId });
    }
    await session.close();
  });

  await waitForAppShell(session.driver, 120000);
  const catalog = await invokeTauri(session.driver, "list_provider_model_catalog", {
    provider: "codex",
    forceRefresh: true,
  });
  if (!Array.isArray(catalog.models) || catalog.models.length < 2) {
    t.skip(`Codex did not expose enough live models: ${catalog.refresh_error ?? "unknown error"}`);
    return;
  }

  const target = catalog.models.find((model) => !model.is_default && model.effort_options?.length > 0);
  if (!target) {
    t.skip("Codex did not expose a non-default model with reasoning efforts.");
    return;
  }
  const effort = target.effort_options.includes("low")
    ? "low"
    : target.default_effort ?? target.effort_options[0];

  const spawned = await invokeTauriResult(session.driver, "spawn_agent", {
    req: {
      sessionName: `NativeCodexModel-${Date.now().toString(36)}`,
      agentClass: "TestClass",
      folder: workspacePath,
      resumeSession: null,
      isOff: false,
      configOverride: {
        provider: "codex",
        custom_args: "-c tui.show_tooltips=false",
      },
    },
  });
  if (!spawned.ok && /program not found|No such file|cannot find/i.test(String(spawned.error?.message ?? spawned.error))) {
    t.skip(`Codex executable is unavailable: ${spawned.error?.message ?? spawned.error}`);
    return;
  }
  assert.equal(spawned.ok, true, `spawn_agent failed: ${JSON.stringify(spawned.error)}`);
  sessionId = spawned.value.session_id;

  await waitForCodexReady(
    session.driver,
    sessionId,
    catalog.models.map((model) => model.id),
  );

  const result = await invokeTauri(session.driver, "update_agent_model_selection", {
    sessionId,
    model: target.id,
    reasoningEffort: effort,
  });
  assert.equal(result.config.model, target.id);
  assert.equal(result.config.provider_config.reasoning_effort, effort);
  if (result.live_application !== "applied") {
    const failedSnapshot = await invokeTauri(session.driver, "request_terminal_snapshot", {
      request: { session_id: sessionId },
    });
    assert.equal(
      result.live_application,
      "applied",
      `${result.live_error ?? "live application failed"}\nVisible grid:\n${failedSnapshot.visible_grid}`,
    );
  }

  const snapshot = await invokeTauri(session.driver, "request_terminal_snapshot", {
    request: { session_id: sessionId },
  });
  const visible = snapshot.visible_grid.toLowerCase();
  assert.match(visible, new RegExp(target.id.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "i"));
  assert.match(visible, new RegExp(`\\b${effort}\\b`, "i"));
});
