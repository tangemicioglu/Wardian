import { readFileSync } from "node:fs";
import { cwd } from "node:process";
import { describe, expect, it } from "vitest";

const appCss = readFileSync(`${cwd()}/src/styles/App.css`, "utf8");

describe("interactive cursor contract", () => {
  it("gives enabled semantic controls a pointer cursor", () => {
    expect(appCss).toMatch(
      /button:not\(:disabled\):not\(\[aria-disabled="true"\]\)[\s\S]*?\[role="switch"\]:not\(\[aria-disabled="true"\]\)\s*\{\s*cursor:\s*pointer;/,
    );
  });

  it("leaves disabled and aria-disabled controls out of the pointer rule", () => {
    expect(appCss).toContain('button:not(:disabled):not([aria-disabled="true"])');
    expect(appCss).toContain('[role="button"]:not([aria-disabled="true"])');
  });

  it("does not make every label look actionable", () => {
    expect(appCss).not.toMatch(/^label\s*,?$/m);
  });
});
