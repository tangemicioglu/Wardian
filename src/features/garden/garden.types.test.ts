import { describe, expect, it } from "vitest";
import { unitKey } from "./garden.types";

describe("unitKey", () => {
  it("namespaces by kind so agent and automation ids never collide", () => {
    expect(unitKey({ kind: "agent", id: "abc" })).toBe("agent:abc");
    expect(unitKey({ kind: "automation", id: "abc" })).toBe("automation:abc");
  });
});
