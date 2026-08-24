/**
 * Watchlist drag ergonomics E2E.
 *
 * Browser-level layer: proves the pointer-driven reorder affordances that need
 * real layout and a real animation frame loop — the press/drag threshold and
 * edge auto-scroll. Reorder persistence and PTY behavior belong to the native
 * harness and are not asserted here.
 */

import { expect, test } from "@playwright/test";
import { existsSync, mkdirSync } from "node:fs";
import path from "node:path";

import { installWorkbenchIpcMock, type WorkbenchAgentFixture } from "../fixtures/workbenchIpcMock";

const ROOT = "/workspace";
const AGENT_COUNT = 40;

function rosterFixture(): WorkbenchAgentFixture[] {
  return Array.from({ length: AGENT_COUNT }, (_, index) => ({
    session_id: `agent-${String(index + 1).padStart(3, "0")}`,
    session_name: `Roster Agent ${String(index + 1).padStart(2, "0")}`,
    agent_class: "Coder",
    folder: ROOT,
    provider: "mock",
    is_off: false,
  }));
}

function screenshotDir(): string {
  const now = new Date();
  const pad = (value: number) => String(value).padStart(2, "0");
  const stamp = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
  const dir = path.join("e2e", "screenshots", "watchlist-drag", stamp);
  if (!existsSync(dir)) mkdirSync(dir, { recursive: true });
  return dir;
}

test.describe("Watchlist drag ergonomics", () => {
  test.beforeEach(async ({ page }) => {
    await installWorkbenchIpcMock(page, { agents: rosterFixture(), explorer_root: ROOT });
    await page.goto("/", { waitUntil: "domcontentloaded" });
    await page.locator('[data-testid="app-shell"]').waitFor({ timeout: 15_000 });
  });

  test("a press without movement never enters the dragging state", async ({ page }) => {
    const firstRow = page.locator('[data-testid="agent-watchlist"] .watchlist-row').first();
    await expect(firstRow).toBeVisible();
    const rowBox = await firstRow.boundingBox();
    if (!rowBox) throw new Error("watchlist row has no bounding box");

    await page.mouse.move(rowBox.x + rowBox.width / 2, rowBox.y + rowBox.height / 2);
    await page.mouse.down();

    await expect(page.getByTestId("watchlist-scroll")).not.toHaveClass(/watchlist-dragging/);
    await expect(firstRow).not.toHaveClass(/opacity-50/);

    await page.mouse.up();
  });

  test("dragging to the bottom edge scrolls the roster, and back to the top reverses it", async ({ page }) => {
    const scroller = page.getByTestId("watchlist-scroll");
    await expect(scroller).toBeVisible();

    const overflow = await scroller.evaluate((el) => el.scrollHeight - el.clientHeight);
    expect(overflow, "roster fixture must overflow the panel for this test to mean anything").toBeGreaterThan(200);

    const roster = page.locator('[data-testid="agent-watchlist"]');
    const shots = screenshotDir();
    await roster.screenshot({ path: path.join(shots, "roster-before-drag.png"), animations: "disabled" });

    const scrollerBox = await scroller.boundingBox();
    const firstRow = page.locator('[data-testid="agent-watchlist"] .watchlist-row').first();
    const rowBox = await firstRow.boundingBox();
    if (!scrollerBox || !rowBox) throw new Error("watchlist geometry unavailable");

    const pointerX = rowBox.x + rowBox.width / 2;
    await page.mouse.move(pointerX, rowBox.y + rowBox.height / 2);
    await page.mouse.down();

    // Park the pointer inside the bottom hot zone and hold it there.
    await page.mouse.move(pointerX, scrollerBox.y + scrollerBox.height - 8, { steps: 12 });
    await expect(scroller).toHaveClass(/watchlist-dragging/);
    await expect
      .poll(async () => scroller.evaluate((el) => el.scrollTop), { timeout: 5_000 })
      .toBeGreaterThan(150);

    // Rows sliding under a parked cursor must keep the drop indicator current.
    await expect(page.locator('[data-testid="agent-watchlist"] .watchlist-row[class*="drag-over-"]')).toHaveCount(1);

    await roster.screenshot({
      path: path.join(shots, "roster-auto-scrolled-mid-drag.png"),
      animations: "disabled",
    });

    const scrolled = await scroller.evaluate((el) => el.scrollTop);

    // Reversing to the top hot zone scrolls back the other way.
    await page.mouse.move(pointerX, scrollerBox.y + 6, { steps: 12 });
    await expect
      .poll(async () => scroller.evaluate((el) => el.scrollTop), { timeout: 5_000 })
      .toBeLessThan(scrolled - 100);

    await page.mouse.up();
    await expect(scroller).not.toHaveClass(/watchlist-dragging/);
  });
});
