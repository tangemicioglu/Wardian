import { describe, expect, it } from "vitest";

import { MAX_TERRAIN_CELLS } from "./terrainFrontier";
import {
  DIR_WEIGHT,
  GROUND_GAP,
  MIN_GROUND_RADIUS,
  area,
  assignRootsToCells,
  basename,
  buildTerrain,
  groundRadiusFor,
  intersectsDisc,
  quantizeAnchor,
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

describe("assignRootsToCells", () => {
  // Four quadrant cells of a 400x400 ground centred on the origin.
  const QUADRANTS = [
    { x: -200, y: -200, width: 200, height: 200 },
    { x: 0, y: -200, width: 200, height: 200 },
    { x: -200, y: 0, width: 200, height: 200 },
    { x: 0, y: 0, width: 200, height: 200 },
  ];
  const ORIGIN = { x: 0, y: 0 };

  it("puts each root under the agents that work in it", () => {
    // Sorted order would give `alpha` the top-left cell, but its agents settled
    // bottom-right — which is how an agent ends up sitting over a repository it
    // has never touched.
    const roots = ["alpha", "beta", "gamma", "delta"].sort();
    const anchors = new Map([
      ["alpha", { x: 100, y: 100 }],
      ["beta", { x: -100, y: 100 }],
      ["gamma", { x: 100, y: -100 }],
      ["delta", { x: -100, y: -100 }],
    ]);
    expect(assignRootsToCells(roots, QUADRANTS, anchors, ORIGIN)).toEqual([
      "delta",
      "gamma",
      "beta",
      "alpha",
    ]);
  });

  it("keeps sorted order when nothing says otherwise", () => {
    const roots = ["alpha", "beta", "delta", "gamma"];
    expect(assignRootsToCells(roots, QUADRANTS, undefined, ORIGIN)).toEqual(roots);
  });

  it("is a permutation, never a loss", () => {
    const roots = ["a", "b", "c", "d", "e", "f", "g", "h"];
    const cells = roots.map((_, index) => ({ x: index * 10, y: 0, width: 10, height: 10 }));
    const anchors = new Map(roots.map((root, index) => [root, { x: (7 - index) * 10, y: 0 }]));
    const assigned = assignRootsToCells(roots, cells, anchors, ORIGIN);
    expect([...assigned].sort()).toEqual([...roots].sort());
  });

  it("resolves against the district's own origin", () => {
    // Anchors are district-relative; cells are world-space. Comparing them in
    // one frame is the whole correctness condition.
    const roots = ["alpha", "beta"];
    const cells = [
      { x: 900, y: 0, width: 100, height: 100 },
      { x: 1000, y: 0, width: 100, height: 100 },
    ];
    const anchors = new Map([
      ["alpha", { x: 50, y: 50 }],
      ["beta", { x: -50, y: 50 }],
    ]);
    expect(assignRootsToCells(roots, cells, anchors, { x: 1000, y: 0 })).toEqual([
      "beta",
      "alpha",
    ]);
  });
});

describe("quantizeAnchor", () => {
  it("snaps so a pixel of drift cannot swap two roots' territory", () => {
    expect(quantizeAnchor({ x: 3, y: -3 })).toEqual({ x: 0, y: -0 });
    expect(quantizeAnchor({ x: 101, y: 99 })).toEqual({ x: 120, y: 80 });
  });
});

describe("groundRadiusFor", () => {
  it("inflates a collapsed district to a plot worth drawing", () => {
    expect(groundRadiusFor(0, Number.POSITIVE_INFINITY)).toBe(MIN_GROUND_RADIUS);
  });

  it("stops inflating before it reaches a neighbour", () => {
    // Two one-agent districts about 216 apart is the reported case: at the old
    // fixed floor both discs were 120 and visibly bled into each other.
    expect(groundRadiusFor(20, 216)).toBe(216 / 2 - GROUND_GAP);
    expect(groundRadiusFor(20, 216) * 2).toBeLessThan(216);
  });

  it("keeps a district that genuinely reaches further than its gap", () => {
    // The units already overlap here. Shrinking the ground would hide a layout
    // problem rather than fix one.
    expect(groundRadiusFor(400, 300)).toBe(400);
  });

  it("never inflates past the floor when there is room to spare", () => {
    expect(groundRadiusFor(10, 5000)).toBe(MIN_GROUND_RADIUS);
  });
});

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

  it("gives a folder more room than a file it sits beside", () => {
    // The reported confusion: with equal weights a loose file at the repository
    // root was drawn exactly as large as `src/`.
    const listings = new Map([
      ["d:/work/repo", listing("d:/work/repo", ["src", "readme.md"], ["src"])],
    ]);
    const cells = buildTerrain({ districts, listings, minSubdivideArea: 0, maxCells: 100 });
    const src = cells.find((cell) => cell.name === "src");
    const readme = cells.find((cell) => cell.name === "readme.md");
    expect(area(src!.rect) / area(readme!.rect)).toBeCloseTo(DIR_WEIGHT, 4);
  });

  it("opens a realistic district under a realistic roster", () => {
    // The regression a toy fixture missed entirely: four repository roots of
    // ~46 entries each need ~190 cells for their *first* level, and the cut is
    // whole-level. A per-district share below that drew four bare squares that
    // never opened at any zoom — which is what shipped, briefly.
    const roots = ["d:/dev/wardian", "d:/dev/wt-2", "d:/dev/wt-3", "d:/dev/wardian.org"];
    const listings = new Map<string, TerrainListing>();
    for (const root of roots) {
      const names = Array.from({ length: 46 }, (_, index) => `entry-${index}`);
      listings.set(root, listing(root, names, names.slice(0, 20)));
    }
    const crowded = new Map(
      Array.from({ length: 37 }, (_, index) => [
        `workspace:d${index}`,
        district({
          roots: index === 0 ? roots : [`d:/other/${index}`],
          origin: { x: index * 5000, y: 0 },
        }),
      ]),
    );

    const cells = buildTerrain({
      districts: crowded,
      listings,
      minSubdivideArea: 0,
      maxCells: MAX_TERRAIN_CELLS,
    });
    const mine = cells.filter((cell) => cell.districtId === "workspace:d0");
    // No root is left as a bare square, and each one actually opened. The count
    // falls short of 46 x 4 because cells in the ground square's corners fall
    // outside the district's disc, which is the clip working as intended.
    const openedRoots = mine.filter((cell) => cell.depth === 0 && !cell.truncated);
    expect(openedRoots.map((cell) => cell.path).sort()).toEqual([...roots].sort());
    expect(mine.filter((cell) => cell.depth === 1).length).toBeGreaterThan(150);
  });

  it("keeps a district's detail independent of what other districts ingest", () => {
    // The instability this replaced a shared budget to fix: with one pool, the
    // deepest level was a function of the *total* cell count, so a listing
    // arriving in one district deleted a level in a district on the other side
    // of the map — and the next invalidation there put it back.
    const alone = new Map([
      ["alpha:root", district({ roots: ["d:/alpha"] })],
    ]);
    const crowded = new Map([
      ["alpha:root", district({ roots: ["d:/alpha"] })],
      ["beta:root", district({ roots: ["d:/beta"], origin: { x: 5000, y: 0 } })],
    ]);
    const listings = new Map([
      ["d:/alpha", listing("d:/alpha", ["a", "b"], ["a", "b"])],
      ["d:/alpha/a", listing("d:/alpha/a", ["1", "2"])],
      ["d:/alpha/b", listing("d:/alpha/b", ["3", "4"])],
      // Beta is listed several levels deep, so a shared pool would be exhausted
      // by it and alpha would lose its second level.
      ["d:/beta", listing("d:/beta", ["x", "y", "z"], ["x", "y", "z"])],
      ["d:/beta/x", listing("d:/beta/x", ["1", "2", "3", "4"])],
      ["d:/beta/y", listing("d:/beta/y", ["5", "6", "7", "8"])],
      ["d:/beta/z", listing("d:/beta/z", ["9", "10", "11", "12"])],
    ]);

    const before = buildTerrain({ districts: alone, listings, minSubdivideArea: 0, maxCells: 14 });
    const after = buildTerrain({ districts: crowded, listings, minSubdivideArea: 0, maxCells: 14 });
    const alphaCells = (cells: ReturnType<typeof buildTerrain>) =>
      cells.filter((cell) => cell.districtId === "alpha:root");

    expect(alphaCells(after)).toEqual(alphaCells(before));
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

  it("draws the ground at exactly the radius it was given", () => {
    // The floor lives in `groundRadiusFor`, which is the only place that knows
    // how much free space there is. If the geometry re-applied it, the ground
    // could exceed the clip and folders would vanish at the district edge.
    const cells = buildTerrain({
      districts: new Map([["workspace:d:/solo", district({ radius: 40, roots: ["d:/solo"] })]]),
      listings: new Map(),
      minSubdivideArea: 0,
      maxCells: 100,
    });
    expect(cells[0].rect.width).toBe(80);
  });

  it("draws nothing for a district with no territory", () => {
    const cells = buildTerrain({
      districts: new Map([["workspace:d:/solo", district({ radius: 0, roots: ["d:/solo"] })]]),
      listings: new Map(),
      minSubdivideArea: 0,
      maxCells: 100,
    });
    expect(cells).toEqual([]);
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
