import { describe, expect, it } from "vitest";

import type { TerrainCell, TerrainListing } from "./terrain";
import {
  EXPAND_AREA_PX,
  MAX_LISTING_REQUESTS,
  frontierRequests,
  minSubdivideArea,
  staleListings,
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

describe("staleListings", () => {
  const cached = () =>
    new Map([
      ["d:/work/repo", listing("d:/work/repo")],
      ["d:/work/repo/src", listing("d:/work/repo/src")],
      ["d:/work/repo/src/deep", listing("d:/work/repo/src/deep")],
      ["d:/work/other", listing("d:/work/other")],
    ]);

  it("refreshes the listing that lists the changed file, and only it", () => {
    // The whole point of the change: a write to one file must not disturb the
    // rest of the district, or the ground blinks whenever an agent is working.
    expect(staleListings(cached(), "d:/work/repo", ["d:/work/repo/src/a.ts"])).toEqual([
      "d:/work/repo/src",
    ]);
  });

  it("refreshes a changed directory itself as well as its parent", () => {
    expect(staleListings(cached(), "d:/work/repo", ["d:/work/repo/src"])).toEqual([
      "d:/work/repo",
      "d:/work/repo/src",
    ]);
  });

  it("names nothing for a change beneath a folder that was never listed", () => {
    expect(staleListings(cached(), "d:/work/repo", ["d:/work/repo/docs/guide/a.md"])).toEqual(
      [],
    );
  });

  it("refreshes every cached listing under the root when the event names no path", () => {
    expect(staleListings(cached(), "d:/work/repo", [])).toEqual([
      "d:/work/repo",
      "d:/work/repo/src",
      "d:/work/repo/src/deep",
    ]);
  });

  it("ignores a sibling root whose name shares a prefix", () => {
    const listings = new Map([
      ["d:/work/repo", listing("d:/work/repo")],
      ["d:/work/repo-two", listing("d:/work/repo-two")],
    ]);
    expect(staleListings(listings, "d:/work/repo", ["d:/work/repo-two/a.ts"])).toEqual([]);
  });

  it("spends a bound budget shallowest first", () => {
    const result = staleListings(
      cached(),
      "d:/work/repo",
      ["d:/work/repo/src/deep/b.ts", "d:/work/repo/src/a.ts", "d:/work/repo/a.ts"],
      { maxRequests: 2 },
    );
    expect(result).toEqual(["d:/work/repo", "d:/work/repo/src"]);
  });

  it("leaves the input untouched", () => {
    const listings = cached();
    staleListings(listings, "d:/work/repo", []);
    expect(listings.size).toBe(4);
  });
});
