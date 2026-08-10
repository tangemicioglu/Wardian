import { describe, expect, it } from "vitest";

import {
  MIN_GROUND_RADIUS,
  area,
  basename,
  buildTerrain,
  intersectsDisc,
  squarify,
  type TerrainDistrict,
  type TerrainListing,
} from "./terrain";

const RECT = { x: 0, y: 0, width: 200, height: 100 };

function district(overrides: Partial<TerrainDistrict> = {}): TerrainDistrict {
  return {
    roots: ["d:/work/repo"],
    origin: { x: 0, y: 0 },
    radius: 400,
    ...overrides,
  };
}

function listing(path: string, names: readonly string[], dirs: readonly string[] = []): TerrainListing {
  return {
    path,
    children: names.map((name) => ({
      name,
      path: `${path}/${name}`,
      isDir: dirs.includes(name),
      extension: null,
    })),
  };
}

describe("squarify", () => {
  it("fills the rect exactly", () => {
    const items = [1, 1, 1, 1, 1].map((value, index) => ({ value, datum: index }));
    const placed = squarify(items, RECT);
    expect(placed).toHaveLength(5);
    const total = placed.reduce((sum, entry) => sum + area(entry.rect), 0);
    expect(total).toBeCloseTo(area(RECT), 6);
  });

  it("keeps every cell inside the rect", () => {
    const items = Array.from({ length: 17 }, (_, index) => ({ value: 1, datum: index }));
    for (const { rect } of squarify(items, RECT)) {
      expect(rect.x).toBeGreaterThanOrEqual(RECT.x - 1e-9);
      expect(rect.y).toBeGreaterThanOrEqual(RECT.y - 1e-9);
      expect(rect.x + rect.width).toBeLessThanOrEqual(RECT.x + RECT.width + 1e-9);
      expect(rect.y + rect.height).toBeLessThanOrEqual(RECT.y + RECT.height + 1e-9);
    }
  });

  it("preserves input order", () => {
    const items = ["a", "b", "c", "d"].map((datum) => ({ value: 1, datum }));
    expect(squarify(items, RECT).map((entry) => entry.datum)).toEqual(["a", "b", "c", "d"]);
  });

  it("beats slice-and-dice on aspect ratio for equal weights", () => {
    const items = Array.from({ length: 12 }, (_, index) => ({ value: 1, datum: index }));
    const worst = Math.max(
      ...squarify(items, { x: 0, y: 0, width: 240, height: 240 }).map((entry) =>
        Math.max(entry.rect.width / entry.rect.height, entry.rect.height / entry.rect.width),
      ),
    );
    // Slice-and-dice on the same input yields 12:1. Anything near square is the
    // point of using squarified at all.
    expect(worst).toBeLessThan(3);
  });

  it("returns nothing for a degenerate rect or empty input", () => {
    expect(squarify([{ value: 1, datum: 0 }], { x: 0, y: 0, width: 0, height: 10 })).toEqual([]);
    expect(squarify([], RECT)).toEqual([]);
  });
});

describe("buildTerrain", () => {
  const districts = new Map([["workspace:d:/work/repo", district()]]);

  it("draws one cell per root and marks it truncated without a listing", () => {
    const cells = buildTerrain({
      districts,
      listings: new Map(),
      minSubdivideArea: 0,
      maxCells: 100,
    });
    expect(cells).toHaveLength(1);
    expect(cells[0]).toMatchObject({
      path: "d:/work/repo",
      name: "repo",
      depth: 0,
      isDir: true,
      truncated: true,
    });
  });

  it("subdivides a listed root and un-truncates it", () => {
    const cells = buildTerrain({
      districts,
      listings: new Map([["d:/work/repo", listing("d:/work/repo", ["src", "readme.md"], ["src"])]]),
      minSubdivideArea: 0,
      maxCells: 100,
    });
    expect(cells.find((cell) => cell.depth === 0)?.truncated).toBe(false);
    expect(cells.filter((cell) => cell.depth === 1).map((cell) => cell.name).sort()).toEqual([
      "readme.md",
      "src",
    ]);
    // A file has no children, so it is never truncated; a directory without a
    // listing is.
    expect(cells.find((cell) => cell.name === "readme.md")?.truncated).toBe(false);
    expect(cells.find((cell) => cell.name === "src")?.truncated).toBe(true);
  });

  it("does not subdivide below the area threshold", () => {
    const listings = new Map([
      ["d:/work/repo", listing("d:/work/repo", ["src", "readme.md"], ["src"])],
    ]);
    const cells = buildTerrain({
      districts,
      listings,
      minSubdivideArea: Number.MAX_SAFE_INTEGER,
      maxCells: 100,
    });
    expect(cells).toHaveLength(1);
    expect(cells[0].truncated).toBe(true);
  });

  it("gives a drawn cell the same rect at every detail threshold", () => {
    // The stability claim this whole design rests on: zoom changes which levels
    // are drawn, never where a drawn cell sits.
    const listings = new Map([
      ["d:/work/repo", listing("d:/work/repo", ["src", "docs"], ["src", "docs"])],
      ["d:/work/repo/src", listing("d:/work/repo/src", ["a.ts", "b.ts"])],
    ]);
    // Sits between the root cell's area and its children's, so the coarse pass
    // draws one level fewer than the fine one.
    const coarse = buildTerrain({ districts, listings, minSubdivideArea: 400_000, maxCells: 500 });
    const fine = buildTerrain({ districts, listings, minSubdivideArea: 0, maxCells: 500 });

    expect(fine.length).toBeGreaterThan(coarse.length);
    for (const cell of coarse) {
      const same = fine.find((candidate) => candidate.path === cell.path);
      expect(same, cell.path).toBeDefined();
      expect(same?.rect).toEqual(cell.rect);
    }
  });

  it("drops the deepest level whole when the budget binds", () => {
    const listings = new Map([
      ["d:/work/repo", listing("d:/work/repo", ["a", "b", "c"], ["a", "b", "c"])],
      ["d:/work/repo/a", listing("d:/work/repo/a", ["1", "2"])],
      ["d:/work/repo/b", listing("d:/work/repo/b", ["3", "4"])],
      ["d:/work/repo/c", listing("d:/work/repo/c", ["5", "6"])],
    ]);
    // Root plus level 1 is 4 cells; level 2 would add 6 more.
    const cells = buildTerrain({ districts, listings, minSubdivideArea: 0, maxCells: 8 });
    expect(cells).toHaveLength(4);
    expect(cells.every((cell) => cell.depth <= 1)).toBe(true);
    // Parents keep their truncation marker, so the map says "more here" rather
    // than asserting three empty folders.
    expect(cells.filter((cell) => cell.depth === 1).every((cell) => cell.truncated)).toBe(true);
  });

  it("lays several roots out as the top level of one district", () => {
    const cells = buildTerrain({
      districts: new Map([
        ["team:alpha", district({ roots: ["d:/work/one", "d:/work/two"] })],
      ]),
      listings: new Map(),
      minSubdivideArea: 0,
      maxCells: 100,
    });
    expect(cells.map((cell) => cell.name).sort()).toEqual(["one", "two"]);
    expect(cells.every((cell) => cell.districtId === "team:alpha")).toBe(true);
  });

  it("is deterministic across district and root ordering", () => {
    const first = buildTerrain({
      districts: new Map([
        ["b", district({ roots: ["d:/b", "d:/a"], origin: { x: 900, y: 0 } })],
        ["a", district({ roots: ["d:/a"] })],
      ]),
      listings: new Map(),
      minSubdivideArea: 0,
      maxCells: 100,
    });
    const second = buildTerrain({
      districts: new Map([
        ["a", district({ roots: ["d:/a"] })],
        ["b", district({ roots: ["d:/a", "d:/b"], origin: { x: 900, y: 0 } })],
      ]),
      listings: new Map(),
      minSubdivideArea: 0,
      maxCells: 100,
    });
    expect(first).toEqual(second);
  });

  it("floors a collapsed district to a plot worth drawing", () => {
    const cells = buildTerrain({
      districts: new Map([["workspace:d:/solo", district({ radius: 0, roots: ["d:/solo"] })]]),
      listings: new Map(),
      minSubdivideArea: 0,
      maxCells: 100,
    });
    expect(cells[0].rect.width).toBe(MIN_GROUND_RADIUS * 2);
  });

  it("skips a district with no roots", () => {
    const cells = buildTerrain({
      districts: new Map([["commons:shared", district({ roots: [] })]]),
      listings: new Map(),
      minSubdivideArea: 0,
      maxCells: 100,
    });
    expect(cells).toEqual([]);
  });
});

describe("intersectsDisc", () => {
  const disc = { origin: { x: 0, y: 0 }, radius: 10 };

  it("keeps a rect that reaches the disc", () => {
    expect(intersectsDisc({ x: 5, y: 5, width: 10, height: 10 }, disc)).toBe(true);
  });

  it("drops a corner rect entirely outside it", () => {
    expect(intersectsDisc({ x: 20, y: 20, width: 5, height: 5 }, disc)).toBe(false);
  });
});

describe("basename", () => {
  it("takes the trailing segment", () => {
    expect(basename("d:/work/repo")).toBe("repo");
    expect(basename("d:/work/repo/")).toBe("repo");
  });

  it("keeps a root path whole rather than labelling it with nothing", () => {
    expect(basename("d:")).toBe("d:");
    expect(basename("/")).toBe("/");
  });
});
