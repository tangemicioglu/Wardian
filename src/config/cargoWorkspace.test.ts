import { describe, expect, it } from "vitest";
import workspaceCargoToml from "../../Cargo.toml?raw";
import appCargoToml from "../../src-tauri/Cargo.toml?raw";
import cliCargoToml from "../../crates/wardian-cli/Cargo.toml?raw";

const binName = (manifest: string) =>
  manifest.match(/\[\[bin\]\][\s\S]*?name\s*=\s*"([^"]+)"/)?.[1];

describe("Cargo workspace binary names", () => {
  it("keeps the desktop executable as Wardian and CLI implementation distinct", () => {
    const appBinName = binName(appCargoToml);
    const cliBinName = binName(cliCargoToml);

    expect(appBinName).toBe("Wardian");
    expect(cliBinName).toBe("wardian-cli");
    expect(appBinName?.toLowerCase()).not.toBe(cliBinName?.toLowerCase());
  });

  it("keeps direct local desktop release builds out of the incremental cache", () => {
    expect(workspaceCargoToml).toMatch(
      /\[profile\.release\]\s+incremental\s*=\s*false/,
    );
  });
});
