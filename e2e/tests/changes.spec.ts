import { expect, test } from "@playwright/test";

import { installWorkbenchIpcMock } from "../fixtures/workbenchIpcMock";

const ROOT = "/workspace";

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
          }],
          computed_at: "2026-08-01T00:00:00Z",
          truncated: false,
        },
        git_available: true,
        head_ref: "head-1",
      },
      git_show_file_revision: "baseline\n",
    },
  });

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
  await expect(file).toHaveAttribute("aria-expanded", "false");
  await file.click();
  await expect(file).toHaveAttribute("aria-expanded", "true");
  await expect.poll(async () => (await ipc.calls("git_show_file_revision")).length).toBe(1);
});
