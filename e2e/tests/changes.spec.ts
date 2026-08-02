import { expect, test } from "@playwright/test";
import { existsSync, mkdirSync } from "node:fs";
import path from "node:path";

import { installWorkbenchIpcMock } from "../fixtures/workbenchIpcMock";

const ROOT = "/workspace";

function screenshotTimestamp(date = new Date()): string {
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}-${pad(date.getHours())}${pad(date.getMinutes())}${pad(date.getSeconds())}`;
}

test("selects Changes, chooses a baseline, and expands a file", async ({ page }) => {
  const ipc = await installWorkbenchIpcMock(page, {
    agents: [{
      session_id: "agent-1",
      session_name: "Agent Alpha",
      agent_class: "Coder",
      folder: ROOT,
      provider: "mock",
      is_off: false,
    }],
    explorer_root: ROOT,
    files: [{
      path: `${ROOT}/src/changed.ts`,
      content: "current\n",
    }, {
      path: `${ROOT}/src/agent.ts`,
      content: "agent-current\n",
    }],
    responses: {
      load_change_review_prefs: { schema: 1, baseline: "last_effective_turn" },
      load_change_review: {
        summary: {
          schema: 1,
          baseline: "last_effective_turn",
          baseline_ref: null,
          from_turn_index: 1,
          to_turn_index: 1,
          files: [{
            path: "src/changed.ts",
            change_kind: "modified",
            old_path: null,
            insertions: 2,
            deletions: 1,
            evidence: "inferred",
            agent_ids: [],
            turn_indices: [],
            binary: false,
            truncated: false,
            reviewed: false,
          }, {
            path: "src/agent.ts",
            change_kind: "modified",
            old_path: null,
            insertions: 4,
            deletions: 0,
            evidence: "attributed",
            agent_ids: ["agent-1"],
            turn_indices: [1],
            binary: false,
            truncated: false,
            reviewed: false,
          }],
          computed_at: "2026-08-01T00:00:00Z",
          truncated: false,
        },
        git_available: true,
        head_ref: "head-1",
        skipped_turn_records: 0,
      },
      git_show_file_revision: "baseline\n",
    },
  });

  await page.setViewportSize({ width: 1920, height: 1080 });
  await page.goto("/");
  await expect(page.getByTestId("app-shell")).toBeVisible();
  await page.locator('[data-testid="agent-watchlist"] .watchlist-row[aria-label="Agent Agent Alpha"]').click();
  await page.getByTestId("sidebar-tab-changes").click();

  const panel = page.getByTestId("changes-panel");
  await expect(panel).toBeVisible();
  const baseline = panel.getByLabel("Change review baseline");
  await expect(baseline).toHaveValue("last_effective_turn");
  await baseline.selectOption("head");
  await expect(baseline).toHaveValue("head");

  const file = panel.getByRole("button", { name: /src\/changed\.ts/ });
  await expect(panel.getByRole("button", { name: /src\/agent\.ts/ })).toBeVisible();
  await expect(panel.getByText("attributed")).toBeVisible();
  await expect(panel.getByText("inferred")).toBeVisible();
  await expect(file).toHaveAttribute("aria-expanded", "false");
  await file.click();
  await expect(file).toHaveAttribute("aria-expanded", "true");
  await expect.poll(async () => (await ipc.calls("git_show_file_revision")).length).toBe(1);

  const comparison = panel.locator(".files-comparison-lens");
  await expect(comparison).toBeVisible();
  const resizeHandle = page.getByTestId("sidebar-resize-handle").first();
  const resizeBox = await resizeHandle.boundingBox();
  if (!resizeBox) throw new Error("Changes sidebar resize handle is not visible");
  await page.mouse.move(resizeBox.x + resizeBox.width / 2, resizeBox.y + resizeBox.height / 2);
  await page.mouse.down();
  await page.mouse.move(resizeBox.x + 100, resizeBox.y + resizeBox.height / 2, { steps: 10 });
  await page.mouse.up();
  await page.getByRole("button", { name: "Hide Left Sidebar" }).click();
  await page.getByRole("button", { name: "Show Left Sidebar" }).click();
  await expect(comparison.locator(".files-comparison-body")).not.toHaveAttribute("data-layout", "measuring");
  await expect(comparison.locator(".monaco-diff-editor")).toBeVisible({ timeout: 30_000 });

  const screenshotPath = path.resolve(
    "e2e/screenshots/agent-change-review",
    screenshotTimestamp(),
    "changes-sidebar-pane-with-inline-diff.png",
  );
  mkdirSync(path.dirname(screenshotPath), { recursive: true });
  await page.screenshot({ path: screenshotPath, fullPage: true });
  expect(existsSync(screenshotPath)).toBe(true);
  console.log(`Changes PR screenshot evidence: ${screenshotPath}`);
});
