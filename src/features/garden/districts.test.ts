import { describe, expect, it } from "vitest";
import type { AgentConfig } from "../../types";
import {
  COMMONS_DISTRICT_ID,
  DEFAULT_HILBERT_ORDER,
  MAX_DISTRICT_MEMBERS,
  cellDistance,
  cellToHilbert,
  createDistrictLayout,
  districtCenter,
  districtId,
  hilbertCellCount,
  hilbertToCell,
  parcelsFor,
  placeDistricts,
  resolveAgentDistrict,
  resolveEntityDistrict,
  retireDistricts,
} from "./districts";

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

describe("Hilbert curve", () => {
  it("round-trips every cell of the grid", () => {
    const order = 3;
    for (let index = 0; index < hilbertCellCount(order); index += 1) {
      expect(cellToHilbert(hilbertToCell(index, order), order)).toBe(index);
    }
  });

  it("keeps consecutive indices grid-adjacent", () => {
    // The locality property the whole placement scheme rests on.
    const order = 4;
    for (let index = 1; index < hilbertCellCount(order); index += 1) {
      const previous = hilbertToCell(index - 1, order);
      const current = hilbertToCell(index, order);
      const manhattan =
        Math.abs(previous.x - current.x) + Math.abs(previous.y - current.y);
      expect(manhattan).toBe(1);
    }
  });

  it("preserves locality better than row-major order", () => {
    // Compare mean grid distance between index-adjacent cells. Row-major wraps
    // at the end of each row and jumps the full width; Hilbert never does.
    const order = 4;
    const side = 1 << order;
    let hilbertTotal = 0;
    let rowMajorTotal = 0;
    for (let index = 1; index < hilbertCellCount(order); index += 1) {
      hilbertTotal += cellDistance(index - 1, index, order);
      const previous = { x: (index - 1) % side, y: Math.floor((index - 1) / side) };
      const current = { x: index % side, y: Math.floor(index / side) };
      rowMajorTotal += Math.max(
        Math.abs(previous.x - current.x),
        Math.abs(previous.y - current.y),
      );
    }
    expect(hilbertTotal).toBeLessThan(rowMajorTotal);
  });

  it("maps cells to evenly spaced world coordinates", () => {
    const first = districtCenter(0, { order: 3, spacing: 100, origin: { x: 10, y: 20 } });
    expect(first).toEqual({ x: 10, y: 20 });
    const second = districtCenter(1, { order: 3, spacing: 100, origin: { x: 10, y: 20 } });
    expect(Math.max(Math.abs(second.x - first.x), Math.abs(second.y - first.y))).toBe(100);
  });
});

describe("placeDistricts", () => {
  it("never moves an already-placed district", () => {
    // The stability invariant: insertion is additive and the interior is frozen.
    let layout = createDistrictLayout(4);
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
    let layout = createDistrictLayout(4);
    // Seed two far-apart districts by hand so the objective has a clear answer.
    layout = { ...layout, cells: { "team:near": 0, "team:far": 200 } };
    const similarity = (a: string, b: string) => {
      if (a === "team:new" && b === "team:near") return 1;
      if (a === "team:new" && b === "team:far") return 0.01;
      return 0;
    };
    const result = placeDistricts(layout, ["team:near", "team:far", "team:new"], similarity);
    const cell = result.layout.cells["team:new"];
    expect(cellDistance(cell, 0, 4)).toBeLessThan(cellDistance(cell, 200, 4));
  });

  it("is deterministic regardless of enumeration order", () => {
    const base = createDistrictLayout(4);
    const forward = placeDistricts(base, ["team:a", "team:b", "team:c"]).layout;
    const reversed = placeDistricts(base, ["team:c", "team:b", "team:a"]).layout;
    expect(forward.cells).toEqual(reversed.cells);
  });

  it("assigns distinct cells", () => {
    const result = placeDistricts(
      createDistrictLayout(4),
      Array.from({ length: 12 }, (_, i) => `team:${i}`),
    );
    const cells = Object.values(result.layout.cells);
    expect(new Set(cells).size).toBe(cells.length);
  });

  it("uses the default grid order when none is given", () => {
    expect(createDistrictLayout().order).toBe(DEFAULT_HILBERT_ORDER);
  });
});

describe("retireDistricts", () => {
  const now = 1_700_000_000_000;

  it("reserves an emptied district's cell so a returning district does not move", () => {
    // Removing the last agent from a team and re-adding it must not relocate the
    // district — that is motion with no perceivable cause.
    let layout = placeDistricts(createDistrictLayout(4), ["team:a", "team:b"]).layout;
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
    let layout = placeDistricts(createDistrictLayout(4), ["team:a", "team:b"]).layout;
    layout = retireDistricts(layout, ["team:a"], now, 1000);
    layout = retireDistricts(layout, ["team:a"], now + 2000, 1000);
    expect(layout.cells["team:b"]).toBeUndefined();
    expect(layout.tombstones["team:b"]).toBeUndefined();
  });

  it("clears a tombstone as soon as the district is active again", () => {
    let layout = placeDistricts(createDistrictLayout(4), ["team:a"]).layout;
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
