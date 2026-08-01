import { expect, test } from "@playwright/test";

import { openSurface, surfacePanel, surfaceTab } from "../fixtures/workbench";
import {
  installWorkbenchIpcMock,
  makeWorkbenchDocument,
  makeWorkbenchSurface,
  type WorkbenchAgentFixture,
} from "../fixtures/workbenchIpcMock";

const agents: WorkbenchAgentFixture[] = [
  {
    session_id: "agent-alpha",
    session_name: "Alpha",
    agent_class: "Coder",
    folder: "/workspace/alpha",
    provider: "mock",
    is_off: false,
  },
  {
    session_id: "agent-beta",
    session_name: "Beta",
    agent_class: "Reviewer",
    folder: "/workspace/beta",
    provider: "mock",
    is_off: false,
  },
];

test("renders a capture-ready tabs-and-splits workbench", async ({ page }, testInfo) => {
  const overview = makeWorkbenchSurface("overview-evidence", "agents-overview", {
    state: {
      mode: "grid",
      focused_agent_id: null,
      search_query: "",
      status_filter: [],
    },
  });
  const queue = makeWorkbenchSurface("inbox-evidence", "inbox");
  const document = makeWorkbenchDocument({
    revision: 2,
    root: {
      kind: "split",
      node_id: "evidence-split",
      direction: "horizontal",
      ratio: 0.7,
      first: { kind: "group", group_id: "group-overview" },
      second: { kind: "group", group_id: "group-queue" },
    },
    groups: {
      "group-overview": {
        group_id: "group-overview",
        surface_ids: [overview.surface_id],
        active_surface_id: overview.surface_id,
      },
      "group-queue": {
        group_id: "group-queue",
        surface_ids: [queue.surface_id],
        active_surface_id: queue.surface_id,
      },
    },
    surfaces: [overview, queue],
    active_group_id: "group-overview",
  });

  await page.setViewportSize({ width: 1920, height: 1080 });
  await page.addInitScript(() => {
    localStorage.setItem("wardian-settings", JSON.stringify({
      state: { gridCardDisplayMode: "chat" },
      version: 2,
    }));
  });
  await installWorkbenchIpcMock(page, {
    agents,
    load_result: {
      source: "primary",
      document,
      notice: null,
      durable_revision: document.revision,
      durable_token: "evidence-token-2",
    },
  });

  await page.goto("/");
  await expect(page.getByTestId("workbench-group")).toHaveCount(2);
  await expect(surfaceTab(page, "agents-overview")).toBeVisible();
  await expect(surfaceTab(page, "inbox")).toBeVisible();
  await expect(surfacePanel(page, "agents-overview")).toBeVisible();
  await expect(surfacePanel(page, "inbox")).toBeVisible();
  await expect(page.getByTestId("sidebar-icon-rail")).toBeVisible();
  await expect(page.getByTestId("agent-watchlist")).toBeVisible();
  await expect(page.locator('[data-testid="agent-card"]:visible')).toHaveCount(2);
  await expect(page.getByText("Saving workbench changes…", { exact: true })).toBeHidden();
  await page.waitForTimeout(500);

  const path = process.env.WARDIAN_WORKBENCH_SCREENSHOT
    ?? testInfo.outputPath("tabs-and-splits.png");
  await page.screenshot({ path, animations: "disabled" });
  await testInfo.attach("tabs-and-splits", { path, contentType: "image/png" });
});

test("renders chat attachment chips alongside a compact provider launch row", async ({ page }, testInfo) => {
  const overview = makeWorkbenchSurface("chat-attachment-evidence", "agents-overview", {
    state: {
      mode: "single",
      focused_agent_id: "agent-alpha",
      search_query: "",
      status_filter: [],
    },
  });
  const document = makeWorkbenchDocument({ revision: 3, surfaces: [overview] });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.addInitScript(() => {
    localStorage.setItem("wardian-settings", JSON.stringify({
      state: { gridCardDisplayMode: "chat" },
      version: 2,
    }));
  });
  await installWorkbenchIpcMock(page, {
    agents: [{
      session_id: "agent-alpha",
      session_name: "Alpha",
      agent_class: "Coder",
      folder: "/workspace/alpha",
      provider: "codex",
      is_off: false,
    }],
    load_result: {
      source: "primary",
      document,
      notice: null,
      durable_revision: document.revision,
      durable_token: "chat-attachment-evidence-token-3",
    },
    responses: {
      load_agent_chat_transcript: [{
        id: "codex-launch",
        session_id: "agent-alpha",
        provider: "codex",
        kind: "terminal_output",
        role: "system",
        text: "OpenAI Codex\nReady for your task\n/workspace/alpha",
        title: "Codex started",
        status: null,
        turn_id: null,
        source: null,
        command: null,
        exit_code: null,
        path: null,
        language: null,
        created_at: null,
        sequence: 1,
        metadata: { terminal_presentation: "launch" },
      }],
      "plugin:dialog|open": ["C:/evidence/dashboard.png", "C:/evidence/notes.txt"],
    },
  });

  await page.goto("/");
  await expect(page.getByTestId("agent-card")).toBeVisible();
  await expect(page.getByText("Codex started", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Attach files" }).click();
  await expect(page.getByText("dashboard.png", { exact: true })).toBeVisible();
  await expect(page.getByText("notes.txt", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Send message" })).toBeEnabled();

  const path = process.env.WARDIAN_CHAT_ATTACHMENTS_SCREENSHOT
    ?? testInfo.outputPath("chat-attachments.png");
  await page.locator('[data-testid="agent-card"]').screenshot({ path, animations: "disabled" });
  await testInfo.attach("chat-attachments", { path, contentType: "image/png" });
});

test("renders a capture-ready new-tab surface launcher", async ({ page }, testInfo) => {
  const dashboard = makeWorkbenchSurface("dashboard-launcher-evidence", "dashboard");
  const document = makeWorkbenchDocument({ revision: 3, surfaces: [dashboard] });
  await page.setViewportSize({ width: 1440, height: 900 });
  await installWorkbenchIpcMock(page, {
    load_result: {
      source: "primary",
      document,
      notice: null,
      durable_revision: document.revision,
      durable_token: "launcher-evidence-token-3",
    },
  });

  await page.goto("/");
  const group = page.getByTestId("workbench-group").filter({
    has: surfaceTab(page, "dashboard"),
  });
  await group.getByLabel("Open Surface", { exact: true }).click();
  await expect(group.getByRole("tab", { name: "New Tab", exact: true }))
    .toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("heading", { name: "Choose a surface" })).toBeVisible();
  await expect(page.getByLabel("Available surfaces").getByRole("button")).toHaveCount(8);
  await expect(page.getByText("Monitor active agents.", { exact: true })).toBeVisible();
  await page.waitForTimeout(250);

  const path = process.env.WARDIAN_WORKBENCH_LAUNCHER_SCREENSHOT
    ?? testInfo.outputPath("surface-launcher.png");
  await page.screenshot({ path, animations: "disabled" });
  await testInfo.attach("surface-launcher", { path, contentType: "image/png" });
});

test("renders the Changes workbench with turn attribution", async ({ page }, testInfo) => {
  const dashboard = makeWorkbenchSurface("changes-dashboard", "dashboard");
  const document = makeWorkbenchDocument({ revision: 4, surfaces: [dashboard] });
  await page.setViewportSize({ width: 1440, height: 900 });
  await installWorkbenchIpcMock(page, {
    load_result: {
      source: "primary",
      document,
      notice: null,
      durable_revision: document.revision,
      durable_token: "changes-evidence-token-4",
    },
    agents,
    explorer_root: "/workspace/alpha",
    responses: {
      load_change_review: {
        summary: {
          schema: 1,
          baseline: "last_effective_turn",
          baseline_ref: null,
          from_turn_index: 7,
          to_turn_index: 8,
          files: [
            {
              path: "src/agent.ts",
              change_kind: "modified",
              old_path: null,
              insertions: 12,
              deletions: 4,
              evidence: "attributed",
              agent_ids: ["agent-alpha"],
              turn_indices: [8],
              binary: false,
              truncated: false,
            },
            {
              path: "notes/review.md",
              change_kind: "untracked",
              old_path: null,
              insertions: null,
              deletions: null,
              evidence: "inferred",
              agent_ids: [],
              turn_indices: [],
              binary: false,
              truncated: false,
            },
          ],
          computed_at: "2026-08-01T00:00:00Z",
          truncated: false,
        },
        git_available: true,
        head_ref: "abc1234",
      },
    },
  });

  await page.goto("/");
  await page.getByLabel("Agent Alpha", { exact: true }).click();
  await openSurface(page, "changes");

  const changes = surfacePanel(page, "changes");
  await expect(changes.getByRole("heading", { name: "Changes" })).toBeVisible();
  await expect(changes.getByLabel("Change review baseline")).toHaveValue("last_effective_turn");
  await expect(changes.getByText("src/agent.ts", { exact: true })).toBeVisible();
  await expect(changes.getByText("attributed", { exact: true })).toBeVisible();
  await expect(changes.getByText("inferred", { exact: true })).toBeVisible();
  await expect(changes.getByRole("button", { name: "Mark reviewed", exact: true })).toBeVisible();

  const path = process.env.WARDIAN_CHANGE_REVIEW_SCREENSHOT
    ?? testInfo.outputPath("changes-workbench.png");
  await page.screenshot({ path, animations: "disabled" });
  await testInfo.attach("changes-workbench", { path, contentType: "image/png" });
});
