import { describe, expect, it } from "vitest";
import type { GardenPosition } from "./garden.types";
import type { Neighbour, NeighbourGraph } from "./metric";
import { symmetrizeGraph } from "./metric";
import {
  DEFAULT_LAYOUT_SCALE,
  DRIFT_FREE,
  DRIFT_NEW,
  DRIFT_PINNED,
  DRIFT_SETTLED,
  DRIFT_VISITED,
  distanceError,
  initSmacof,
  maxDisplacement,
  runSmacof,
  smacofStep,
  type SmacofNode,
} from "./smacof";

/** Build a symmetric neighbour graph from an edge list of semantic distances. */
function graphOf(edges: Array<[string, string, number]>): NeighbourGraph {
  const raw = new Map<string, Neighbour[]>();
  for (const [a, b, distance] of edges) {
    if (!raw.has(a)) raw.set(a, []);
    if (!raw.has(b)) raw.set(b, []);
    raw.get(a)!.push({ key: b, distance });
  }
  return symmetrizeGraph(raw);
}

function nodesOf(keys: string[], rho = DRIFT_SETTLED, anchors?: Record<string, GardenPosition>) {
  return keys.map<SmacofNode>((key) => ({ key, rho, anchor: anchors?.[key] }));
}

function rendered(positions: ReadonlyMap<string, GardenPosition>, a: string, b: string) {
  const left = positions.get(a)!;
  const right = positions.get(b)!;
  return Math.hypot(left.x - right.x, left.y - right.y);
}

describe("runSmacof", () => {
  it("makes rendered distance match semantic distance, not just topology", () => {
    // The whole point over a spring layout: the drawing acquires a scale bar.
    const graph = graphOf([
      ["a", "b", 0.2],
      ["b", "c", 0.2],
      ["a", "c", 0.4],
    ]);
    const state = runSmacof({ nodes: nodesOf(["a", "b", "c"], DRIFT_FREE), graph }, 300);

    expect(rendered(state.positions, "a", "b")).toBeCloseTo(0.2 * DEFAULT_LAYOUT_SCALE, 0);
    expect(rendered(state.positions, "b", "c")).toBeCloseTo(0.2 * DEFAULT_LAYOUT_SCALE, 0);
    expect(rendered(state.positions, "a", "c")).toBeCloseTo(0.4 * DEFAULT_LAYOUT_SCALE, 0);
    expect(distanceError(state)).toBeLessThan(2);
  });

  it("preserves relative ordering of distances", () => {
    const graph = graphOf([
      ["hub", "near", 0.1],
      ["hub", "mid", 0.4],
      ["hub", "far", 0.8],
    ]);
    const state = runSmacof({ nodes: nodesOf(["hub", "near", "mid", "far"], DRIFT_FREE), graph }, 300);
    expect(rendered(state.positions, "hub", "near")).toBeLessThan(
      rendered(state.positions, "hub", "mid"),
    );
    expect(rendered(state.positions, "hub", "mid")).toBeLessThan(
      rendered(state.positions, "hub", "far"),
    );
  });

  it("resolves a dense hub without collapsing it into a point", () => {
    // The hairball failure mode: 15 spokes on one hub must fan out, not pile up.
    const spokes = Array.from({ length: 15 }, (_, i) => `s${i}`);
    const graph = graphOf(spokes.map((spoke) => ["hub", spoke, 0.3] as [string, string, number]));
    const state = runSmacof({ nodes: nodesOf(["hub", ...spokes], DRIFT_FREE), graph }, 400);

    for (let i = 0; i < spokes.length; i += 1) {
      for (let j = i + 1; j < spokes.length; j += 1) {
        expect(rendered(state.positions, spokes[i], spokes[j])).toBeGreaterThan(1);
      }
    }
  });

  it("is deterministic across runs and input orderings", () => {
    const edges: Array<[string, string, number]> = [
      ["a", "b", 0.3],
      ["b", "c", 0.5],
      ["c", "d", 0.2],
      ["a", "d", 0.6],
    ];
    const keys = ["a", "b", "c", "d"];
    const first = runSmacof({ nodes: nodesOf(keys, DRIFT_FREE), graph: graphOf(edges) }, 200);
    const second = runSmacof(
      { nodes: nodesOf([...keys].reverse(), DRIFT_FREE), graph: graphOf([...edges].reverse()) },
      200,
    );
    for (const key of keys) {
      expect(second.positions.get(key)!.x).toBeCloseTo(first.positions.get(key)!.x, 9);
      expect(second.positions.get(key)!.y).toBeCloseTo(first.positions.get(key)!.y, 9);
    }
  });

  it("separates coincident nodes deterministically", () => {
    // Two nodes seeded at the identical anchor have no direction to separate
    // along; the tie-break must be reproducible rather than floating-point noise.
    const graph = graphOf([["a", "b", 0.4]]);
    const anchors = { a: { x: 100, y: 100 }, b: { x: 100, y: 100 } };
    const first = runSmacof({ nodes: nodesOf(["a", "b"], DRIFT_FREE, anchors), graph }, 200);
    const second = runSmacof({ nodes: nodesOf(["a", "b"], DRIFT_FREE, anchors), graph }, 200);
    expect(rendered(first.positions, "a", "b")).toBeGreaterThan(1);
    expect(first.positions.get("a")).toEqual(second.positions.get("a"));
  });

  it("handles a single node and an edgeless set without diverging", () => {
    const solo = runSmacof({ nodes: nodesOf(["only"]), graph: new Map() });
    expect(solo.positions.get("only")).toBeDefined();
    expect(Number.isFinite(solo.positions.get("only")!.x)).toBe(true);

    const edgeless = runSmacof({ nodes: nodesOf(["a", "b", "c"]), graph: new Map() });
    for (const key of ["a", "b", "c"]) {
      expect(Number.isFinite(edgeless.positions.get(key)!.x)).toBe(true);
    }
  });
});

describe("pinning", () => {
  it("holds a pinned node exactly where it was placed", () => {
    // A user's placement is a hard constraint, not a suggestion.
    const graph = graphOf([
      ["pinned", "a", 0.3],
      ["a", "b", 0.3],
    ]);
    const nodes: SmacofNode[] = [
      { key: "pinned", rho: DRIFT_PINNED, anchor: { x: 500, y: 500 } },
      { key: "a", rho: DRIFT_FREE },
      { key: "b", rho: DRIFT_FREE },
    ];
    const state = runSmacof({ nodes, graph }, 300);
    expect(state.positions.get("pinned")).toEqual({ x: 500, y: 500 });
  });

  it("arranges the rest of the district around multiple pinned landmarks", () => {
    const graph = graphOf([
      ["left", "middle", 0.3],
      ["middle", "right", 0.3],
    ]);
    const nodes: SmacofNode[] = [
      { key: "left", rho: DRIFT_PINNED, anchor: { x: 0, y: 0 } },
      { key: "right", rho: DRIFT_PINNED, anchor: { x: 400, y: 0 } },
      { key: "middle", rho: DRIFT_FREE },
    ];
    const state = runSmacof({ nodes, graph }, 300);
    const middle = state.positions.get("middle")!;
    expect(state.positions.get("left")).toEqual({ x: 0, y: 0 });
    expect(state.positions.get("right")).toEqual({ x: 400, y: 0 });
    // Equidistant targets put it between the landmarks.
    expect(middle.x).toBeGreaterThan(50);
    expect(middle.x).toBeLessThan(350);
  });
});

describe("drift penalty", () => {
  const baseEdges: Array<[string, string, number]> = [
    ["a", "b", 0.25],
    ["b", "c", 0.25],
    ["c", "d", 0.25],
    ["a", "d", 0.5],
  ];
  const existing = ["a", "b", "c", "d"];

  function settledLayout() {
    const state = runSmacof(
      { nodes: nodesOf(existing, DRIFT_FREE), graph: graphOf(baseEdges) },
      400,
    );
    const anchors: Record<string, GardenPosition> = {};
    for (const key of existing) anchors[key] = state.positions.get(key)!;
    return { state, anchors };
  }

  it("keeps existing entities in place when a new one is inserted", () => {
    // The stability contract: insertion is additive and the interior is quiet.
    const { state: before, anchors } = settledLayout();

    const withInsert = runSmacof(
      {
        nodes: [
          ...existing.map<SmacofNode>((key) => ({
            key,
            rho: DRIFT_SETTLED,
            anchor: anchors[key],
          })),
          { key: "new", rho: DRIFT_NEW },
        ],
        graph: graphOf([...baseEdges, ["a", "new", 0.25]]),
      },
      400,
    );

    const drift = maxDisplacement(before.positions, withInsert.positions);
    // Without the drift term the same insertion moves the district by hundreds
    // of world units; the term confines it to a small local adjustment.
    expect(drift).toBeLessThan(0.2 * DEFAULT_LAYOUT_SCALE);
    expect(withInsert.positions.get("new")).toBeDefined();
  });

  it("lets a free layout move much further than a drift-penalized one", () => {
    const { state: before, anchors } = settledLayout();
    const extendedEdges: Array<[string, string, number]> = [
      ...baseEdges,
      ["a", "new", 0.25],
      ["b", "new", 0.25],
    ];

    const penalized = runSmacof(
      {
        nodes: [
          ...existing.map<SmacofNode>((key) => ({ key, rho: DRIFT_VISITED, anchor: anchors[key] })),
          { key: "new", rho: DRIFT_NEW },
        ],
        graph: graphOf(extendedEdges),
      },
      400,
    );
    const free = runSmacof(
      {
        nodes: [
          ...existing.map<SmacofNode>((key) => ({ key, rho: DRIFT_FREE, anchor: anchors[key] })),
          { key: "new", rho: DRIFT_NEW },
        ],
        graph: graphOf(extendedEdges),
      },
      400,
    );
    expect(maxDisplacement(before.positions, penalized.positions)).toBeLessThan(
      maxDisplacement(before.positions, free.positions),
    );
  });

  it("resists motion in proportion to stiffness", () => {
    const { anchors } = settledLayout();
    const displaced: Record<string, GardenPosition> = {
      ...anchors,
      a: { x: anchors.a.x + 300, y: anchors.a.y + 300 },
    };
    const drifts = [DRIFT_NEW, DRIFT_SETTLED, DRIFT_VISITED].map((rho) => {
      const state = runSmacof(
        {
          nodes: existing.map<SmacofNode>((key) => ({ key, rho, anchor: displaced[key] })),
          graph: graphOf(baseEdges),
        },
        200,
      );
      return Math.hypot(
        state.positions.get("a")!.x - displaced.a.x,
        state.positions.get("a")!.y - displaced.a.y,
      );
    });
    expect(drifts[0]).toBeGreaterThan(drifts[1]);
    expect(drifts[1]).toBeGreaterThan(drifts[2]);
  });
});

describe("interruptible iteration", () => {
  const graph = graphOf([
    ["a", "b", 0.3],
    ["b", "c", 0.4],
    ["c", "d", 0.3],
  ]);

  it("reaches the same result in batches as in one run", () => {
    // Batching is what keeps a district reflow off the animation critical path.
    const keys = ["a", "b", "c", "d"];
    const oneShot = runSmacof({ nodes: nodesOf(keys, DRIFT_FREE), graph }, 40);

    let batched = initSmacof({ nodes: nodesOf(keys, DRIFT_FREE), graph });
    while (!batched.converged && batched.iterations < 40) {
      batched = smacofStep(batched, 5);
    }
    for (const key of keys) {
      expect(batched.positions.get(key)!.x).toBeCloseTo(oneShot.positions.get(key)!.x, 6);
    }
  });

  it("monotonically reduces stress", () => {
    let state = initSmacof({ nodes: nodesOf(["a", "b", "c", "d"], DRIFT_FREE), graph });
    let previous = state.stress;
    for (let batch = 0; batch < 6 && !state.converged; batch += 1) {
      state = smacofStep(state, 3);
      expect(state.stress).toBeLessThanOrEqual(previous + 1e-9);
      previous = state.stress;
    }
  });

  it("reports convergence and stops advancing", () => {
    let state = initSmacof({ nodes: nodesOf(["a", "b", "c", "d"], DRIFT_FREE), graph });
    for (let batch = 0; batch < 60 && !state.converged; batch += 1) {
      state = smacofStep(state, 5);
    }
    expect(state.converged).toBe(true);
    const settled = state.iterations;
    expect(smacofStep(state, 5).iterations).toBe(settled);
  });

  it("rejects a state it did not produce", () => {
    expect(() =>
      smacofStep({ keys: [], positions: new Map(), iterations: 0, stress: 0, converged: false }),
    ).toThrow(/initSmacof/);
  });
});

describe("performance", () => {
  it("lays out a full district well inside a frame budget", () => {
    // Districts cap n precisely so this stays true; 80 members is the practical
    // ceiling before parcels split the district.
    const keys = Array.from({ length: 80 }, (_, i) => `n${String(i).padStart(2, "0")}`);
    const edges: Array<[string, string, number]> = [];
    for (let i = 0; i < keys.length; i += 1) {
      for (let offset = 1; offset <= 8; offset += 1) {
        const j = (i + offset) % keys.length;
        edges.push([keys[i], keys[j], 0.15 + 0.05 * offset]);
      }
    }
    const graph = graphOf(edges);

    const started = performance.now();
    const state = runSmacof({ nodes: nodesOf(keys, DRIFT_FREE), graph }, 50);
    const elapsed = performance.now() - started;

    expect(state.iterations).toBeGreaterThan(0);
    expect(elapsed).toBeLessThan(150);
  });
});

describe("seeding a metrically degenerate parcel", () => {
  /** Extent of the seed pattern for `count` nodes with no anchor and no edges. */
  function seededExtent(count: number): number {
    const nodes = Array.from({ length: count }, (_, i) => ({
      key: `n${String(i).padStart(3, "0")}`,
      rho: DRIFT_NEW,
    }));
    const state = initSmacof({ nodes, graph: new Map(), center: { x: 0, y: 0 } });
    let extent = 0;
    for (const node of nodes) {
      const position = state.positions.get(node.key)!;
      extent = Math.max(extent, Math.hypot(position.x, position.y));
    }
    return extent;
  }

  it("grows the seed radius as sqrt(n), not linearly", () => {
    // Entities that share no distinguishing facet have no neighbours to be
    // pulled toward, so they stay near their seeds and the seed pattern *is*
    // the layout. A radius growing linearly in the node index therefore made a
    // degenerate group's extent scale with n: thirty workflows carrying nothing
    // but their own ids smeared across ~1800 world units, which then set the
    // grid pitch for every district on the map.
    const small = seededExtent(4);
    const large = seededExtent(64);
    // sqrt(64/4) == 4. Linear growth would be nearer 16x.
    expect(large / small).toBeGreaterThan(3);
    expect(large / small).toBeLessThan(6);
  });

  it("keeps a realistic degenerate group compact", () => {
    // Thirty unplaceable workflows is the case that broke the map.
    expect(seededExtent(30)).toBeLessThan(2 * DEFAULT_LAYOUT_SCALE);
  });

  it("separates coincident seeds so overlap removal has little to undo", () => {
    const nodes = Array.from({ length: 12 }, (_, i) => ({
      key: `n${String(i).padStart(3, "0")}`,
      rho: DRIFT_NEW,
    }));
    const state = initSmacof({ nodes, graph: new Map(), center: { x: 0, y: 0 } });
    const seen = new Set<string>();
    for (const node of nodes) {
      const position = state.positions.get(node.key)!;
      seen.add(`${Math.round(position.x)},${Math.round(position.y)}`);
    }
    expect(seen.size).toBe(nodes.length);
  });

  it("is deterministic and independent of input order", () => {
    const keys = Array.from({ length: 10 }, (_, i) => `n${String(i).padStart(3, "0")}`);
    const build = (order: string[]) =>
      initSmacof({
        nodes: order.map((key) => ({ key, rho: DRIFT_NEW })),
        graph: new Map(),
        center: { x: 0, y: 0 },
      });
    const forward = build(keys);
    const reversed = build([...keys].reverse());
    for (const key of keys) {
      expect(reversed.positions.get(key)).toEqual(forward.positions.get(key));
    }
  });
});
