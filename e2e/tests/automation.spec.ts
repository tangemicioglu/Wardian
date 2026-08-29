/**
 * Automation Builder E2E tests.
 *
 * Tests are split into two groups:
 *   1. UI-only (browser E2E): builder renders, block palette, navigation.
 *   2. @native-only: tests that require live agent blocks executing inside
 *      a running automation. These need the native E2E harness.
 *      Run via: npm run test:e2e:native
 */

import { test, expect, type Page } from "@playwright/test";
import { openSurface } from "../fixtures/workbench";

test.describe("Automation Builder UI", () => {
  test.describe.configure({ mode: "serial" });

  let page: Page;

  test.beforeAll(async ({ browser }) => {
    page = await browser.newPage();
    await page.goto("/", { waitUntil: "domcontentloaded" });
    await page.locator('[data-testid="app-shell"]').waitFor({ timeout: 15_000 });
    await page.locator('[data-testid="sidebar-tab-automations"]').click();
    await page.locator("aside").nth(1).getByRole("heading", { name: "Automations" }).waitFor();
  });

  test.afterAll(async () => {
    await page.close();
  });

  test("automation glance pane renders", async () => {
    const sidebar = page.locator("aside").nth(1);
    await expect(sidebar.getByRole("heading", { name: "Automations" })).toBeVisible();
    await expect(sidebar.getByRole("button", { name: "Monitor" })).toBeVisible();
  });

  test("switching to Automations view renders the edit canvas", async () => {
    await openSurface(page, "automations");
    await expect(page.getByTestId("automations-view")).toBeVisible();
    await expect(page.getByTestId("automations-edit-mode")).toBeVisible();
    await expect(page.locator(".react-flow")).toBeVisible();
  });

  test("automation edit mode opens the node library", async () => {
    await openSurface(page, "automations");
    await page.getByTestId("automations-view").getByRole("button", { name: "Add node" }).click();
    await expect(page.getByTestId("node-library")).toBeVisible();
    await page.getByTestId("node-library").getByRole("button", { name: "Close" }).click();
    await expect(page.getByTestId("node-library")).toHaveCount(0);
  });

  test("add-node button is visible in edit mode", async () => {
    await openSurface(page, "automations");
    await expect(page.getByTestId("automations-view").getByRole("button", { name: "Add node" })).toBeVisible();
  });

  test("run button is disabled for an unsaved empty automation", async () => {
    await openSurface(page, "automations");
    await expect(page.getByTestId("automations-view").getByRole("button", { name: /^Run$/ })).toBeDisabled();
  });
});

// @native-only: the tests below require live agent blocks and real automation execution.
// Automation execution triggers agent spawning which requires native IPC + PTY.
test.describe("Automation Execution (@native-only)", () => {
  test.skip("creating an automation with two mock-agent blocks shows them on canvas", async () => {
    // Open builder, click add-block, add two mock-agent blocks.
    // Assert two block nodes visible on canvas.
  });

  test.skip("running an automation transitions block status to Processing", async () => {
    // Create automation with mock agents.
    // Click run-automation-button.
    // Assert block status indicators show Processing.
  });

  test.skip("automation completes and block status transitions to Idle", async () => {
    // Run automation with basic mock scenario.
    // Assert all blocks reach Idle/completed state.
  });

  test.skip("cancelling a running automation stops block execution", async () => {
    // Start automation, then cancel mid-run.
    // Assert blocks stop and automation shows cancelled state.
  });
});
