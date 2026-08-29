import { expect, test } from "@playwright/test";
import { mkdirSync } from "node:fs";
import path from "node:path";
import {
  installWorkbenchIpcMock,
  makeWorkbenchDocument,
  makeWorkbenchSurface,
} from "../fixtures/workbenchIpcMock";

test("shows an explicit notice when the automation catalog is bounded", async ({ page }, testInfo) => {
  const document = makeWorkbenchDocument({
    surfaces: [makeWorkbenchSurface("automations-surface", "automations")],
  });
  await installWorkbenchIpcMock(page, {
    load_result: {
      source: "primary",
      document,
      notice: null,
      durable_revision: document.revision,
      durable_token: `mock-token-${document.revision}`,
    },
    responses: {
      automation_list_blueprints: {
        blueprints: [{ id: "wf", name: "Example automation", path: "/automations/example.md" }],
        truncated: true,
        next_offset: 500,
      },
      automation_list_runs: { runs: [], truncated: false },
    },
  });

  await page.goto("/", { waitUntil: "domcontentloaded" });
  await expect(page.getByTestId("automations-view")).toBeVisible();
  await expect(page.getByTestId("blueprint-selector").getByRole("status")).toContainText("Showing the first 500 automations");
  await expect(page.getByTestId("blueprint-selector").getByRole("button", { name: "Load next 500" })).toBeVisible();

  const screenshotPath = process.env.WARDIAN_BOUNDED_RESOURCE_SCREENSHOT
    ?? testInfo.outputPath("automation-catalog-truncated.png");
  mkdirSync(path.dirname(screenshotPath), { recursive: true });
  await page.getByTestId("blueprint-selector").screenshot({ path: screenshotPath, animations: "disabled" });
  await testInfo.attach("automation-catalog-truncated", { path: screenshotPath, contentType: "image/png" });
});
