import { expect, test } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";

import {
  installWorkbenchIpcMock,
  makeWorkbenchDocument,
  makeWorkbenchSurface,
} from "../fixtures/workbenchIpcMock";
import { surfacePanel, surfaceTab } from "../fixtures/workbench";

const CHECKPOINT_SESSION_ID = "checkpoint-agent";
const CHECKPOINT_LINES = Array.from(
  { length: 72 },
  (_, index) => `checkpoint-line-${String(index + 1).padStart(2, "0")} — preserved terminal history`,
).join("\r\n");

function checkpointWorkbenchDocument() {
  const overview = makeWorkbenchSurface("checkpoint-overview", "agents-overview", {
    state: {
      mode: "single",
      focused_agent_id: CHECKPOINT_SESSION_ID,
      search_query: "",
      status_filter: [],
    },
  });
  return makeWorkbenchDocument({ surfaces: [overview] });
}

test("restores and scrolls checkpointed terminal history when no broker is present", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 960 });
  await page.addInitScript(() => {
    localStorage.setItem("wardian-settings", JSON.stringify({
      state: { gridCardDisplayMode: "terminal" },
      version: 2,
    }));
  });
  await installWorkbenchIpcMock(page, {
    load_result: {
      source: "primary",
      document: checkpointWorkbenchDocument(),
      notice: null,
      durable_revision: 0,
      durable_token: "checkpoint-mock-token",
    },
    agents: [{
      session_id: CHECKPOINT_SESSION_ID,
      session_name: "Checkpoint recovery",
      agent_class: "Coder",
      folder: "/workspace/checkpoint-recovery",
      provider: "mock",
      is_off: false,
    }],
  });
  await page.addInitScript(({ checkpointSessionId, checkpointLines }) => {
    const tauriWindow = window as Window & {
      __TAURI_INTERNALS__?: {
        invoke?: (command: string, args?: Record<string, unknown>) => Promise<unknown>;
      };
      __WARDIAN_TERMINAL_CHECKPOINT_E2E__?: { loads: number };
    };
    const originalInvoke = tauriWindow.__TAURI_INTERNALS__?.invoke;
    if (!originalInvoke || !tauriWindow.__TAURI_INTERNALS__) {
      throw new Error("Expected workbench IPC mock before terminal checkpoint mock");
    }
    tauriWindow.__WARDIAN_TERMINAL_CHECKPOINT_E2E__ = { loads: 0 };
    tauriWindow.__TAURI_INTERNALS__.invoke = async (command, args) => {
      if (command === "register_terminal_presentation") {
        throw new Error("SessionNotFound");
      }
      if (command === "load_terminal_presentation_checkpoint") {
        tauriWindow.__WARDIAN_TERMINAL_CHECKPOINT_E2E__!.loads += 1;
        return {
          version: 1,
          session_id: checkpointSessionId,
          cols: 120,
          rows: 36,
          serialized_state: checkpointLines,
        };
      }
      return originalInvoke(command, args);
    };
  }, { checkpointSessionId: CHECKPOINT_SESSION_ID, checkpointLines: CHECKPOINT_LINES });

  await page.goto("/");
  await expect(surfaceTab(page, "agents-overview")).toHaveAttribute("aria-selected", "true");
  await expect(surfacePanel(page, "agents-overview")).toBeVisible();

  const terminalHost = page.locator('[data-testid="agent-terminal-host"]').first();
  await expect(terminalHost).toBeVisible();
  const presentationId = await terminalHost.getAttribute("data-terminal-presentation-id");
  expect(presentationId).not.toBeNull();

  await expect.poll(async () => page.evaluate((id) => {
    const snapshot = window.__wardianTerminalDebug?.snapshot?.(id);
    return {
      loads: window.__WARDIAN_TERMINAL_CHECKPOINT_E2E__?.loads ?? 0,
      hasHistory: snapshot?.allLines?.some((line: string) => line.includes("checkpoint-line-01")) ?? false,
      baseY: snapshot?.renderer?.baseY ?? 0,
    };
  }, presentationId!)).toEqual(expect.objectContaining({
    loads: 1,
    hasHistory: true,
  }));

  await page.evaluate((id) => {
    window.__wardianTerminalDebug?.scrollToTop?.(id);
  }, presentationId!);
  await expect.poll(async () => page.evaluate((id) => {
    const snapshot = window.__wardianTerminalDebug?.snapshot?.(id);
    return {
      viewportY: snapshot?.renderer?.viewportY,
      lines: snapshot?.renderer?.lines ?? [],
    };
  }, presentationId!)).toEqual(expect.objectContaining({
    viewportY: 0,
    lines: expect.arrayContaining([expect.stringContaining("checkpoint-line-01")]),
  }));

  const screenshotPath = process.env.WARDIAN_TERMINAL_CHECKPOINT_SCREENSHOT;
  if (screenshotPath) {
    fs.mkdirSync(path.dirname(screenshotPath), { recursive: true });
    await page.locator('[data-testid="agent-card"]').first().screenshot({
      path: screenshotPath,
      animations: "disabled",
    });
  }
});
