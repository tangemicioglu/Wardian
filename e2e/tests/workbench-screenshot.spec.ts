import { expect, test } from "@playwright/test";

import { surfacePanel, surfaceTab } from "../fixtures/workbench";
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
      model: "gpt-5.6-sol",
      provider_config: { type: "codex", reasoning_effort: "high" },
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
      list_provider_model_catalog: {
        provider: "codex",
        version: "codex-cli 0.146.0",
        source: "live_catalog",
        refresh_error: null,
        models: [{
          id: "gpt-5.6-sol",
          display_name: "5.6 Terra",
          effort_options: ["low", "high"],
          default_effort: "high",
          is_default: true,
        }],
      },
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
  await expect(page.getByLabel("Available surfaces").getByRole("button")).toHaveCount(7);
  await expect(page.getByText("Monitor active agents.", { exact: true })).toBeVisible();
  await page.waitForTimeout(250);

  const path = process.env.WARDIAN_WORKBENCH_LAUNCHER_SCREENSHOT
    ?? testInfo.outputPath("surface-launcher.png");
  await page.screenshot({ path, animations: "disabled" });
  await testInfo.attach("surface-launcher", { path, contentType: "image/png" });
});
