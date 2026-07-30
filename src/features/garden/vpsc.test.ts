import { describe, expect, it } from "vitest";
import {
  DEFAULT_UNIT_PADDING,
  overlaps,
  removeOverlaps,
  solveAxis,
  type UnitBox,
} from "./vpsc";

function unit(key: string, x: number, y: number, width = 40, height = 40, weight?: number): UnitBox {
  return { key, position: { x, y }, width, height, weight };
}

describe("solveAxis", () => {
  it("leaves a satisfied system exactly where it is", () => {
    const solved = solveAxis(
      new Map([
        ["a", 0],
        ["b", 100],
      ]),
      new Map(),
      [{ left: "a", right: "b", gap: 50 }],
    );
    expect(solved.get("a")).toBe(0);
    expect(solved.get("b")).toBe(100);
  });

  it("satisfies a violated constraint by splitting the correction evenly", () => {
    // Equal weights, so minimal displacement moves each by half the deficit.
    const solved = solveAxis(
      new Map([
        ["a", 0],
        ["b", 10],
      ]),
      new Map(),
      [{ left: "a", right: "b", gap: 50 }],
    );
    expect(solved.get("b")! - solved.get("a")!).toBeCloseTo(50, 9);
    expect(solved.get("a")).toBeCloseTo(-20, 9);
    expect(solved.get("b")).toBeCloseTo(30, 9);
  });

  it("preserves the centre of mass, so a group does not drift", () => {
    const desired = new Map([
      ["a", 0],
      ["b", 5],
      ["c", 10],
    ]);
    const solved = solveAxis(desired, new Map(), [
      { left: "a", right: "b", gap: 40 },
      { left: "b", right: "c", gap: 40 },
    ]);
    const before = [...desired.values()].reduce((sum, value) => sum + value, 0) / 3;
    const after = [...solved.values()].reduce((sum, value) => sum + value, 0) / 3;
    expect(after).toBeCloseTo(before, 9);
  });

  it("moves the lighter variable further", () => {
    const solved = solveAxis(
      new Map([
        ["heavy", 0],
        ["light", 10],
      ]),
      new Map([
        ["heavy", 100],
        ["light", 1],
      ]),
      [{ left: "heavy", right: "light", gap: 50 }],
    );
    expect(Math.abs(solved.get("heavy")! - 0)).toBeLessThan(1);
    expect(Math.abs(solved.get("light")! - 10)).toBeGreaterThan(35);
  });

  it("holds a pinned variable in place and routes the correction around it", () => {
    const solved = solveAxis(
      new Map([
        ["pinned", 0],
        ["free", 10],
      ]),
      new Map([["pinned", Infinity]]),
      [{ left: "pinned", right: "free", gap: 50 }],
    );
    expect(solved.get("pinned")).toBeCloseTo(0, 4);
    expect(solved.get("free")).toBeCloseTo(50, 4);
  });

  it("resolves a chain transitively", () => {
    const solved = solveAxis(
      new Map([
        ["a", 0],
        ["b", 1],
        ["c", 2],
        ["d", 3],
      ]),
      new Map(),
      [
        { left: "a", right: "b", gap: 30 },
        { left: "b", right: "c", gap: 30 },
        { left: "c", right: "d", gap: 30 },
      ],
    );
    const values = ["a", "b", "c", "d"].map((key) => solved.get(key)!);
    for (let i = 1; i < values.length; i += 1) {
      expect(values[i] - values[i - 1]).toBeGreaterThanOrEqual(30 - 1e-9);
    }
  });

  it("is deterministic under constraint reordering", () => {
    const desired = new Map([
      ["a", 0],
      ["b", 5],
      ["c", 7],
    ]);
    const constraints = [
      { left: "a", right: "b", gap: 40 },
      { left: "b", right: "c", gap: 40 },
    ];
    const forward = solveAxis(desired, new Map(), constraints);
    const reversed = solveAxis(desired, new Map(), [...constraints].reverse());
    expect([...forward.entries()].sort()).toEqual([...reversed.entries()].sort());
  });

  it("terminates on contradictory constraints instead of hanging", () => {
    // "a left of b" and "b left of a" cannot both hold. removeOverlaps orients
    // every constraint from one fixed ranking so this cannot arise there, but a
    // solver that loops forever on an infeasible input is a latent hang.
    const solved = solveAxis(
      new Map([
        ["a", 0],
        ["b", 0],
      ]),
      new Map(),
      [
        { left: "a", right: "b", gap: 50 },
        { left: "b", right: "a", gap: 50 },
      ],
    );
    expect(Number.isFinite(solved.get("a")!)).toBe(true);
    expect(Number.isFinite(solved.get("b")!)).toBe(true);
  });

  it("ignores constraints referencing unknown or identical variables", () => {
    const solved = solveAxis(new Map([["a", 0]]), new Map(), [
      { left: "a", right: "ghost", gap: 50 },
      { left: "a", right: "a", gap: 50 },
    ]);
    expect(solved.get("a")).toBe(0);
  });
});

describe("removeOverlaps", () => {
  it("separates overlapping units of different sizes along the cheaper axis", () => {
    // A wide, short unit beside a small one: correcting vertically costs far
    // less than clearing the full combined width, so that is the axis used.
    const units = [unit("small", 0, 0, 20, 20), unit("large", 10, 0, 120, 60)];
    const { positions, residualOverlaps } = removeOverlaps(units);
    const separationY = Math.abs(positions.get("small")!.y - positions.get("large")!.y);
    expect(separationY).toBeGreaterThanOrEqual((20 + 60) / 2 + DEFAULT_UNIT_PADDING - 1e-6);
    expect(residualOverlaps).toEqual([]);
    const resolved = units.map((item) => ({ ...item, position: positions.get(item.key)! }));
    expect(overlaps(resolved[0], resolved[1])).toBe(false);
  });

  it("preserves order, so a longer label never reshuffles the map", () => {
    // The property that separates a map from a canvas: growing a unit must not
    // swap it past its neighbour.
    const before = [unit("a", 0, 0), unit("b", 30, 0), unit("c", 60, 0)];
    const { positions } = removeOverlaps(before);
    expect(positions.get("a")!.x).toBeLessThan(positions.get("b")!.x);
    expect(positions.get("b")!.x).toBeLessThan(positions.get("c")!.x);

    // Same layout, but "b" gets much wider.
    const widened = [unit("a", 0, 0), unit("b", 30, 0, 200, 40), unit("c", 60, 0)];
    const after = removeOverlaps(widened).positions;
    expect(after.get("a")!.x).toBeLessThan(after.get("b")!.x);
    expect(after.get("b")!.x).toBeLessThan(after.get("c")!.x);
  });

  it("separates a vertical stack vertically rather than smearing it into a row", () => {
    // Each pair is routed to the cheaper axis, so a column stays a column.
    const units = [unit("a", 0, 0), unit("b", 0, 12), unit("c", 0, 24)];
    const { positions } = removeOverlaps(units);
    const xs = ["a", "b", "c"].map((key) => positions.get(key)!.x);
    expect(Math.max(...xs) - Math.min(...xs)).toBeLessThan(1);
    expect(positions.get("a")!.y).toBeLessThan(positions.get("b")!.y);
    expect(positions.get("b")!.y).toBeLessThan(positions.get("c")!.y);
  });

  it("leaves an already-clear layout untouched", () => {
    const units = [unit("a", 0, 0), unit("b", 500, 0), unit("c", 0, 500)];
    const { positions, residualOverlaps } = removeOverlaps(units);
    for (const item of units) {
      expect(positions.get(item.key)).toEqual(item.position);
    }
    expect(residualOverlaps).toEqual([]);
  });

  it("holds a pinned unit exactly in place", () => {
    const units = [unit("pinned", 0, 0, 40, 40, Infinity), unit("free", 5, 0)];
    const { positions } = removeOverlaps(units);
    expect(positions.get("pinned")!.x).toBeCloseTo(0, 3);
    expect(positions.get("pinned")!.y).toBeCloseTo(0, 3);
    expect(Math.abs(positions.get("free")!.x)).toBeGreaterThan(40);
  });

  it("clears a fully coincident pile", () => {
    const units = Array.from({ length: 6 }, (_, i) => unit(`n${i}`, 0, 0));
    const { positions, residualOverlaps } = removeOverlaps(units);
    expect(residualOverlaps).toEqual([]);
    const resolved = units.map((item) => ({
      ...item,
      position: positions.get(item.key)!,
    }));
    for (let i = 0; i < resolved.length; i += 1) {
      for (let j = i + 1; j < resolved.length; j += 1) {
        expect(overlaps(resolved[i], resolved[j])).toBe(false);
      }
    }
  });

  it("clears a dense grid of mixed footprints", () => {
    const units: UnitBox[] = [];
    for (let row = 0; row < 8; row += 1) {
      for (let column = 0; column < 8; column += 1) {
        units.push(
          unit(
            `n-${row}-${column}`,
            column * 18,
            row * 18,
            20 + ((row * 8 + column) % 5) * 24,
            20 + ((row + column) % 3) * 18,
          ),
        );
      }
    }
    const { positions, residualOverlaps, rounds } = removeOverlaps(units);
    expect(residualOverlaps).toEqual([]);
    expect(rounds).toBeLessThanOrEqual(4);
    const resolved = units.map((item) => ({ ...item, position: positions.get(item.key)! }));
    for (let i = 0; i < resolved.length; i += 1) {
      for (let j = i + 1; j < resolved.length; j += 1) {
        expect(overlaps(resolved[i], resolved[j])).toBe(false);
      }
    }
  });

  it("takes a second round when separating one pair collides with a third unit", () => {
    // A and B overlap; C is clear of B. Separating A from B pushes B into C,
    // which was not in the first round's constraint set. This is why the loop
    // exists at all.
    const units = [unit("a", 0, 0), unit("b", 20, 0), unit("c", 76, 0)];
    const { positions, residualOverlaps, rounds } = removeOverlaps(units);
    expect(rounds).toBe(2);
    expect(residualOverlaps).toEqual([]);
    expect(positions.get("b")!.x - positions.get("a")!.x).toBeGreaterThanOrEqual(54 - 1e-6);
    expect(positions.get("c")!.x - positions.get("b")!.x).toBeGreaterThanOrEqual(54 - 1e-6);
  });

  it("is deterministic under input reordering", () => {
    const units = [unit("a", 0, 0), unit("b", 10, 5), unit("c", 5, 10)];
    const forward = removeOverlaps(units).positions;
    const reversed = removeOverlaps([...units].reverse()).positions;
    for (const item of units) {
      expect(reversed.get(item.key)!.x).toBeCloseTo(forward.get(item.key)!.x, 9);
      expect(reversed.get(item.key)!.y).toBeCloseTo(forward.get(item.key)!.y, 9);
    }
  });

  it("handles empty input", () => {
    expect(removeOverlaps([]).positions.size).toBe(0);
  });

  it("stays within a frame budget at district scale", () => {
    const units = Array.from({ length: 80 }, (_, i) =>
      unit(`n${String(i).padStart(2, "0")}`, (i % 10) * 22, Math.floor(i / 10) * 22, 60, 30),
    );
    const started = performance.now();
    const { residualOverlaps } = removeOverlaps(units);
    expect(performance.now() - started).toBeLessThan(120);
    expect(residualOverlaps).toEqual([]);
  });
});
