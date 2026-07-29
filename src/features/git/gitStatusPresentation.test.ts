import { describe, expect, it } from "vitest";
import { gitStatusColor, gitStatusLabel, gitStatusTextClass } from "./gitStatusPresentation";

describe("gitStatusPresentation", () => {
  it("keeps Explorer and Source Control on the same semantic palette", () => {
    expect(gitStatusColor("M")).toBe("var(--color-wardian-warning)");
    expect(gitStatusColor("?")).toBe("var(--color-wardian-success)");
    expect(gitStatusTextClass("D")).toBe("text-[var(--color-wardian-error)]");
    expect(gitStatusLabel("UU")).toBe("Both Modified");
  });
});
