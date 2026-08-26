import { expect, test } from "@playwright/test";
import { mkdirSync } from "node:fs";
import path from "node:path";
import {
  installWorkbenchIpcMock,
  makeWorkbenchDocument,
  makeWorkbenchSurface,
} from "../fixtures/workbenchIpcMock";

test("shows an explicit notice when the workflow catalog is bounded", async ({ page }, testInfo) => {
  const document = makeWorkbenchDocument({
    surfaces: [makeWorkbenchSurface("workflows-surface", "workflows")],
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
      workflow_list_blueprints: {
        blueprints: [{ id: "wf", name: "Example workflow", path: "/workflows/example.md" }],
        truncated: true,
        next_offset: 500,
      },
      workflow_list_runs: { runs: [], truncated: false },
    },
  });

  await page.goto("/", { waitUntil: "domcontentloaded" });
  await expect(page.getByTestId("workflows-view")).toBeVisible();
  await expect(page.getByTestId("blueprint-selector").getByRole("status")).toContainText("Showing the first 500 workflows");
  await expect(page.getByTestId("blueprint-selector").getByRole("button", { name: "Load next 500" })).toBeVisible();

  const screenshotPath = process.env.WARDIAN_BOUNDED_RESOURCE_SCREENSHOT
    ?? testInfo.outputPath("workflow-catalog-truncated.png");
  mkdirSync(path.dirname(screenshotPath), { recursive: true });
  await page.getByTestId("blueprint-selector").screenshot({ path: screenshotPath, animations: "disabled" });
  await testInfo.attach("workflow-catalog-truncated", { path: screenshotPath, contentType: "image/png" });
});
