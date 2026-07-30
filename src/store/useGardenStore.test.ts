import { beforeEach, describe, expect, it } from "vitest";
import { useGardenStore } from "./useGardenStore";
import { createScene, recordPositions } from "../features/garden/gardenScene";

beforeEach(() => {
  useGardenStore.getState().reset();
  localStorage.clear();
});

describe("useGardenStore", () => {
  it("stores a pin relative to its district, not as an absolute point", () => {
    // An absolute pin strands its entity if the district's cell ever moves.
    useGardenStore.getState().pin("agent:a1", "team:hw", { x: 340, y: 220 }, { x: 300, y: 200 });
    expect(useGardenStore.getState().scene.pins["agent:a1"]).toMatchObject({
      district_id: "team:hw",
      dx: 40,
      dy: 20,
    });
  });

  it("unpins", () => {
    useGardenStore.getState().pin("agent:a1", "team:hw", { x: 1, y: 1 }, { x: 0, y: 0 });
    useGardenStore.getState().unpin("agent:a1");
    expect(useGardenStore.getState().scene.pins["agent:a1"]).toBeUndefined();
  });

  it("records an excluded district", () => {
    useGardenStore.getState().exclude("agent:a1", "team:web");
    expect(useGardenStore.getState().scene.exclusions["agent:a1"]).toEqual(["team:web"]);
  });

  it("marks a unit visited so it resists drift", () => {
    useGardenStore.getState().visit("agent:a1");
    expect(useGardenStore.getState().scene.visited["agent:a1"]).toBeGreaterThan(0);
  });

  it("adopts the scene a layout pass returned", () => {
    const settled = recordPositions(createScene(), new Map([["agent:a1", { x: 7, y: 8 }]]));
    useGardenStore.getState().adoptScene(settled);
    expect(useGardenStore.getState().scene.positions["agent:a1"]).toEqual({ x: 7, y: 8 });
  });

  it("persists the scene under wardian-garden", () => {
    useGardenStore.getState().pin("agent:a1", "team:hw", { x: 9, y: 9 }, { x: 0, y: 0 });
    expect(localStorage.getItem("wardian-garden")).toContain("agent:a1");
  });

  it("resets to a fresh scene", () => {
    useGardenStore.getState().pin("agent:a1", "team:hw", { x: 9, y: 9 }, { x: 0, y: 0 });
    useGardenStore.getState().reset();
    expect(useGardenStore.getState().scene.pins).toEqual({});
  });
});
