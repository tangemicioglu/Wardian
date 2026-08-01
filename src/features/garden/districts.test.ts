import { describe, expect, it } from "vitest";
import type { AgentConfig } from "../../types";
import {
  COMMONS_DISTRICT_ID,
  MAX_DISTRICT_MEMBERS,
  buildDistrictAffinity,
  createDistrictLayout,
  districtCenter,
  districtId,
  parcelsFor,
  placeDistricts,
  resolveAgentDistrict,
  resolveDistrictByAffinity,
  resolveEntityDistrict,
  retireDistricts,
} from "./districts";
import {
  CENTER_SLOT,
  firstSlotOfRing,
  ringRadii,
  ringSlotOf,
  slotDistance,
  slotIndex,
  slotPoint,
  slotsInRing,
} from "./ringLattice";

function agent(overrides: Partial<AgentConfig> = {}): AgentConfig {
  return {
    session_id: "a1",
    session_name: "Alpha",
    agent_class: "Coder",
    folder: "D:\\Dev\\Ward",
    is_off: false,
    ...overrides,
  } as AgentConfig;
}

describe("resolveAgentDistrict", () => {
  it("prefers a declared team over every derived tier", () => {
    expect(
      resolveAgentDistrict(agent(), { teamIds: ["t1"], fallbackGroupId: "g1", worktreeId: "wt1" }),
    ).toEqual({ tier: "team", id: "t1" });
  });

  it("falls back through fallback group, worktree, then workspace path", () => {
    expect(resolveAgentDistrict(agent(), { fallbackGroupId: "g1", worktreeId: "wt1" })).toEqual({
      tier: "fallback",
      id: "g1",
    });
    expect(resolveAgentDistrict(agent(), { worktreeId: "wt1" })).toEqual({
      tier: "worktree",
      id: "wt1",
    });
    expect(resolveAgentDistrict(agent())).toEqual({ tier: "workspace", id: "d:/dev/ward" });
  });

  it("traces a worktree agent back to its source repo, matching the same-project lens", () => {
    expect(
      resolveAgentDistrict(
        agent({ folder: "D:\\Dev\\Ward-wt1", git_worktree_source: "D:\\Dev\\Ward" }),
      ),
    ).toEqual({ tier: "workspace", id: "d:/dev/ward" });
  });

  it("does not depend on team file ordering, so the map is machine-independent", () => {
    const forward = resolveAgentDistrict(agent(), { teamIds: ["t2", "t1"] });
    const reversed = resolveAgentDistrict(agent(), { teamIds: ["t1", "t2"] });
    expect(forward).toEqual(reversed);
  });

  it("lands in the commons when nothing is known", () => {
    expect(districtId(resolveAgentDistrict(agent({ folder: "" })))).toBe(COMMONS_DISTRICT_ID);
  });
});

describe("ring lattice", () => {
  it("puts slot 0 at the origin", () => {
    expect(slotPoint(0, 100)).toEqual({ x: 0, y: 0 });
  });

  it("round-trips every slot through its ring and position", () => {
    for (let index = 0; index < 200; index += 1) {
      expect(slotIndex(ringSlotOf(index))).toBe(index);
    }
  });

  it("numbers slots contiguously, ring by ring", () => {
    // Ring r holds 6r slots starting at 1 + 3r(r-1); an off-by-one here would
    // silently overlay two districts on one point.
    let expected = 0;
    for (let ring = 0; ring < 8; ring += 1) {
      expect(firstSlotOfRing(ring)).toBe(expected);
      expected += slotsInRing(ring);
    }
  });

  it("spaces neighbours about one pitch apart in every direction", () => {
    // The property that lets a single pitch govern the whole map: the arc
    // between neighbours in a ring is independent of the radius, and matches the
    // gap between rings. Without it, outer districts would either collide or
    // drift apart as the map grew.
    for (let ring = 1; ring < 6; ring += 1) {
      const first = firstSlotOfRing(ring);
      const along = Math.hypot(
        slotPoint(first + 1).x - slotPoint(first).x,
        slotPoint(first + 1).y - slotPoint(first).y,
      );
      expect(along).toBeGreaterThan(0.9);
      expect(along).toBeLessThan(1.2);
    }
    // Radially: ring r sits at radius r.
    for (let ring = 0; ring < 6; ring += 1) {
      const point = slotPoint(firstSlotOfRing(ring));
      expect(Math.hypot(point.x, point.y)).toBeCloseTo(ring);
    }
  });

  it("keeps every slot distinct", () => {
    const seen = new Set<string>();
    for (let index = 0; index < 200; index += 1) {
      const point = slotPoint(index, 100);
      seen.add(`${point.x.toFixed(3)},${point.y.toFixed(3)}`);
    }
    expect(seen.size).toBe(200);
  });

  it("measures distance across the map, not along the index", () => {
    // Two slots in the same ring can be adjacent or diametrically opposite, so
    // index arithmetic is not a distance. Ring 2 holds 12 slots: opposite ends
    // are four apart, and that must read as farther than one step.
    const first = firstSlotOfRing(2);
    expect(slotDistance(first, first + 6)).toBeGreaterThan(
      slotDistance(first, first + 1),
    );
  });

  it("grows a map of districts outward rather than in one direction", () => {
    // A row-major or curve arrangement fills a quadrant; rings stay balanced
    // about the centre, which is what makes the middle read as the middle.
    let sumX = 0;
    let sumY = 0;
    for (let index = 0; index < 61; index += 1) {
      const point = slotPoint(index, 100);
      sumX += point.x;
      sumY += point.y;
    }
    expect(Math.hypot(sumX, sumY)).toBeLessThan(1);
  });

  it("maps slots to evenly spaced world coordinates", () => {
    const first = districtCenter(0, { spacing: 100, origin: { x: 10, y: 20 } });
    expect(first).toEqual({ x: 10, y: 20 });
    const second = districtCenter(1, { spacing: 100, origin: { x: 10, y: 20 } });
    expect(Math.hypot(second.x - first.x, second.y - first.y)).toBeCloseTo(100);
  });
});

describe("placeDistricts", () => {
  it("never moves an already-placed district", () => {
    // The stability invariant: insertion is additive and the interior is frozen.
    let layout = createDistrictLayout();
    layout = placeDistricts(layout, ["team:a", "team:b"]).layout;
    const snapshot = { ...layout.cells };

    const result = placeDistricts(layout, ["team:a", "team:b", "team:c", "team:d"]);
    expect(result.stable).toBe(true);
    for (const [id, cell] of Object.entries(snapshot)) {
      expect(result.layout.cells[id]).toBe(cell);
    }
    expect(result.placed).toEqual(["team:c", "team:d"]);
  });

  it("places a new district beside the district it most resembles", () => {
    let layout = createDistrictLayout();
    // Seed two far-apart districts by hand so the objective has a clear answer.
    layout = { ...layout, cells: { "team:near": 0, "team:far": 200 } };
    const similarity = (a: string, b: string) => {
      if (a === "team:new" && b === "team:near") return 1;
      if (a === "team:new" && b === "team:far") return 0.01;
      return 0;
    };
    const result = placeDistricts(layout, ["team:near", "team:far", "team:new"], similarity);
    const cell = result.layout.cells["team:new"];
    expect(slotDistance(cell, 0)).toBeLessThan(slotDistance(cell, 200));
  });

  it("is deterministic regardless of enumeration order", () => {
    const base = createDistrictLayout();
    const forward = placeDistricts(base, ["team:a", "team:b", "team:c"]).layout;
    const reversed = placeDistricts(base, ["team:c", "team:b", "team:a"]).layout;
    expect(forward.cells).toEqual(reversed.cells);
  });

  it("assigns distinct cells", () => {
    const result = placeDistricts(
      createDistrictLayout(),
      Array.from({ length: 12 }, (_, i) => `team:${i}`),
    );
    const cells = Object.values(result.layout.cells);
    expect(new Set(cells).size).toBe(cells.length);
  });

  it("seats the commons at the centre", () => {
    // The commons is what the map is arranged around; if arrival order could
    // win the middle, the arrangement would assert something false about it.
    const result = placeDistricts(createDistrictLayout(), [
      "team:a",
      COMMONS_DISTRICT_ID,
      "workspace:d:/dev",
    ]);
    expect(result.layout.cells[COMMONS_DISTRICT_ID]).toBe(CENTER_SLOT);
    expect(slotPoint(CENTER_SLOT, 720)).toEqual({ x: 0, y: 0 });
  });

  it("keeps the centre for the commons even when it arrives later", () => {
    let layout = placeDistricts(createDistrictLayout(), ["team:a", "team:b"]).layout;
    expect(Object.values(layout.cells)).not.toContain(CENTER_SLOT);
    layout = placeDistricts(layout, ["team:a", "team:b", COMMONS_DISTRICT_ID]).layout;
    expect(layout.cells[COMMONS_DISTRICT_ID]).toBe(CENTER_SLOT);
  });
});

describe("retireDistricts", () => {
  const now = 1_700_000_000_000;

  it("reserves an emptied district's cell so a returning district does not move", () => {
    // Removing the last agent from a team and re-adding it must not relocate the
    // district — that is motion with no perceivable cause.
    let layout = placeDistricts(createDistrictLayout(), ["team:a", "team:b"]).layout;
    const original = layout.cells["team:b"];

    layout = retireDistricts(layout, ["team:a"], now);
    expect(layout.cells["team:b"]).toBe(original);
    expect(layout.tombstones["team:b"]).toBeGreaterThan(now);

    const revived = placeDistricts(layout, ["team:a", "team:b"]);
    expect(revived.layout.cells["team:b"]).toBe(original);
    expect(revived.layout.tombstones["team:b"]).toBeUndefined();
    expect(revived.placed).toEqual([]);
  });

  it("reclaims the cell once the TTL expires", () => {
    let layout = placeDistricts(createDistrictLayout(), ["team:a", "team:b"]).layout;
    layout = retireDistricts(layout, ["team:a"], now, 1000);
    layout = retireDistricts(layout, ["team:a"], now + 2000, 1000);
    expect(layout.cells["team:b"]).toBeUndefined();
    expect(layout.tombstones["team:b"]).toBeUndefined();
  });

  it("clears a tombstone as soon as the district is active again", () => {
    let layout = placeDistricts(createDistrictLayout(), ["team:a"]).layout;
    layout = retireDistricts(layout, [], now);
    expect(layout.tombstones["team:a"]).toBeDefined();
    layout = retireDistricts(layout, ["team:a"], now + 10);
    expect(layout.tombstones["team:a"]).toBeUndefined();
  });
});

describe("resolveEntityDistrict", () => {
  const districtByAgent = new Map([
    ["a1", "team:hardware"],
    ["a2", "team:web"],
  ]);

  it("places a deployed skill in its target agent's district", () => {
    expect(resolveEntityDistrict(["deployed:agent:a1", "section:skills"], districtByAgent)).toBe(
      "team:hardware",
    );
  });

  it("treats a copied deployment as the same link", () => {
    // Weaker evidence of relatedness, but it still locates the skill.
    expect(resolveEntityDistrict(["deployed:agent:a1~copy"], districtByAgent)).toBe(
      "team:hardware",
    );
  });

  it("places an artifact where it was produced", () => {
    expect(resolveEntityDistrict(["origin:agent:a2"], districtByAgent)).toBe("team:web");
  });

  it("picks the most-referenced district, breaking ties deterministically", () => {
    expect(
      resolveEntityDistrict(
        ["deployed:agent:a1", "deployed:agent:a2", "origin:agent:a2"],
        districtByAgent,
      ),
    ).toBe("team:web");
  });

  it("lands in the commons rather than guessing when there is no canonical link", () => {
    expect(resolveEntityDistrict(["section:prompts", "tag:review"], districtByAgent)).toBe(
      COMMONS_DISTRICT_ID,
    );
    expect(resolveEntityDistrict(["deployed:agent:unknown"], districtByAgent)).toBe(
      COMMONS_DISTRICT_ID,
    );
  });
});

describe("parcelsFor", () => {
  it("keeps a small district whole", () => {
    const parcels = parcelsFor("team:a", ["k1", "k2"]);
    expect([...parcels.keys()]).toEqual(["team:a"]);
  });

  it("splits an oversized district so the layout's superlinear stages stay bounded", () => {
    const members = Array.from({ length: MAX_DISTRICT_MEMBERS * 2 + 5 }, (_, i) => `k${i}`);
    const parcels = parcelsFor("team:a", members);
    expect(parcels.size).toBe(3);
    for (const parcelMembers of parcels.values()) {
      expect(parcelMembers.length).toBeLessThanOrEqual(MAX_DISTRICT_MEMBERS);
    }
    expect([...parcels.values()].flat().sort()).toEqual([...members].sort());
  });

  it("puts the same member in the same parcel across sessions", () => {
    const members = Array.from({ length: 130 }, (_, i) => `k${i}`);
    const first = parcelsFor("team:a", members);
    const second = parcelsFor("team:a", [...members].reverse());
    expect([...first.entries()]).toEqual([...second.entries()]);
  });
});

describe("district affinity", () => {
  // 12 agents: 2 in the Trident workspace, 10 in Wardian. Every agent shares
  // the drive root, which is exactly the token that must not decide anything.
  const agents = [
    ...Array.from({ length: 2 }, () => ({
      tokens: ["path:d:/", "path:d:/trading", "path:d:/trading/trident"],
      districtId: "workspace:d:/trading/trident",
    })),
    ...Array.from({ length: 10 }, () => ({
      tokens: ["path:d:/", "path:d:/development", "path:d:/development/wardian"],
      districtId: "workspace:d:/development/wardian",
    })),
  ];
  const affinity = buildDistrictAffinity(agents);

  it("places on a rare shared facet", () => {
    expect(
      resolveDistrictByAffinity(["section:workflows", "path:d:/trading/trident"], affinity),
    ).toBe("workspace:d:/trading/trident");
  });

  it("refuses to place on a universal facet", () => {
    // df == N makes the IDF exactly 0, so a drive root is free and decides
    // nothing. No rule about drive roots is needed anywhere.
    expect(resolveDistrictByAffinity(["path:d:/"], affinity)).toBeNull();
  });

  it("refuses to place on a facet no agent carries", () => {
    expect(resolveDistrictByAffinity(["path:e:/elsewhere"], affinity)).toBeNull();
  });

  it("prefers the district a facet is concentrated in, not the largest one", () => {
    // The Wardian district has five times the agents; the deep Trident path is
    // still the more informative signal.
    expect(
      resolveDistrictByAffinity(
        ["path:d:/", "path:d:/trading", "path:d:/trading/trident"],
        affinity,
      ),
    ).toBe("workspace:d:/trading/trident");
  });

  it("discounts a facet split across districts", () => {
    // A token spread evenly over many districts is weak evidence for any one of
    // them, and the share factor is what makes that fall out.
    const split = buildDistrictAffinity([
      { tokens: ["tag:shared"], districtId: "a" },
      { tokens: ["tag:shared"], districtId: "b" },
      { tokens: ["tag:shared"], districtId: "c" },
      { tokens: ["tag:shared"], districtId: "d" },
    ]);
    expect(resolveDistrictByAffinity(["tag:shared"], split)).toBeNull();
  });

  it("is deterministic when two districts score identically", () => {
    const tied = buildDistrictAffinity([
      { tokens: ["path:x"], districtId: "zeta" },
      { tokens: ["path:x"], districtId: "alpha" },
      ...Array.from({ length: 20 }, () => ({ tokens: ["path:other"], districtId: "big" })),
    ]);
    const first = resolveDistrictByAffinity(["path:x"], tied);
    for (let i = 0; i < 5; i += 1) {
      expect(resolveDistrictByAffinity(["path:x"], tied)).toBe(first);
    }
  });

  it("returns null on an empty corpus rather than inventing a home", () => {
    expect(resolveDistrictByAffinity(["path:x"], buildDistrictAffinity([]))).toBeNull();
  });

  it("lets an explicit link win over affinity", () => {
    // A deployment or binding is a canonical record; affinity is inference from
    // shared facets, so it only speaks when nothing canonical does.
    const districtByAgent = new Map([["a1", "team:hw"]]);
    expect(
      resolveEntityDistrict(
        ["deployed:agent:a1", "path:d:/trading/trident"],
        districtByAgent,
        affinity,
      ),
    ).toBe("team:hw");
    expect(resolveEntityDistrict(["path:d:/trading/trident"], districtByAgent, affinity)).toBe(
      "workspace:d:/trading/trident",
    );
  });
});

describe("ringRadii", () => {
  /** Slot indices for `count` districts, filled from the centre outward. */
  const slots = (count: number) => Array.from({ length: count }, (_, i) => i);

  it("sizes a ring to what it holds, not to the widest district on the map", () => {
    // The sparsity report: one busy district set a single pitch for the whole
    // map, so thirty one-agent districts each sat alone in a cell scaled for the
    // commons. Rings of small districts should stay tight regardless of how big
    // the centre is.
    const extents = new Map(slots(19).map((slot) => [slot, slot === 0 ? 1000 : 60]));
    const radii = ringRadii(extents, 96);
    // Ring 1 has to clear the big centre, but ring 2 only has to clear ring 1.
    expect(radii[2] - radii[1]).toBeLessThan(radii[1] / 3);
  });

  it("keeps neighbours in a ring from overlapping", () => {
    // Six slots in ring 1: at too small a radius they would collide even though
    // the radial constraint alone is satisfied.
    const extents = new Map(slots(7).map((slot) => [slot, slot === 0 ? 0 : 200]));
    const radii = ringRadii(extents, 0);
    const first = firstSlotOfRing(1);
    const a = slotPoint(first, 1, radii);
    const b = slotPoint(first + 1, 1, radii);
    expect(Math.hypot(a.x - b.x, a.y - b.y)).toBeGreaterThanOrEqual(400 - 1e-6);
  });

  it("clears the ring inside it", () => {
    const extents = new Map(slots(19).map((slot) => [slot, 150]));
    const radii = ringRadii(extents, 40);
    for (let ring = 1; ring < radii.length; ring += 1) {
      // Each ring is at least its own half-width plus the inner one's away.
      expect(radii[ring] - radii[ring - 1]).toBeGreaterThanOrEqual(300);
    }
  });

  it("does not move a ring for a change too small to see", () => {
    // Radii are a continuous function of extents, so without quantization one
    // unit settling a pixel further out would slide every ring outside it. The
    // stability contract forbids exactly that.
    const base = new Map(slots(13).map((slot) => [slot, 100]));
    const nudged = new Map(base);
    nudged.set(5, 101);
    expect(ringRadii(nudged, 96)).toEqual(ringRadii(base, 96));
  });

  it("does move once the change is a real one", () => {
    const base = new Map(slots(13).map((slot) => [slot, 100]));
    const grown = new Map(base);
    grown.set(5, 400);
    expect(ringRadii(grown, 96)[2]).toBeGreaterThan(ringRadii(base, 96)[2]);
  });

  it("puts the centre at the origin whatever it holds", () => {
    const radii = ringRadii(new Map([[0, 5000]]), 96);
    expect(radii[0]).toBe(0);
    expect(slotPoint(0, 1, radii)).toEqual({ x: 0, y: 0 });
  });

  it("falls back to a uniform pitch when given no radii", () => {
    const point = slotPoint(firstSlotOfRing(2), 300);
    expect(Math.hypot(point.x, point.y)).toBeCloseTo(600);
  });
});
