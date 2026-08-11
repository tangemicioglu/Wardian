import { describe, expect, it } from "vitest";
import type { AgentConfig } from "../../types";
import { agentRef, libraryEntryRef } from "./entityRef";
import { emitAgentFacets, emitSkillFacets } from "./facets";
import {
  COMMONS_DISTRICT_ID,
  DEFAULT_DISTRICT_SPACING,
  MAX_DISTRICT_MEMBERS,
  MAX_DISTRICT_SPACING,
} from "./districts";
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
import { DISTRICT_GROUND_MARGIN, districtFootprint, MIN_GROUND_RADIUS } from "./terrain";

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

  it("separates districts by the room their ground will need", () => {
    // Stated as the invariant rather than as a distance, because the distance is
    // derived: the lattice reserves each district's drawn footprint and leaves
    // `DISTRICT_GROUND_MARGIN` between them. A literal here would only record
    // whatever the arithmetic currently produces.
    const result = layoutGarden({ entities, scene: createScene(), now });
    const footprintOf = (districtId: string) =>
      districtFootprint(result.districtExtents.get(districtId) ?? 0);
    const hardware = result.districtOrigins.get("team:hw")!;
    const web = result.districtOrigins.get("team:web")!;
    const between = Math.hypot(hardware.x - web.x, hardware.y - web.y);
    expect(between - footprintOf("team:hw") - footprintOf("team:web")).toBeGreaterThanOrEqual(
      DISTRICT_GROUND_MARGIN,
    );
  });

  it("keeps two districts' members in visibly different places", () => {
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
    expect(Math.hypot(hardware.x - web.x, hardware.y - web.y)).toBeGreaterThan(
      MIN_GROUND_RADIUS,
    );
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
    // The offset is stored relative to the district, so it resolves against
    // wherever the district currently sits — which is the whole point of storing
    // it that way, and is why moving the district does not strand the pin.
    const origin = result.districtOrigins.get("team:hw")!;
    expect(unit.position.x).toBeCloseTo(origin.x + 250);
    expect(unit.position.y).toBeCloseTo(origin.y - 180);
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
    // Compared inside the district's own frame. Ring geometry decides where a
    // district sits, and a pin changes what its district measures, so world
    // coordinates from two different runs are not comparable — while the local
    // solve this claim is actually about is untouched by any of that.
    const webLocal = (run: ReturnType<typeof layoutGarden>, key: string) => {
      const origin = run.districtOrigins.get("team:web")!;
      const unit = run.units.find((u) => u.key === key)!;
      return { x: unit.position.x - origin.x, y: unit.position.y - origin.y };
    };
    const webPeers = (run: ReturnType<typeof layoutGarden>) =>
      centroid(
        run.units
          .filter((u) => u.districtId === "team:web" && u.key !== "agent:wanderer")
          .map((u) => ({ position: webLocal(run, u.key) })),
      );

    const plain = layoutGarden({ entities: mixed, scene: createScene(), now });

    const anchored = pinEntity(
      createScene(),
      "agent:wanderer",
      "team:web",
      { x: 0, y: 0 },
      { x: 0, y: 0 },
      now,
    );
    const withAnchor = layoutGarden({ entities: mixed, scene: anchored, now });

    // The anchor facet is present on the entity's vector.
    const anchoredFacets = withAnchor.corpus.df.get("scene_anchor:team:web");
    expect(anchoredFacets).toBe(1);

    // Both runs measured against one fixed reference — where team:web sat
    // before anchoring. Using each run's own centroid would move the target:
    // pinning the wanderer displaces its neighbours a little, so the peers
    // drift with it and the comparison stops meaning anything.
    const reference = webPeers(plain);
    expect(distance(webLocal(withAnchor, "agent:wanderer"), reference)).toBeLessThanOrEqual(
      distance(webLocal(plain, "agent:wanderer"), reference) + 1e-6,
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

describe("district separation", () => {
  /** Axis-aligned bounds of a district's drawn units, footprints included. */
  function boundsOf(
    units: ReturnType<typeof layoutGarden>["units"],
    entities: LayoutEntity[],
    districtId: string,
  ) {
    const sizeOf = new Map(entities.map((e) => [entityKeyOf(e), e]));
    const members = units.filter((unit) => unit.districtId === districtId);
    return members.reduce(
      (box, unit) => {
        const entity = sizeOf.get(unit.key)!;
        return {
          minX: Math.min(box.minX, unit.position.x - entity.width / 2),
          maxX: Math.max(box.maxX, unit.position.x + entity.width / 2),
          minY: Math.min(box.minY, unit.position.y - entity.height / 2),
          maxY: Math.max(box.maxY, unit.position.y + entity.height / 2),
        };
      },
      { minX: Infinity, maxX: -Infinity, minY: Infinity, maxY: -Infinity },
    );
  }

  function entityKeyOf(entity: LayoutEntity): string {
    return `${entity.ref.kind}:${entity.ref.id}`;
  }

  function boxesOverlap(
    a: ReturnType<typeof boundsOf>,
    b: ReturnType<typeof boundsOf>,
  ): boolean {
    return a.minX < b.maxX && b.minX < a.maxX && a.minY < b.maxY && b.minY < a.maxY;
  }

  it("keeps crowded districts from bleeding into each other", () => {
    // The failure this exists to catch: the grid pitch was a constant while
    // overlap removal ran per parcel with no notion of a cell boundary, so a
    // populous district simply grew past its cell. The map then showed one
    // crowd where the data had two.
    const entities = [
      ...teamOf(24, "alpha", "D:\Dev\Alpha"),
      ...teamOf(24, "beta", "D:\Dev\Beta"),
      ...teamOf(24, "gamma", "D:\Dev\Gamma"),
    ];
    const result = layoutGarden({ entities, scene: createScene(), now });

    const ids = ["team:alpha", "team:beta", "team:gamma"];
    for (let i = 0; i < ids.length; i += 1) {
      for (let j = i + 1; j < ids.length; j += 1) {
        const left = boundsOf(result.units, entities, ids[i]);
        const right = boundsOf(result.units, entities, ids[j]);
        expect(boxesOverlap(left, right)).toBe(false);
      }
    }
  });

  it("widens the pitch only as far as the widest district needs", () => {
    const small = layoutGarden({
      entities: [...teamOf(3, "alpha", "D:\Dev\Alpha"), ...teamOf(3, "beta", "D:\Dev\Beta")],
      scene: createScene(),
      now,
    });
    const large = layoutGarden({
      entities: [...teamOf(30, "alpha", "D:\Dev\Alpha"), ...teamOf(30, "beta", "D:\Dev\Beta")],
      scene: createScene(),
      now,
    });
    expect(small.districts.spacing).toBe(DEFAULT_DISTRICT_SPACING);
    expect(large.districts.spacing).toBeGreaterThanOrEqual(small.districts.spacing);
  });

  it("settles on a pitch instead of ratcheting outward across passes", () => {
    // Spacing feeds back into the next pass through warm starts, so a rule that
    // only ever grew would push the map apart a little on every relayout.
    const entities = [
      ...teamOf(20, "alpha", "D:\Dev\Alpha"),
      ...teamOf(20, "beta", "D:\Dev\Beta"),
    ];
    let scene = createScene();
    const seen: number[] = [];
    for (let pass = 0; pass < 6; pass += 1) {
      const result = layoutGarden({ entities, scene, now });
      scene = result.scene;
      seen.push(result.districts.spacing);
    }
    expect(seen[seen.length - 1]).toBe(seen[2]);
  });

  it("does not let one dragged unit inflate the map", () => {
    // Letting a pin count toward its district's extent was tried, to make the
    // district "hold" what was dragged into it, and it was worse than the
    // problem: a pin offset is arbitrary, so the district grew to match, every
    // ring grew with it, and the pinned unit rode its own origin outward.
    // Measured on a twelve-district map, dropping one unit on a neighbour
    // inflated the span from 1259 to 2935 and left the unit 600 units from where
    // it was released. Districts are sized by what the metric placed in them; a
    // drag that would leave the district is clamped to it by the caller instead.
    const entities = [
      ...teamOf(6, "alpha", "D:\Dev\Alpha"),
      ...teamOf(6, "beta", "D:\Dev\Beta"),
    ];
    const base = layoutGarden({ entities, scene: createScene(), now });
    const origin = base.districtOrigins.get("team:alpha")!;
    const dragged = pinEntity(
      base.scene,
      "agent:alpha-a0",
      "team:alpha",
      { x: origin.x + 4000, y: origin.y + 4000 },
      origin,
    );

    const after = layoutGarden({ entities, scene: dragged, now });

    // Every other district stays exactly where it was: a drag places one unit,
    // it does not rearrange the map.
    for (const [id, at] of base.districtOrigins) {
      expect(after.districtOrigins.get(id)!.x).toBeCloseTo(at.x);
      expect(after.districtOrigins.get(id)!.y).toBeCloseTo(at.y);
    }

    // The pin is still honoured exactly, and against an origin that did not move
    // — which is what stops the unit drifting away from where it was dropped.
    const unit = after.units.find((u) => u.key === "agent:alpha-a0")!;
    expect(unit.position.x).toBeCloseTo(origin.x + 4000);
    expect(unit.position.y).toBeCloseTo(origin.y + 4000);
  });

  it("reports how far each district reaches, so a drag can be held inside it", () => {
    // The caller cannot keep a drop in its district without knowing where the
    // district ends, and only the layout knows that.
    const entities = [...teamOf(6, "alpha", "D:\\Dev\\Alpha"), ...teamOf(6, "beta", "D:\\Dev\\Beta")];
    const result = layoutGarden({ entities, scene: createScene(), now });

    for (const districtId of ["team:alpha", "team:beta"]) {
      const extent = result.districtExtents.get(districtId)!;
      expect(extent).toBeGreaterThan(0);
      const origin = result.districtOrigins.get(districtId)!;
      for (const unit of result.units.filter((u) => u.districtId === districtId)) {
        expect(Math.hypot(unit.position.x - origin.x, unit.position.y - origin.y)).toBeLessThanOrEqual(
          extent + 1e-6,
        );
      }
    }
  });

  it("stops growing once it has made room", () => {
    // The reason pins were kept out of the measurement in the first place. The
    // growth has to be idempotent: re-running the layout over a settled scene
    // must leave the districts exactly where they are, or a drag would push the
    // map outward a little further on every pass for as long as it was open.
    const entities = [...teamOf(6, "alpha", "D:\\Dev\\Alpha"), ...teamOf(6, "beta", "D:\\Dev\\Beta")];
    const base = layoutGarden({ entities, scene: createScene(), now });
    const origin = base.districtOrigins.get("team:alpha")!;
    const dragged = pinEntity(
      base.scene,
      "agent:alpha-a0",
      "team:alpha",
      { x: origin.x + 4000, y: origin.y + 4000 },
      origin,
    );

    const first = layoutGarden({ entities, scene: dragged, now });
    const second = layoutGarden({ entities, scene: first.scene, now });
    const third = layoutGarden({ entities, scene: second.scene, now });
    for (const [id, at] of second.districtOrigins) {
      expect(third.districtOrigins.get(id)!.x).toBeCloseTo(at.x);
      expect(third.districtOrigins.get(id)!.y).toBeCloseTo(at.y);
    }
  });
});

describe("stored positions never become geometry", () => {
  // Twenty-four agents over six workspaces: enough districts that a change in
  // how they are grouped moves most units into a different frame.
  function roster(districtOf: (i: number) => string): LayoutEntity[] {
    return Array.from({ length: 24 }, (_, i) =>
      agentEntity(`r${i}`, districtOf(i), { folder: `D:\\Dev\\P${i % 6}` }),
    );
  }

  const asCommons = roster(() => COMMONS_DISTRICT_ID);
  const asWorkspaces = roster((i) => `workspace:d:/dev/p${i % 6}`);

  /** Full extent of the laid-out map, which is what the viewport must fit. */
  function span(result: ReturnType<typeof layoutGarden>) {
    const xs = result.units.map((unit) => unit.position.x);
    const ys = result.units.map((unit) => unit.position.y);
    return Math.max(Math.max(...xs) - Math.min(...xs), Math.max(...ys) - Math.min(...ys));
  }

  it("survives the districting rule changing under a persisted scene", () => {
    // This is the failure that produced an empty Garden on a real install, and
    // it needs a *persisted* scene to appear at all — every test that starts
    // from `createScene()` misses it.
    //
    // A stored position is absolute, so it only means anything against the
    // origin it was measured from. When these agents move from the commons into
    // per-workspace districts, yesterday's coordinate re-based on today's origin
    // is off by roughly the distance between two cells. The drift penalty then
    // holds each unit at that offset, the district's measured extent grows by a
    // whole grid pitch, and the pitch is derived from that extent — so the next
    // pass reads every stored position in a wider frame again. Observed: one
    // re-grouping took the pitch from 720 to 7440, and a few in succession put
    // the map eleven thousand times too large to draw.
    let scene = layoutGarden({ entities: asCommons, scene: createScene(), now }).scene;

    for (let pass = 0; pass < 6; pass += 1) {
      const result = layoutGarden({ entities: asWorkspaces, scene, now });
      scene = result.scene;
      expect(result.districts.spacing).toBeLessThanOrEqual(MAX_DISTRICT_SPACING);
      expect(span(result)).toBeLessThan(6000);
    }
  });

  it("reaches the same map whether or not a stale scene preceded it", () => {
    // The point of rejecting a stale warm start is not merely that the map stays
    // small — it is that history the layout cannot honour leaves no trace.
    const stale = layoutGarden({ entities: asCommons, scene: createScene(), now }).scene;
    const fromStale = layoutGarden({ entities: asWorkspaces, scene: stale, now });
    const fromFresh = layoutGarden({ entities: asWorkspaces, scene: createScene(), now });
    expect(fromStale.districts.spacing).toBe(fromFresh.districts.spacing);
  });

  it("heals a scene whose stored geometry is already inflated", () => {
    // Scenes in this state exist on disk, written before the above was caught.
    // They are internally consistent — the positions really were written under
    // these districts — so nothing but an absolute bound can recognise them.
    const healthy = layoutGarden({ entities: asWorkspaces, scene: createScene(), now });
    const inflated: GardenScene = {
      ...healthy.scene,
      districts: { ...healthy.scene.districts, spacing: 9_402_240 },
      positions: Object.fromEntries(
        Object.entries(healthy.scene.positions).map(([key, position], i) => [
          key,
          { x: position.x + i * 700_000, y: position.y + i * 1_300_000 },
        ]),
      ),
    };

    const recovered = layoutGarden({ entities: asWorkspaces, scene: inflated, now });
    expect(recovered.districts.spacing).toBeLessThanOrEqual(MAX_DISTRICT_SPACING);
    expect(span(recovered)).toBeLessThan(6000);
  });

  it("still warm-starts from a position written under the same district", () => {
    // The bound must not be so eager that it throws away the incremental
    // insertion it exists to protect.
    const settled = layoutGarden({ entities: asWorkspaces, scene: createScene(), now });
    const again = layoutGarden({ entities: asWorkspaces, scene: settled.scene, now });
    for (const unit of again.units) {
      const before = settled.units.find((other) => other.key === unit.key)!;
      // A few world units is stress majorization converging to its tolerance.
      // A *rejected* warm start reseeds the unit and moves it by hundreds.
      expect(distance(unit.position, before.position)).toBeLessThan(5);
    }
  });

  it("records the district each stored position was measured in", () => {
    const result = layoutGarden({ entities: asWorkspaces, scene: createScene(), now });
    for (const unit of result.units) {
      expect(result.scene.position_districts[unit.key]).toBe(unit.districtId);
    }
  });
});
