import { test, expect, type Page } from "@playwright/test";

test.describe("Sidebar Navigation", () => {
  test.describe.configure({ mode: "serial" });

  let page: Page;

  test.beforeAll(async ({ browser }) => {
    page = await browser.newPage();
    await page.goto("/", { waitUntil: "domcontentloaded" });
    await page.locator('[data-testid="app-shell"]').waitFor({ timeout: 15_000 });
  });

  test.afterAll(async () => {
    await page.close();
  });

  test("sidebar icon rail has navigation buttons", async () => {
    const rail = page.locator('[data-testid="sidebar-icon-rail"]');
    const buttons = rail.locator("button");
    await expect(buttons.nth(0)).toHaveAttribute("data-testid", "sidebar-tab-explorer");
    await expect(buttons.nth(1)).toHaveAttribute("data-testid", "sidebar-tab-agent-config");
    const count = await buttons.count();
    expect(count).toBeGreaterThanOrEqual(3);
  });

  test("clicking Agent Configuration opens its pane through its stable target", async () => {
    const rail = page.locator('[data-testid="sidebar-icon-rail"]');
    await rail.getByTestId("sidebar-tab-explorer").click();
    await rail.getByTestId("sidebar-tab-agent-config").click();

    await expect(rail.getByTestId("sidebar-tab-agent-config")).toHaveAttribute("data-sidebar-active", "true");
    await expect(page.getByRole("heading", { name: "Agent Configuration" })).toBeVisible();
  });
});
