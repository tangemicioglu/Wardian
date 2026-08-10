import { describe, expect, it } from "vitest";

import type { TerrainCell, TerrainListing } from "./terrain";
import {
  EXPAND_AREA_PX,
  MAX_LISTING_REQUESTS,
  frontierRequests,
  invalidateUnder,
  minSubdivideArea,
  type TerrainViewport,
} from "./terrainFrontier";

const VIEWPORT: TerrainViewport = {
  world: { x: 0, y: 0, width: 1000, height: 1000 },
  scale: 1,
};

function cell(overrides: Partial<TerrainCell> = {}): TerrainCell {
  return {
    path: "d:/work/repo",
    name: "repo",
    isDir: true,
    districtId: "workspace:d:/work/repo",
    depth: 0,
    rect: { x: 0, y: 0, width: 200, height: 200 },
    truncated: true,
    ...overrides,
  };
}

function listing(path: string): TerrainListing {
  return { path, children: [] };
}

describe("minSubdivideArea", () => {
  it("scales with the inverse square of zoom", () => {
    expect(minSubdivideArea(1)).toBeGreaterThan(minSubdivideArea(2));
    expect(minSubdivideArea(0.5) / minSubdivideArea(1)).toBeCloseTo(4, 6);
  });

  it("subdivides nothing at a degenerate zoom", () => {
    expect(minSubdivideArea(0)).toBe(Infinity);
    expect(minSubdivideArea(Number.NaN)).toBe(Infinity);
  });
});

describe("frontierRequests", () => {
  it("requests a large truncated directory", () => {
    expect(frontierRequests([cell()], VIEWPORT, new Map())).toEqual(["d:/work/repo"]);
  });

  it("ignores a directory already listed", () => {
    const listings = new Map([["d:/work/repo", listing("d:/work/repo")]]);
    expect(frontierRequests([cell()], VIEWPORT, listings)).toEqual([]);
  });

  it("ignores files and untruncated directories", () => {
    const cells = [
      cell({ path: "d:/a.ts", isDir: false, truncated: false }),
      cell({ path: "d:/src", truncated: false }),
    ];
    expect(frontierRequests(cells, VIEWPORT, new Map())).toEqual([]);
  });

  it("ignores a directory below the screen-area threshold", () => {
    const side = Math.sqrt(EXPAND_AREA_PX) - 4;
    const small = cell({ rect: { x: 0, y: 0, width: side, height: side } });
    expect(frontierRequests([small], VIEWPORT, new Map())).toEqual([]);
  });

  it("re-admits that directory once zoomed in", () => {
    const side = Math.sqrt(EXPAND_AREA_PX) - 4;
    const small = cell({ rect: { x: 0, y: 0, width: side, height: side } });
    expect(frontierRequests([small], { ...VIEWPORT, scale: 2 }, new Map())).toEqual([
      "d:/work/repo",
    ]);
  });

  it("ignores a directory outside the viewport", () => {
    const offscreen = cell({ rect: { x: 5000, y: 5000, width: 200, height: 200 } });
    expect(frontierRequests([offscreen], VIEWPORT, new Map())).toEqual([]);
  });

  it("spends the request budget on the largest cells first", () => {
    const cells = Array.from({ length: MAX_LISTING_REQUESTS + 10 }, (_, index) =>
      cell({
        path: `d:/work/repo/${index}`,
        rect: { x: 0, y: 0, width: 100 + index, height: 100 + index },
      }),
    );
    const requests = frontierRequests(cells, VIEWPORT, new Map());
    expect(requests).toHaveLength(MAX_LISTING_REQUESTS);
    expect(requests[0]).toBe(`d:/work/repo/${cells.length - 1}`);
  });

  it("stops requesting once the frontier budget is spent", () => {
    const listings = new Map(
      Array.from({ length: 400 }, (_, index) => [`d:/x/${index}`, listing(`d:/x/${index}`)]),
    );
    expect(frontierRequests([cell()], VIEWPORT, listings)).toEqual([]);
  });

  it("never returns the same path twice", () => {
    const duplicated = [cell(), cell()];
    expect(frontierRequests(duplicated, VIEWPORT, new Map())).toEqual(["d:/work/repo"]);
  });
});

describe("invalidateUnder", () => {
  it("drops the root and everything beneath it", () => {
    const listings = new Map([
      ["d:/work/repo", listing("d:/work/repo")],
      ["d:/work/repo/src", listing("d:/work/repo/src")],
      ["d:/work/other", listing("d:/work/other")],
    ]);
    const next = invalidateUnder(listings, "d:/work/repo");
    expect([...next.keys()]).toEqual(["d:/work/other"]);
  });

  it("does not drop a sibling with a shared prefix", () => {
    const listings = new Map([
      ["d:/work/repo", listing("d:/work/repo")],
      ["d:/work/repo-two", listing("d:/work/repo-two")],
    ]);
    const next = invalidateUnder(listings, "d:/work/repo");
    expect([...next.keys()]).toEqual(["d:/work/repo-two"]);
  });

  it("leaves the input untouched", () => {
    const listings = new Map([["d:/work/repo", listing("d:/work/repo")]]);
    invalidateUnder(listings, "d:/work/repo");
    expect(listings.size).toBe(1);
  });
});
