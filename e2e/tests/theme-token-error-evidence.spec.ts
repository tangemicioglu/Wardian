import { mkdirSync } from "node:fs";
import path from "node:path";
import { test, expect, type Page } from "@playwright/test";

/**
 * Evidence for the error-colour token swap.
 *
 * Several surfaces spelled their danger colour as a Tailwind palette class
 * (`text-red-400`, `bg-red-900`, `bg-red-950`). Those are fixed values, so the
 * text they coloured did not respond to the theme. They now use
 * `.text-wardian-error`, which reads `--color-wardian-error`: `#dc2626` in
 * light and `#ef4444` in dark.
 *
 * The capture is a light/dark pair of the same Settings error line, because
 * responding to the theme at all is the change. The assertions below prove the
 * computed colour actually differs between themes, so the screenshot is
 * showing a real difference rather than two identical frames.
 */

const OUT_DIR = path.resolve("e2e/screenshots/theme-error-token/2026-08-28");

async function openSettings(page: Page) {
  const dialog = page.getByRole("dialog", { name: "Settings" });
  if (!(await dialog.isVisible().catch(() => false))) {
    await page.locator('[data-testid="sidebar-tab-settings"]').click();
  }
  await expect(dialog).toBeVisible();
  return dialog;
}

async function setTheme(page: Page, theme: "light" | "dark") {
  await page.evaluate((value) => {
    document.documentElement.setAttribute("data-theme", value);
  }, theme);
  await expect
    .poll(async () =>
      page.evaluate(() => document.documentElement.getAttribute("data-theme")),
    )
    .toBe(theme);
}

test("the settings error line follows the theme instead of a fixed red", async ({ page }, testInfo) => {
  mkdirSync(OUT_DIR, { recursive: true });
  await page.goto("/");

  const dialog = await openSettings(page);
  await dialog.getByRole("button", { name: "Advanced", exact: true }).click();

  // Without the Tauri bridge the settings-folder action rejects, which is the
  // path that renders the error line. That is the line this PR re-coloured.
  await dialog.getByLabel("Open settings folder").click();

  const errorLine = dialog.getByText("Unable to access settings folder.", { exact: true });
  await expect(errorLine).toBeVisible({ timeout: 10_000 });

  const colours: Record<string, string> = {};
  for (const theme of ["dark", "light"] as const) {
    await setTheme(page, theme);

    // Compare against the token rather than a literal, so changing the palette
    // does not break this test — only detaching the class from the token does.
    const { colour, token } = await errorLine.evaluate((node) => {
      const probe = document.createElement("span");
      probe.style.color = "var(--color-wardian-error)";
      document.body.appendChild(probe);
      const token = getComputedStyle(probe).color;
      probe.remove();
      return { colour: getComputedStyle(node).color, token };
    });
    expect(colour, `the ${theme} error line should resolve --color-wardian-error`).toBe(token);
    colours[theme] = colour;

    const file = path.join(OUT_DIR, `settings-error-${theme}.png`);
    await dialog.screenshot({ path: file, animations: "disabled" });
    await testInfo.attach(`settings-error-${theme}`, { path: file, contentType: "image/png" });
  }

  // The point of the change: a themed token resolves differently per theme,
  // where the previous fixed `text-red-400` could not.
  expect(colours.dark, "the error colour should differ between themes").not.toBe(colours.light);
});
