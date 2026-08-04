import { expect, test } from "@playwright/test";
import * as fs from "node:fs";
import * as path from "node:path";

import {
  installWorkbenchIpcMock,
  makeWorkbenchDocument,
  makeWorkbenchSurface,
} from "../fixtures/workbenchIpcMock";

test("offers first-launch users an opt-in, action-gated guided setup", async ({ page }, testInfo) => {
  const overview = makeWorkbenchSurface("onboarding-overview", "agents-overview", {
    state: {
      mode: "grid",
      focused_agent_id: null,
      search_query: "",
      status_filter: [],
    },
  });
  const document = makeWorkbenchDocument({ revision: 1, surfaces: [overview] });

  await page.setViewportSize({ width: 1440, height: 960 });
  const workbench = await installWorkbenchIpcMock(page, {
    load_result: {
      source: "primary",
      document,
      notice: null,
      durable_revision: document.revision,
      durable_token: "onboarding-evidence-token",
    },
    responses: {
      load_onboarding_hints: {
        dismissed_hint_ids: ["spawn-agent-first-run:v1"],
        contextual_tips_enabled: true,
        guided_tour_state: "unseen",
      },
      set_guided_tour_state: {
        dismissed_hint_ids: ["spawn-agent-first-run:v1"],
        contextual_tips_enabled: true,
        guided_tour_state: "in_progress",
      },
    },
  });
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await expect(page.getByTestId("app-shell")).toBeVisible();

  const welcome = page.getByTestId("onboarding-welcome");
  await expect(welcome).toBeVisible();
  await expect(welcome.getByRole("button", { name: "Not now" })).toBeVisible();
  await welcome.getByRole("button", { name: "Take the tour" }).click();

  const tour = page.getByTestId("onboarding-tour");
  await expect(tour.getByRole("heading", { name: "Create an Evolver" })).toBeVisible();
  await expect(page.getByTestId("spawn-submit")).toBeVisible();

  const screenshotPath = process.env.WARDIAN_ONBOARDING_SCREENSHOT
    ?? testInfo.outputPath("action-gated-guided-setup.png");
  fs.mkdirSync(path.dirname(screenshotPath), { recursive: true });
  await page.screenshot({ path: screenshotPath, animations: "disabled" });
  await testInfo.attach("onboarding-guidance", { path: screenshotPath, contentType: "image/png" });

  await workbench.setAgents([{
    session_id: "evolver-1",
    session_name: "evolver",
    agent_class: "Evolver",
    folder: "/workspace",
    provider: "mock",
    is_off: false,
  }]);
  await expect(tour.getByRole("heading", { name: "Let the Evolver create its partner" })).toBeVisible();
  await expect(tour.getByText(/Do not create a graph connection/)).toBeVisible();
});
