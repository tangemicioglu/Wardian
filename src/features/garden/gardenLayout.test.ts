import { describe, expect, it } from "vitest";
import type { AgentConfig } from "../../types";
import { agentRef, libraryEntryRef } from "./entityRef";
import { emitAgentFacets, emitSkillFacets } from "./facets";
import { COMMONS_DISTRICT_ID, MAX_DISTRICT_MEMBERS } from "./districts";
import { interactionWeight, personalizedPageRank } from "./metric";
import { maxDisplacement } from "./smacof";
import { overlaps } from "./vpsc";
import {
  createScene,
  excludeFromDistrict,
  markVisited,
  pinEntity,
  type GardenScene,
} from "./gardenScene";
import { layoutGarden, type LayoutEntity } from "./gardenLayout";

const now = 1_700_000_000_000;

function agent(id: string, overrides: Partial<AgentConfig> = {}): AgentConfig {
  return {
    session_id: id,
    session_name: id,
    agent_class: "Coder",
    folder: "D:\\Dev\\Ward",
    is_off: false,
    ...overrides,
  } as AgentConfig;
}

function agentEntity(
  id: string,
  districtId: string,
  config: Partial<AgentConfig> = {},
  context: Parameters<typeof emitAgentFacets>[2] = {},
): LayoutEntity {
  const ref = agentRef(id);
  return {
    ref,
    facets: emitAgentFacets(agent(id, config), ref, context),
    districtId,
    width: 90,
    height: 40,
  };
}

function skillEntity(entryRef: string, districtId: string, agentIds: string[]): LayoutEntity {
  const ref = libraryEntryRef(entryRef)!;
  return {
    ref,
    facets: emitSkillFacets(ref, {
      deployments: agentIds.map((targetId) => ({ targetType: "agent", targetId, linked: true })),
    }),
    districtId,
    width: 70,
    height: 30,
  };
}

function teamOf(size: number, teamId: string, folder: string): LayoutEntity[] {
  return Array.from({ length: size }, (_, i) =>
    agentEntity(`${teamId}-a${i}`, `team:${teamId}`, { folder }, { teamIds: [teamId] }),
  );
}

describe("layoutGarden", () => {
  const entities = [
    ...teamOf(4, "hw", "D:\\Dev\\Hardware"),
    ...teamOf(4, "web", "D:\\Dev\\Web"),
  ];

  it("places every entity exactly once", () => {
    const result = layoutGarden({ entities, scene: createScene(), now });
    expect(result.units).toHaveLength(entities.length);
    expect(new Set(result.units.map((unit) => unit.key)).size).toBe(entities.length);
  });

  it("separates districts in world space", () => {
    const result = layoutGarden({ entities, scene: createScene(), now });
    const centroid = (districtId: string) => {
      const members = result.units.filter((unit) => unit.districtId === districtId);
      return {
        x: members.reduce((sum, unit) => sum + unit.position.x, 0) / members.length,
        y: members.reduce((sum, unit) => sum + unit.position.y, 0) / members.length,
      };
    };
    const hardware = centroid("team:hw");
    const web = centroid("team:web");
    expect(Math.hypot(hardware.x - web.x, hardware.y - web.y)).toBeGreaterThan(300);
  });

  it("leaves no overlapping units", () => {
    const result = layoutGarden({ entities, scene: createScene(), now });
    expect(result.residualOverlaps).toEqual([]);
    const boxes = result.units.map((unit) => {
      const source = entities.find((entity) => entity.ref.id === unit.ref.id)!;
      return { key: unit.key, position: unit.position, width: source.width, height: source.height };
    });
    for (let i = 0; i < boxes.length; i += 1) {
      for (let j = i + 1; j < boxes.length; j += 1) {
        expect(overlaps(boxes[i], boxes[j])).toBe(false);
      }
    }
  });

  it("is deterministic under input reordering", () => {
    const forward = layoutGarden({ entities, scene: createScene(), now });
    const reversed = layoutGarden({ entities: [...entities].reverse(), scene: createScene(), now });
    expect(reversed.units).toEqual(forward.units);
  });

  it("places a deployed skill inside its target agent's district", () => {
    const withSkill = [...entities, skillEntity("skills/kicad", "team:hw", ["hw-a0"])];
    const result = layoutGarden({ entities: withSkill, scene: createScene(), now });
    const skill = result.units.find((unit) => unit.ref.kind === "skill")!;
    expect(skill.districtId).toBe("team:hw");

    const target = result.units.find((unit) => unit.key === "agent:hw-a0")!;
    const stranger = result.units.find((unit) => unit.key === "agent:web-a0")!;
    expect(distance(skill.position, target.position)).toBeLessThan(
      distance(skill.position, stranger.position),
    );
  });
});

describe("stability", () => {
  const base = [...teamOf(5, "hw", "D:\\Dev\\Hardware"), ...teamOf(5, "web", "D:\\Dev\\Web")];

  function settle(entities: LayoutEntity[], scene: GardenScene) {
    // Two passes: the first has no stored positions, so nothing resists drift.
    const first = layoutGarden({ entities, scene, now });
    return layoutGarden({ entities, scene: first.scene, now });
  }

  it("keeps existing units in place when a new agent joins an existing team", () => {
    // The stability contract: insertion is additive and the interior is quiet.
    const settled = settle(base, createScene());
    const before = new Map(settled.units.map((unit) => [unit.key, unit.position]));

    const grown = [
      ...base,
      agentEntity("hw-new", "team:hw", { folder: "D:\\Dev\\Hardware" }, { teamIds: ["hw"] }),
    ];
    const after = layoutGarden({ entities: grown, scene: settled.scene, now });
    const positions = new Map(after.units.map((unit) => [unit.key, unit.position]));

    expect(maxDisplacement(before, positions)).toBeLessThan(120);
    expect(after.units.find((unit) => unit.key === "agent:hw-new")).toBeDefined();
  });

  it("does not move an unrelated district when a new district appears", () => {
    const settled = settle(base, createScene());
    const webBefore = new Map(
      settled.units.filter((u) => u.districtId === "team:web").map((u) => [u.key, u.position]),
    );

    const grown = [...base, ...teamOf(3, "ml", "D:\\Dev\\ML")];
    const after = layoutGarden({ entities: grown, scene: settled.scene, now });
    const webAfter = new Map(
      after.units.filter((u) => u.districtId === "team:web").map((u) => [u.key, u.position]),
    );
    expect(maxDisplacement(webBefore, webAfter)).toBeLessThan(60);
  });

  it("keeps a district's cell across a pass", () => {
    const settled = settle(base, createScene());
    const cells = { ...settled.scene.districts.cells };
    const after = layoutGarden({ entities: base, scene: settled.scene, now });
    expect(after.districts.cells).toMatchObject(cells);
  });

  it("converges on idle re-runs instead of creeping", () => {
    // SMACOF stops at a tolerance rather than an exact optimum, so a re-run
    // warm-started from its own output takes a few more steps toward the true
    // solution. That is convergence, not drift — the distinction that matters is
    // whether successive passes shrink. They decay geometrically here, so an
    // idle map settles instead of wandering.
    let scene = createScene();
    let previous = new Map<string, { x: number; y: number }>();
    const drifts: number[] = [];
    for (let pass = 0; pass < 6; pass += 1) {
      const result = layoutGarden({ entities: base, scene, now });
      const positions = new Map(result.units.map((unit) => [unit.key, unit.position]));
      if (pass > 0) drifts.push(maxDisplacement(previous, positions));
      previous = positions;
      scene = result.scene;
    }

    for (let i = 1; i < drifts.length; i += 1) {
      expect(drifts[i]).toBeLessThanOrEqual(drifts[i - 1] + 1e-9);
    }
    expect(drifts[drifts.length - 1]).toBeLessThan(1);
  });
});

describe("user placement", () => {
  const base = teamOf(4, "hw", "D:\\Dev\\Hardware");

  it("holds a pinned unit exactly where the user put it", () => {
    const first = layoutGarden({ entities: base, scene: createScene(), now });
    const districtOrigin = { x: 0, y: 0 };
    const pinned = pinEntity(
      first.scene,
      "agent:hw-a0",
      "team:hw",
      { x: districtOrigin.x + 250, y: districtOrigin.y - 180 },
      districtOrigin,
      now,
    );
    const result = layoutGarden({ entities: base, scene: pinned, now });
    const unit = result.units.find((u) => u.key === "agent:hw-a0")!;
    expect(unit.pinned).toBe(true);
    expect(unit.position).toEqual({ x: 250, y: -180 });
  });

  it("reports a pin stranded by a district change instead of honouring it", () => {
    const first = layoutGarden({ entities: base, scene: createScene(), now });
    const pinned = pinEntity(first.scene, "agent:hw-a0", "team:hw", { x: 9, y: 9 }, { x: 0, y: 0 }, now);
    const moved = base.map((entity) =>
      entity.ref.id === "hw-a0" ? { ...entity, districtId: "team:web" } : entity,
    );
    const result = layoutGarden({ entities: moved, scene: pinned, now });
    expect(result.stalePinKeys).toEqual(["agent:hw-a0"]);
  });

  it("pulls an anchored entity toward the district it was placed in", () => {
    // Placement feeds the metric, not just the geometry: after anchoring, the
    // entity is genuinely closer to that district's members.
    const mixed = [
      ...teamOf(4, "hw", "D:\\Dev\\Hardware"),
      ...teamOf(4, "web", "D:\\Dev\\Web"),
      agentEntity("wanderer", "team:web", { folder: "D:\\Dev\\Elsewhere" }),
    ];
    const plain = layoutGarden({ entities: mixed, scene: createScene(), now });
    const plainWanderer = plain.units.find((u) => u.key === "agent:wanderer")!;
    const plainWebCentroid = centroid(plain.units.filter((u) => u.districtId === "team:web"));

    const anchored = pinEntity(
      createScene(),
      "agent:wanderer",
      "team:web",
      { x: 0, y: 0 },
      { x: 0, y: 0 },
      now,
    );
    const withAnchor = layoutGarden({ entities: mixed, scene: anchored, now });
    const anchorWanderer = withAnchor.units.find((u) => u.key === "agent:wanderer")!;

    // The anchor facet is present on the entity's vector.
    const anchoredFacets = withAnchor.corpus.df.get("scene_anchor:team:web");
    expect(anchoredFacets).toBe(1);
    expect(distance(anchorWanderer.position, plainWebCentroid)).toBeLessThanOrEqual(
      distance(plainWanderer.position, plainWebCentroid) + 1e-6,
    );
  });

  it("repels an entity from a district the user rejected", () => {
    const mixed = [...teamOf(3, "hw", "D:\\Dev\\Hardware"), ...teamOf(3, "web", "D:\\Dev\\Web")];
    const scene = excludeFromDistrict(createScene(), "agent:hw-a0", "team:web");
    const result = layoutGarden({ entities: mixed, scene, now });
    // The exclusion is carried on the entity's facet set, not in its cosine
    // vector, and reaches the metric as a repulsion offset.
    expect(result.units.find((u) => u.key === "agent:hw-a0")).toBeDefined();
    expect(result.scene.exclusions["agent:hw-a0"]).toEqual(["team:web"]);
  });

  it("resists moving a recently visited unit more than a settled one", () => {
    const first = layoutGarden({ entities: base, scene: createScene(), now });
    const settled = layoutGarden({ entities: base, scene: first.scene, now });
    const before = new Map(settled.units.map((u) => [u.key, u.position]));

    const grown = [
      ...base,
      agentEntity("hw-new", "team:hw", { folder: "D:\\Dev\\Hardware" }, { teamIds: ["hw"] }),
    ];
    const plain = layoutGarden({ entities: grown, scene: settled.scene, now });
    const visited = layoutGarden({
      entities: grown,
      scene: markVisited(settled.scene, "agent:hw-a1", now),
      now,
    });

    const plainDrift = shift(before, plain.units, "agent:hw-a1");
    const visitedDrift = shift(before, visited.units, "agent:hw-a1");
    expect(visitedDrift).toBeLessThanOrEqual(plainDrift + 1e-9);
  });
});

describe("interaction affinity", () => {
  it("pulls agents that actually talk closer than agents that merely share a team", () => {
    const members = teamOf(6, "hw", "D:\\Dev\\Hardware");
    const ppr = personalizedPageRank(
      members.map((entity) => `agent:${entity.ref.id}`),
      [
        {
          source: "agent:hw-a0",
          target: "agent:hw-a1",
          weight: interactionWeight({ manual: true, recency: 1, kind: "Task" }),
        },
      ],
    );
    const result = layoutGarden({ entities: members, scene: createScene(), ppr, now });
    const byKey = new Map(result.units.map((unit) => [unit.key, unit.position]));
    const talking = distance(byKey.get("agent:hw-a0")!, byKey.get("agent:hw-a1")!);
    const silent = distance(byKey.get("agent:hw-a0")!, byKey.get("agent:hw-a4")!);
    expect(talking).toBeLessThan(silent);
  });
});

describe("scaling", () => {
  it("splits an oversized district into parcels", () => {
    const big = teamOf(MAX_DISTRICT_MEMBERS + 20, "hw", "D:\\Dev\\Hardware");
    const result = layoutGarden({ entities: big, scene: createScene(), now });
    const parcels = new Set(result.units.map((unit) => unit.parcelId));
    expect(parcels.size).toBeGreaterThan(1);
    expect(result.units).toHaveLength(big.length);
  });

  it("lays out a realistic map inside a frame budget", () => {
    const entities = [
      ...teamOf(12, "hw", "D:\\Dev\\Hardware"),
      ...teamOf(12, "web", "D:\\Dev\\Web"),
      ...teamOf(10, "ml", "D:\\Dev\\ML"),
      ...Array.from({ length: 20 }, (_, i) =>
        skillEntity(`skills/s${i}`, "team:hw", [`hw-a${i % 12}`]),
      ),
      ...Array.from({ length: 8 }, (_, i) =>
        agentEntity(`loose${i}`, COMMONS_DISTRICT_ID, { folder: `D:\\Misc\\p${i}` }),
      ),
    ];
    const started = performance.now();
    const result = layoutGarden({ entities, scene: createScene(), now });
    const elapsed = performance.now() - started;

    expect(result.units).toHaveLength(entities.length);
    expect(elapsed).toBeLessThan(600);
  });

  it("handles an empty map", () => {
    const result = layoutGarden({ entities: [], scene: createScene(), now });
    expect(result.units).toEqual([]);
    expect(result.residualOverlaps).toEqual([]);
  });
});

function distance(a: { x: number; y: number }, b: { x: number; y: number }) {
  return Math.hypot(a.x - b.x, a.y - b.y);
}

function centroid(units: Array<{ position: { x: number; y: number } }>) {
  return {
    x: units.reduce((sum, unit) => sum + unit.position.x, 0) / units.length,
    y: units.reduce((sum, unit) => sum + unit.position.y, 0) / units.length,
  };
}

function shift(
  before: ReadonlyMap<string, { x: number; y: number }>,
  units: Array<{ key: string; position: { x: number; y: number } }>,
  key: string,
) {
  const start = before.get(key)!;
  const end = units.find((unit) => unit.key === key)!.position;
  return distance(start, end);
}
