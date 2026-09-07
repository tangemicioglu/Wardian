import { test, expect, type Page } from "@playwright/test";
import path from "node:path";
import { openSurface, surfacePanel, surfaceTab } from "../fixtures/workbench";
import { installGardenCompositionMock, GARDEN_AGENT, GARDEN_ROOT, GARDEN_RUN } from "../fixtures/gardenComposition";

const stamp = process.env.WARDIAN_GARDEN_STAMP ?? new Date().toISOString().replace(/[:.]/g, "-");
const garden = (page: Page) => surfacePanel(page, "garden");
const object = (page: Page, key: string) => garden(page).locator(`[data-garden-object="${key}"]`);
async function capture(page: Page, name: string) {
  await expect(page.getByText("Saving workbench changes…", { exact: true })).toBeHidden();
  await garden(page).screenshot({ path: path.resolve("e2e/screenshots/garden", stamp, `${name}.png`), animations: "disabled" });
}
async function enterAgent(page: Page) {
  await object(page, `agent:${GARDEN_AGENT}`).press("Enter");
  await expect(garden(page).getByRole("region", { name: "Identity", exact: true })).toBeVisible();
}

test.describe("Garden semantic composition", () => {
  test.beforeEach(async ({ page }) => {
    await installGardenCompositionMock(page);
    await page.goto("/");
    await expect(garden(page).locator("canvas").first()).toBeVisible({ timeout: 15_000 });
    await expect(object(page, `agent:${GARDEN_AGENT}`)).toBeAttached();
  });

  test("single selection keeps camera; Enter opens five regions; memory evidence and return", async ({ page }, testInfo) => {
    const canvas = garden(page).locator(".garden-canvas");
    const agent = object(page, `agent:${GARDEN_AGENT}`);
    await agent.focus();
    const before = await canvas.getAttribute("data-garden-fit");
    const hit = await agent.boundingBox();
    if (!hit) throw new Error("Agent has no canvas hit target");
    await page.mouse.click(hit.x + hit.width / 2, hit.y + hit.height / 2);
    await expect(agent).toHaveAttribute("aria-pressed", "true");
    await expect(canvas).toHaveAttribute("data-garden-fit", before!);
    await expect(surfaceTab(page, "agent-session")).toHaveCount(0);
    await capture(page, "01-selected-habitat");
    await agent.press("Enter");
    for (const name of ["Identity", "Capabilities", "Memory", "Active work", "Ports"]) {
      await expect(garden(page).getByRole("region", { name, exact: true })).toBeVisible();
      await expect(garden(page).getByRole("heading", { name, exact: true })).toBeInViewport();
    }
    await expect(garden(page).getByRole("region", { name: "Ports", exact: true }).getByRole("button", { name: /synthetic\/garden/ })).toBeInViewport({ ratio: 1 });
    await expect(garden(page).getByTestId("garden-selection-summary").getByRole("button", { name: "Enter", exact: true })).toHaveCount(0);
    await expect(garden(page).getByRole("region", { name: "Active work", exact: true })).toContainText("Draft ready for evidence review.");
    await expect(garden(page).getByRole("region", { name: "Capabilities" })).toContainText("Interface Review");
    const memory = garden(page).getByRole("button", { name: /Keep the five agent regions/ });
    await expect(memory).toBeVisible();
    await capture(page, "02-agent-cutaway");
    const identityGeometry = await garden(page).getByRole("region", { name: "Identity", exact: true }).evaluate((region) => {
      const style = getComputedStyle(region);
      const heading = region.querySelector("h3")!;
      const range = document.createRange();
      range.selectNodeContents(heading);
      const text = range.getBoundingClientRect();
      const textPoints = [text.left + 1, (text.left + text.right) / 2, text.right - 1].map((x) => {
        const hit = document.elementFromPoint(x, (text.top + text.bottom) / 2);
        return { x, visible: hit === heading || heading.contains(hit) };
      });
      return { bounds: region.getBoundingClientRect().toJSON(), heading: text.toJSON(), textPoints,
        padding: style.padding, borderRadius: style.borderRadius, overflow: style.overflow };
    });
    await testInfo.attach("identity-computed-geometry", { body: JSON.stringify(identityGeometry, null, 2), contentType: "application/json" });
    console.info("Identity computed geometry:", JSON.stringify(identityGeometry));
    expect(identityGeometry.textPoints.every((point) => point.visible), JSON.stringify(identityGeometry)).toBe(true);
    await memory.click();
    await expect(garden(page).getByRole("article", { name: "memory record" })).toHaveCount(0);
    await expect(garden(page).getByTestId("garden-selection-summary").getByRole("button", { name: "Open record", exact: true })).toBeVisible();
    await memory.press("Enter");
    const record = garden(page).getByRole("article", { name: "memory record" });
    await expect(garden(page).getByTestId("garden-selection-summary").getByRole("button", { name: "Open record", exact: true })).toHaveCount(0);
    await expect(record).toContainText("Review confirmed that Memory stays beside Capabilities");
    await expect(record).toContainText(GARDEN_ROOT);
    await expect(record).toContainText("conversation-design:turn:4");
    await record.getByText("Revision history (2)").click();
    await expect(record).toContainText("Keep agent regions stable.");
    await capture(page, "03-memory-evidence");
    await page.keyboard.press("Escape");
    await expect(garden(page).getByRole("region", { name: "Memory", exact: true })).toBeVisible();
    await garden(page).getByRole("navigation", { name: "Garden breadcrumb" }).getByRole("button", { name: "Habitat", exact: true }).click();
    await expect(canvas).toHaveAttribute("data-garden-fit", before!);
    await expect(garden(page).locator(".garden-composition")).toHaveCount(0);
  });

  test("workspace activity excludes unchanged siblings until full tree; file record carries evidence", async ({ page }) => {
    await enterAgent(page);
    await garden(page).getByRole("region", { name: "Ports" }).getByRole("button", { name: /synthetic\/garden/ }).press("Enter");
    const workspace = garden(page).getByRole("region", { name: "Workspace activity" });
    await expect(workspace.getByRole("button", { name: /src/ })).toBeVisible();
    await expect(workspace.getByRole("button", { name: /README/ })).toHaveCount(0);
    await capture(page, "04-workspace-activity");
    await workspace.getByRole("checkbox", { name: "Show full tree" }).check();
    await expect(workspace.getByRole("button", { name: /README/ })).toBeVisible();
    await capture(page, "05-workspace-full-tree");
    await workspace.getByRole("checkbox", { name: "Show full tree" }).uncheck();
    await workspace.getByRole("button", { name: /src/ }).press("Enter");
    await workspace.getByRole("button", { name: /cutaway.tsx/ }).press("Enter");
    const record = garden(page).getByRole("article", { name: "path record" });
    await expect(record).toContainText("export const regions");
    await expect(record).toContainText("attributed");
    await expect(record).toContainText("garden-reviewer");
    await expect(record.getByRole("button", { name: "Open file", exact: true })).toBeVisible();
    await capture(page, "06-file-evidence");
  });

  test("multi-agent schedule opens ordered run stages and immutable output evidence", async ({ page }) => {
    await enterAgent(page);
    await garden(page).getByRole("region", { name: "Active work", exact: true }).getByRole("button", { name: /Daily design review/ }).press("Enter");
    const flow = garden(page).getByRole("region", { name: "Automation composition" });
    await expect(flow).toContainText("2 assigned agents");
    const lane = flow.getByRole("region", { name: `Run ${GARDEN_RUN}`, exact: true });
    await expect(lane.getByRole("button", { name: /^Stage \d/ })).toHaveText([/Stage 1.*Draft interface.*completed.*Moss Designer/, /Stage 2.*Review evidence.*running.*Fern Reviewer/]);
    await capture(page, "07-run-flow");
    await lane.getByRole("button", { name: /Draft interface/ }).press("Enter");
    await expect(garden(page).getByRole("article")).toContainText("cutaway-preview");
    await expect(garden(page).getByRole("article")).toContainText(GARDEN_RUN);
    await expect(garden(page).getByRole("navigation", { name: "Garden breadcrumb" }).getByRole("button").last()).toHaveText("draft");
    await capture(page, "08-stage-output");
    await page.keyboard.press("Escape");
    await expect(flow).toBeVisible();
  });

  test("canonical agent action preserves Garden breadcrumb and selection on return", async ({ page }) => {
    await openSurface(page, "agents-overview");
    await expect(surfaceTab(page, "agents-overview")).toHaveCount(1);
    await surfaceTab(page, "garden").click();
    await enterAgent(page);
    await garden(page).getByRole("region", { name: "Identity", exact: true }).getByRole("button", { name: /Moss Designer.*Designer/ }).press("Enter");
    const breadcrumb = await garden(page).getByRole("navigation", { name: "Garden breadcrumb" }).textContent();
    const camera = await garden(page).locator(".garden-canvas").getAttribute("data-garden-fit");
    await garden(page).getByRole("article", { name: "identity record" }).getByRole("button", { name: "Open agent session", exact: true }).click();
    await expect(surfaceTab(page, "agent-session")).toHaveCount(1);
    await expect(surfaceTab(page, "agents-overview")).toHaveCount(1);
    await expect(surfaceTab(page, "agent-session")).toHaveAttribute("aria-selected", "true");
    await expect(surfacePanel(page, "agent-session").getByRole("textbox", { name: "Terminal input" })).toBeAttached();
    await surfaceTab(page, "garden").click();
    await expect(garden(page).getByRole("article", { name: "identity record" })).toBeVisible();
    await expect(garden(page).getByRole("navigation", { name: "Garden breadcrumb" })).toHaveText(breadcrumb!);
    await expect(garden(page).locator(".garden-canvas")).toHaveAttribute("data-garden-fit", camera!);
    await surfaceTab(page, "agent-session").click();
    await surfaceTab(page, "agent-session").click({ button: "right" });
    await page.getByRole("menuitem", { name: "Close tab", exact: true }).click();
    await expect(garden(page).getByRole("article", { name: "identity record" })).toBeVisible();
    await expect(garden(page).getByRole("navigation", { name: "Garden breadcrumb" })).toHaveText(breadcrumb!);
    await expect(garden(page).locator(".garden-canvas")).toHaveAttribute("data-garden-fit", camera!);
  });

  test("keyboard roving focus, Space selection, narrow layout and reduced motion", async ({ page }) => {
    await page.setViewportSize({ width: 640, height: 900 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    for (const label of ["Hide Left Sidebar", "Hide Agent List"]) {
      const toggle = page.getByRole("button", { name: label, exact: true });
      if (await toggle.isVisible()) await toggle.click();
    }
    await expect(garden(page).locator("canvas").first()).toBeVisible();
    await garden(page).getByTestId("garden-fit-view").click();
    await object(page, `district:workspace:${GARDEN_ROOT}`).press("Enter");
    const agent = object(page, `agent:${GARDEN_AGENT}`);
    await agent.focus();
    await agent.press("Space");
    await expect(agent).toHaveAttribute("aria-pressed", "true");
    await agent.press("ArrowRight");
    await expect(agent).not.toBeFocused();
    await agent.press("Enter");
    await expect(garden(page).getByRole("region", { name: "Identity", exact: true })).toBeVisible();
    await expect(garden(page).locator(".garden-composition")).toHaveCSS("animation-name", "none");
    await garden(page).getByRole("region", { name: "Ports", exact: true }).scrollIntoViewIfNeeded();
    await expect(garden(page).getByRole("region", { name: "Ports", exact: true })).toBeInViewport();
    expect(await garden(page).evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);
    await garden(page).getByRole("region", { name: "Identity", exact: true }).scrollIntoViewIfNeeded();
    await capture(page, "09-narrow-reduced-motion");
    await page.keyboard.press("Escape");
    await expect(garden(page).locator(".garden-composition")).toHaveCount(0);
  });

  test("workstream shows labelled inhabitants and situated run route", async ({ page }) => {
    await object(page, `district:workspace:${GARDEN_ROOT}`).press("Enter");
    await expect(garden(page).locator(".garden-canvas")).toHaveAttribute("data-focused-district", `workspace:${GARDEN_ROOT}`);
    await expect(object(page, `agent:${GARDEN_AGENT}`)).toBeAttached();
    await expect(object(page, "automation:schedule:daily-design")).toBeAttached();
    await capture(page, "10-workstream-inhabitants");
  });

  for (const action of ["double-click", "keyboard Enter", "summary Enter"] as const) {
    test(`nested canvas directory opens Workspace with ${action}`, async ({ page }) => {
      await object(page, `district:workspace:${GARDEN_ROOT}`).press("Enter");
      const directory = garden(page).locator(`[data-garden-object$=":${GARDEN_ROOT}/src"]`);
      await expect(directory).toBeAttached();
      const hit = await directory.boundingBox();
      if (!hit) throw new Error("Nested directory has no canvas target");
      const point = { x: hit.x + hit.width / 2, y: hit.y + hit.height / 2 };
      const camera = await garden(page).locator(".garden-canvas").getAttribute("data-garden-fit");
      await page.mouse.click(point.x, point.y);
      await expect(directory).toHaveAttribute("aria-pressed", "true");
      await expect(directory).toHaveAttribute("data-garden-object", `workspace:${GARDEN_ROOT}/src`);
      await expect(garden(page).locator(".garden-canvas")).toHaveAttribute("data-garden-fit", camera!);
      if (action === "double-click") await page.mouse.dblclick(point.x, point.y);
      else if (action === "keyboard Enter") await directory.press("Enter");
      else await garden(page).getByTestId("garden-selection-summary").getByRole("button", { name: "Enter", exact: true }).click();
      await expect(garden(page).getByRole("region", { name: "Workspace activity" })).toBeVisible();
      await expect(garden(page).getByRole("region", { name: "Workspace activity" })).toContainText(`${GARDEN_ROOT}/src`);
      await expect(garden(page).getByRole("article", { name: "path record" })).toHaveCount(0);
      await expect(garden(page).getByRole("button", { name: /cutaway.tsx/ })).toBeVisible();
    });
  }

  test("composition margins cannot select, pan, zoom or enter background objects", async ({ page }) => {
    await enterAgent(page);
    const canvas = garden(page).locator(".garden-canvas");
    const bounds = await canvas.boundingBox();
    if (!bounds) throw new Error("Canvas has no bounds");
    const point = { x: bounds.x + 5, y: bounds.y + bounds.height / 2 };
    const camera = await canvas.getAttribute("data-garden-fit");
    const breadcrumb = await garden(page).getByRole("navigation", { name: "Garden breadcrumb" }).textContent();
    const summary = await garden(page).getByTestId("garden-selection-summary").textContent();
    for (const gesture of ["click", "double-click", "drag", "wheel"] as const) {
      await test.step(gesture, async () => {
        await page.mouse.move(point.x, point.y);
        if (gesture === "click") await page.mouse.click(point.x, point.y);
        if (gesture === "double-click") await page.mouse.dblclick(point.x, point.y);
        if (gesture === "drag") {
          await page.mouse.down();
          await page.mouse.move(point.x, point.y + 90, { steps: 8 });
          await page.mouse.up();
        }
        if (gesture === "wheel") await page.mouse.wheel(0, -400);
        // Wait across frames so asynchronous wheel/paint work cannot escape the assertion.
        await page.evaluate(() => new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
        await expect(canvas).toHaveAttribute("data-garden-fit", camera!);
        await expect(garden(page).getByRole("navigation", { name: "Garden breadcrumb" })).toHaveText(breadcrumb!);
        await expect(garden(page).getByTestId("garden-selection-summary")).toHaveText(summary!);
        await expect(garden(page).getByRole("region", { name: "Identity", exact: true })).toBeVisible();
      });
    }
  });

  test("canonical run evidence opens Observe and schedule management opens Monitor with Garden return", async ({ page }) => {
    await enterAgent(page);
    await garden(page).getByRole("region", { name: "Active work", exact: true }).getByRole("button", { name: /Daily design review/ }).press("Enter");
    await garden(page).getByRole("region", { name: `Run ${GARDEN_RUN}`, exact: true }).getByRole("button", { name: /Draft interface/ }).press("Enter");
    await garden(page).getByRole("button", { name: "Inspect run evidence", exact: true }).click();
    const automations = surfacePanel(page, "automations");
    await expect(automations.getByTestId("automations-observe-mode")).toBeVisible();
    await expect(automations.getByTestId("automations-observe-mode")).toContainText(GARDEN_RUN);
    await surfaceTab(page, "automations").click({ button: "right" });
    await page.getByRole("menuitem", { name: "Close tab", exact: true }).click();
    await expect(garden(page).getByRole("article")).toContainText("cutaway-preview");
    await garden(page).locator(".garden-composition").press("Escape");
    await garden(page).getByRole("button", { name: "Manage schedules in Monitor", exact: true }).click();
    await expect(automations.getByTestId("automation-monitor")).toBeVisible();
    await surfaceTab(page, "automations").click({ button: "right" });
    await page.getByRole("menuitem", { name: "Close tab", exact: true }).click();
    await expect(garden(page).getByRole("region", { name: "Automation composition" })).toBeVisible();
  });
});
