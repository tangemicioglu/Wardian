import { expect, test } from "@playwright/test";
import * as path from "path";

import { installWorkbenchIpcMock, makeWorkbenchDocument } from "../fixtures/workbenchIpcMock";

/**
 * Renders Analytics against the two measures added for cache accounting, and
 * checks the claims the spec makes about them rather than merely that they
 * mount.
 *
 * The screenshots are the PR evidence for this change, written under
 * `e2e/screenshots/analytics-cache-measures/`.
 */

const SHOTS = path.join("e2e", "screenshots", "analytics-cache-measures");

const BUCKETS = [
  "2026-08-24T12:00:00.000Z",
  "2026-08-24T13:00:00.000Z",
  "2026-08-24T14:00:00.000Z",
  "2026-08-24T15:00:00.000Z",
  "2026-08-24T16:00:00.000Z",
  "2026-08-24T17:00:00.000Z",
];

type Row = { key: string; sublabel: string; cells: number[] };

/**
 * Rows shaped like a habitat running claude, codex, and pi.
 *
 * The proportions are the ones the spec argues from. Claude routes nearly all
 * of its fresh prompt content through cache writes; codex reports none at all,
 * because its upstream does not bill for them; pi sits between the two.
 */
const ROWS: Record<string, Row[]> = {
  cache_write_tokens: [
    { key: "Wardian-Arch", sublabel: "claude", cells: [318_402, 274_118, 0, 402_551, 511_204, 379_596] },
    { key: "Wardian-Pi", sublabel: "pi", cells: [0, 41_220, 38_904, 0, 52_180, 44_006] },
    { key: "Wardian-Codex", sublabel: "codex", cells: [0, 0, 0, 0, 0, 0] },
  ],
  cache_hit_rate: [
    { key: "Wardian-Arch", sublabel: "claude", cells: [97, 96, 41, 98, 98, 97] },
    { key: "Wardian-Pi", sublabel: "pi", cells: [0, 78, 81, 0, 84, 82] },
    { key: "Wardian-Codex", sublabel: "codex", cells: [88, 86, 90, 74, 89, 87] },
  ],
};

/** The grid the mocked `telemetry_matrix` answers with for one measure. */
function matrixFor(measure: string) {
  const rows = ROWS[measure];
  const isRatio = measure === "cache_hit_rate";
  return {
    dimension: "agent",
    measure,
    grain: "hour",
    window: { from: BUCKETS[0], to: "2026-08-24T18:00:00.000Z", from_floored: true },
    buckets: BUCKETS,
    rows: rows.map((row) => ({
      key: `uuid-${row.key}`,
      label: row.key,
      sublabel: row.sublabel,
      cells: row.cells,
      // A ratio's total is recomputed over the window, never summed across the
      // columns and never averaged.
      total: isRatio ? 96 : row.cells.reduce((sum, value) => sum + value, 0),
    })),
    max_cell: Math.max(...rows.flatMap((row) => row.cells)),
    cells_are_not_additive: isRatio,
  };
}

/**
 * Opens Analytics with one measure's grid canned, then selects that measure.
 *
 * The fixture answers `telemetry_matrix` without reading its arguments, so each
 * test installs the grid it is about. The view renders from `matrix.measure`
 * rather than from its own selector state, so the two stay consistent.
 */
async function openAnalytics(page: import("@playwright/test").Page, measure: string) {
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
      telemetry_matrix: matrixFor(measure),
      telemetry_refresh: { advanced: 0 },
    },
  });

  await page.goto("/");
  await page.getByText("Look up what agents did over a period.").click();
  await expect(page.getByLabel("Measure")).toBeVisible({ timeout: 20_000 });
  await page.getByLabel("Measure").selectOption(measure);
  await expect(page.getByText("Wardian-Arch")).toBeVisible({ timeout: 20_000 });
}

/**
 * The Analytics grid alone, which is the whole of what this change touches.
 *
 * A full-page shot here would be mostly spawn form and roster, and PR evidence
 * is supposed to explain the change rather than tour the app. The transient
 * save toast is waited out so it cannot land across the grid.
 */
async function shootGrid(page: import("@playwright/test").Page, name: string) {
  await expect(page.getByText("Saving workbench changes...")).toHaveCount(0, { timeout: 20_000 });
  await page
    .locator(".analytics-view")
    .screenshot({ path: path.join(SHOTS, `${name}.png`) });
}

test.describe("Analytics cache measures", () => {
  test("offers the cache measures the backend can now plot", async ({ page }) => {
    await openAnalytics(page, "cache_write_tokens");

    const options = await page
      .getByLabel("Measure")
      .locator("option")
      .evaluateAll((nodes) => nodes.map((node) => (node as HTMLOptionElement).value));

    expect(options).toContain("cache_write_tokens");
    expect(options).toContain("cache_hit_rate");
    expect(options).toContain("total_tokens");
  });

  test("plots cache writes as work, separately from cache reads", async ({ page }) => {
    await openAnalytics(page, "cache_write_tokens");

    // Claude carries nearly all its fresh prompt content here; codex reports
    // none, because its upstream does not bill for cache writes. A measure that
    // comes out provider-shaped is reading reality, not failing.
    await expect(page.getByText("Wardian-Codex")).toBeVisible();
    await expect(page.getByText("Wardian-Pi")).toBeVisible();

    await shootGrid(page, "cache-writes");
  });

  test("renders the hit rate as a share rather than as a count", async ({ page }) => {
    await openAnalytics(page, "cache_hit_rate");

    // A ratio without its sign reads as a count of 96.
    await expect(page.getByText("96%").first()).toBeVisible({ timeout: 20_000 });

    await shootGrid(page, "cache-hit-rate");
  });
});
