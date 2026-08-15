import { expect, test } from "@playwright/test";
import * as path from "path";

import { installWorkbenchIpcMock, makeWorkbenchDocument } from "../fixtures/workbenchIpcMock";

/**
 * Renders the Dashboard against a fleet shaped like a real habitat, and checks
 * the design intent the spec commits to rather than merely that it mounts.
 *
 * The screenshots are the PR evidence for this change, written under
 * `e2e/screenshots/dashboard-fleet-monitor/`.
 */

const SHOTS = path.join("e2e", "screenshots", "dashboard-fleet-monitor");

/** A habitat with one runaway, a spread of ordinary agents, and idle capacity. */
function fleet() {
  const spark = (peak: number, at: number) =>
    Array.from({ length: 12 }, (_, index) => (index === at ? peak : Math.round(peak * 0.18)));

  const row = (
    label: string,
    sublabel: string,
    tokensPerHour: number | null,
    turnsPerHour: number,
    files: number,
    added: number,
    removed: number,
    peakAt: number,
  ) => ({
    key: `uuid-${label}`,
    label,
    sublabel,
    tokens_per_hour: tokensPerHour,
    turns_per_hour: turnsPerHour,
    active_ms: Math.round(turnsPerHour * 90_000),
    turns: Math.round(turnsPerHour),
    total_tokens: tokensPerHour === null ? null : Math.round(tokensPerHour),
    files_touched: files,
    lines_added: added,
    lines_removed: removed,
    tokens_reported: tokensPerHour !== null,
    idle: false,
    spark: spark(tokensPerHour === null ? turnsPerHour : tokensPerHour, peakAt),
  });

  const idle = (label: string, sublabel: string) => ({
    key: `uuid-${label}`,
    label,
    sublabel,
    tokens_per_hour: 0,
    turns_per_hour: 0,
    active_ms: 0,
    turns: 0,
    total_tokens: 0,
    files_touched: 0,
    lines_added: 0,
    lines_removed: 0,
    tokens_reported: true,
    idle: true,
    spark: Array.from({ length: 12 }, () => 0),
  });

  return {
    window: {
      from: "2026-08-14T23:00:00.000Z",
      to: "2026-08-15T00:00:00.000Z",
      from_floored: false,
    },
    window_minutes: 60,
    rows: [
      // The runaway: burning the most tokens in the fleet and touching no files.
      row("Wardian-Arch", "Architect", 486_000, 41, 0, 0, 0, 9),
      row("KiCad-IPC-CLI", "Coder", 212_400, 28, 14, 612, 190, 6),
      row("White-Collar", "Coder", 148_000, 22, 9, 431, 77, 4),
      row("RestTrace", "Embedded Systems Engineer", 96_500, 17, 6, 214, 58, 7),
      row("BionicFace-PCB", "Electrical Engineer", 61_200, 11, 4, 96, 12, 2),
      // Antigravity reports no token accounting at all.
      row("Assistant", "Generalist", null, 14, 5, 132, 24, 5),
      row("Librarian", "Researcher", 18_400, 4, 1, 8, 0, 3),
      idle("Evolver", "Evolver"),
      idle("Discord-Wardian", "Communicator"),
      idle("Sierra", "Coder"),
    ],
    maxima: {
      tokens_per_hour: 486_000,
      turns_per_hour: 41,
      turns: 41,
      active_ms: 3_690_000,
      total_tokens: 486_000,
      files_touched: 14,
      lines: 802,
      spark: 486_000,
    },
    buckets: Array.from({ length: 12 }, (_, index) =>
      new Date(Date.UTC(2026, 7, 14, 23, index * 5)).toISOString(),
    ),
    trend_measure: "total_tokens",
    grain: "minute5",
  };
}

async function openDashboard(page: import("@playwright/test").Page) {
  const document = makeWorkbenchDocument();
  await installWorkbenchIpcMock(page, {
    load_result: {
      source: "primary",
      document,
      notice: null,
      durable_revision: document.revision,
      durable_token: "mock-token",
    },
    responses: {
      telemetry_fleet: fleet(),
      load_dashboard_prefs: null,
      telemetry_refresh: { advanced: 0 },
    },
  });
  await page.goto("/");
  await page.getByText("Review habitat telemetry.").click();
  await expect(page.getByText("Wardian-Arch")).toBeVisible({ timeout: 20_000 });
}

test.describe("Dashboard fleet monitor", () => {
  test("renders rates, mini-visuals and available capacity", async ({ page }) => {
    await openDashboard(page);

    // Totals by default; the rate view is a column, not the only denomination.
    await expect(page.getByText("486.0k")).toBeVisible();
    await expect(page.getByText(/rates over the trailing/)).toHaveCount(0);

    // Idle agents are capacity, not dead weight to hide.
    await expect(page.getByText(/Available capacity \(3\)/)).toBeVisible();

    // A provider with no token accounting reads as unmeasured, never as zero.
    const assistant = page.locator('[role="row"]', { hasText: "Assistant" }).first();
    await expect(assistant.getByText("—").first()).toBeVisible();

    await page.screenshot({
      path: path.join(SHOTS, "fleet-default.png"),
      fullPage: false,
    });
  });

  test("scales every bar against the fleet, so the runaway stands out", async ({ page }) => {
    await openDashboard(page);

    // Wardian-Arch is the fleet maximum, so its token bar is full width while a
    // quieter agent's is proportionally short. Per-row scaling would draw both
    // the same and hide exactly the agent worth noticing.
    const widths = await page.evaluate(() => {
      const read = (name: string) => {
        const row = Array.from(document.querySelectorAll('[role="row"]')).find((node) =>
          node.textContent?.includes(name),
        );
        const bar = row?.querySelector('span[style*="width"]') as HTMLElement | undefined;
        return bar?.style.width ?? null;
      };
      return { runaway: read("Wardian-Arch"), quiet: read("Librarian") };
    });

    expect(widths.runaway).toBe("100%");
    expect(Number.parseFloat(widths.quiet ?? "0")).toBeLessThan(10);
  });

  test("offers columns and keeps the default set opinionated", async ({ page }) => {
    await openDashboard(page);

    // CPU and memory exist but are off, per the spec's default.
    await expect(page.getByRole("button", { name: "Sort by CPU" })).toHaveCount(0);
    await page.getByRole("button", { name: /Columns/ }).click();

    // Scoped to the picker: a bare label would also match the column header's
    // "Sort by Turns/hr" aria-label.
    const picker = page.locator(".dashboard-view__picker");
    await expect(picker.getByLabel("CPU")).not.toBeChecked();
    await expect(picker.getByLabel("Memory")).not.toBeChecked();
    await expect(picker.getByLabel("Turns", { exact: true })).toBeChecked();
    await expect(picker.getByLabel("Tokens", { exact: true })).toBeChecked();

    // The rate view is available and off, and each rate column is named for the
    // unit it carries rather than for a judgment about it.
    await expect(picker.getByLabel("Turns/hr")).not.toBeChecked();
    await expect(picker.getByLabel("Tokens/hr")).not.toBeChecked();
    await expect(picker.getByText("Burn")).toHaveCount(0);

    await page.screenshot({
      path: path.join(SHOTS, "fleet-columns.png"),
      fullPage: false,
    });
  });
});

/** A week of six-hourly columns, the grain that used to render as `20 20 20 20`. */
function matrix() {
  const buckets = Array.from({ length: 28 }, (_, index) =>
    new Date(Date.UTC(2026, 7, 8, 0) + index * 6 * 3_600_000).toISOString(),
  );
  const row = (key: string, label: string, sublabel: string, peak: number) => ({
    key,
    label,
    sublabel,
    cells: buckets.map((_, index) =>
      index % 5 === 0 ? peak : Math.round(peak * (0.05 + (index % 4) * 0.12)),
    ),
    total: peak * 6,
  });
  return {
    dimension: "agent",
    measure: "active_ms",
    grain: "six_hour",
    window: {
      from: "2026-08-08T00:00:00.000Z",
      to: "2026-08-15T00:00:00.000Z",
      from_floored: false,
    },
    buckets,
    rows: [
      row("a", "KiCad-IPC-CLI", "Coder", 14_400_000),
      row("b", "Wardian-Claude", "Architect", 9_600_000),
      row("c", "Wardian-Codex-2", "Coder", 6_000_000),
      row("d", "RestTrace", "Embedded Systems Engineer", 3_600_000),
      row("e", "Librarian", "Researcher", 1_200_000),
    ],
    max_cell: 14_400_000,
    cells_are_not_additive: false,
  };
}

test.describe("Dashboard scrolling", () => {
  test("scrolls the rows and pins the header, rather than clipping the fleet", async ({
    page,
  }) => {
    // A real habitat runs dozens of agents. The table used to be a flex child
    // with `overflow-hidden` inside a fixed-height surface, so it shrank below
    // its content and simply cut the list off with no way to reach the rest.
    const many = fleet();
    const base = many.rows[1];
    for (let index = 0; index < 50; index += 1) {
      many.rows.push({ ...base, key: `uuid-extra-${index}`, label: `Agent-${index}` });
    }

    const document = makeWorkbenchDocument();
    await installWorkbenchIpcMock(page, {
      load_result: {
        source: "primary",
        document,
        notice: null,
        durable_revision: document.revision,
        durable_token: "mock-token",
      },
      responses: {
        telemetry_fleet: many,
        load_dashboard_prefs: null,
        telemetry_refresh: { advanced: 0 },
      },
    });
    await page.goto("/");
    await page.getByText("Review habitat telemetry.").click();
    await expect(page.getByText("Wardian-Arch")).toBeVisible({ timeout: 20_000 });

    const table = page.locator(".dashboard-view__table");
    const metrics = await table.evaluate((node) => ({
      scrollHeight: node.scrollHeight,
      clientHeight: node.clientHeight,
      overflowY: getComputedStyle(node).overflowY,
    }));
    expect(metrics.overflowY).toBe("auto");
    expect(metrics.scrollHeight).toBeGreaterThan(metrics.clientHeight);

    // The last agent is unreachable unless the region really scrolls.
    const last = page.getByText("Agent-49");
    await last.scrollIntoViewIfNeeded();
    await expect(last).toBeVisible();

    // Column headings survive the scroll: a monitor whose headers scroll away
    // stops saying what its numbers mean halfway down the list.
    await expect(page.getByRole("button", { name: /Sort by Active/ })).toBeVisible();

    await page.screenshot({
      path: path.join(SHOTS, "fleet-scrolled.png"),
      fullPage: false,
    });
  });
});

test.describe("Analytics matrix", () => {
  test("names days on the axis and says what the shading means", async ({ page }) => {
    const document = makeWorkbenchDocument();
    await installWorkbenchIpcMock(page, {
      load_result: {
        source: "primary",
        document,
        notice: null,
        durable_revision: document.revision,
        durable_token: "mock-token",
      },
      responses: {
        telemetry_matrix: matrix(),
        telemetry_refresh: { advanced: 0 },
      },
    });
    await page.goto("/");
    await page.getByText("Open the Analytics surface.").click();
    await expect(page.getByText("KiCad-IPC-CLI")).toBeVisible({ timeout: 20_000 });

    // Dates on the columns that open a day, rather than a run of bare hours.
    const axis = page.locator(".analytics-view__axis");
    await expect(axis).toContainText("Aug 8");

    // A scale, so the square-rooted ramp has an anchor.
    await expect(page.locator(".analytics-view__scale")).toContainText("busiest");

    // No provider gauge: only codex publishes one, and its presence made the
    // surface's shape depend on which vendor the habitat happened to run.
    await expect(page.locator(".analytics-view__limits")).toHaveCount(0);

    await page.screenshot({
      path: path.join("e2e", "screenshots", "analytics-matrix", "matrix-week.png"),
      fullPage: false,
    });
  });
});
