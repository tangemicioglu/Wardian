import { describe, expect, it } from "vitest";
import stageZellijScript from "../../scripts/stage-zellij.mjs?raw";

describe("stage Zellij script", () => {
  it("pins the no-web runtime and executable checksums for every release target", () => {
    expect(stageZellijScript).toContain("export const ZELLIJ_VERSION = '0.45.0'");
    expect(stageZellijScript).toContain("zellij-no-web-x86_64-pc-windows-msvc.zip");
    expect(stageZellijScript).toContain("zellij-no-web-x86_64-unknown-linux-musl.tar.gz");
    expect(stageZellijScript).toContain("zellij-no-web-aarch64-unknown-linux-musl.tar.gz");
    expect(stageZellijScript).toContain("zellij-no-web-x86_64-apple-darwin.tar.gz");
    expect(stageZellijScript).toContain("zellij-no-web-aarch64-apple-darwin.tar.gz");
    expect(stageZellijScript.match(/sha256: '[a-f0-9]{64}'/g)).toHaveLength(5);
  });

  it("verifies the extracted executable before staging it", () => {
    expect(stageZellijScript).toContain("verifyZellijExecutable(extractedExecutable, artifact)");
    expect(stageZellijScript).toContain("Refusing to stage Zellij: SHA-256 mismatch");
    expect(stageZellijScript).toContain("copyFileSync(extractedExecutable, destination)");
    expect(stageZellijScript).toContain("verifyZellijExecutable(destination, artifact)");
  });

  it("fails closed for unsupported host and cross-compilation targets", () => {
    expect(stageZellijScript).toContain("is not pinned for Rust target");
    expect(stageZellijScript).toContain("is not pinned for ${platform}/${arch}");
  });
});
