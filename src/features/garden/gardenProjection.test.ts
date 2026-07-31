import { describe, expect, it } from "vitest";
import type { AgentConfig } from "../../types";
import type { AgentTeam } from "../../layout/watchlist/types";
import type { AgentGraphProjection } from "../graph/graphProjection";
import {
  buildAgentUnits,
  buildWorkflowUnits,
  computeGardenLayout,
  gardenLayoutSignature,
  type GardenWorkflowInput,
} from "./gardenProjection";
import { createScene, pinEntity } from "./gardenScene";
import { COMMONS_DISTRICT_ID } from "./districts";

function agent(id: string, folder: string, agentClass = "Coder"): AgentConfig {
  return {
    session_id: id,
    session_name: id,
    agent_class: agentClass,
    folder,
    is_off: false,
  } as AgentConfig;
}

function node(id: string, label: string, folder: string, agentClass = "Coder") {
  return {
    id,
    label,
    status: "Idle",
    color: "var(--color-wardian-success)",
    x: 0,
    y: 0,
    size: 9,
    agent: agent(id, folder, agentClass),
    clusterId: null,
    selected: false,
  };
}

function projectionOf(
  nodes: ReturnType<typeof node>[],
  commEdges: AgentGraphProjection["commEdges"] = [],
): AgentGraphProjection {
  return {
    nodes,
    edges: [],
    clusters: [],
    visibleAgents: [],
    scopeLabel: "All",
    commEdges,
  } as unknown as AgentGraphProjection;
}

const teams: AgentTeam[] = [
  { id: "hw", name: "Hardware", agentIds: ["a1", "a2"] },
  { id: "web", name: "Web", agentIds: ["b1"] },
] as AgentTeam[];

const nodes = [
  node("a1", "Alpha", "D:\\Dev\\Hardware"),
  node("a2", "Beta", "D:\\Dev\\Hardware"),
  node("b1", "Gamma", "D:\\Dev\\Web"),
];

const workflows: GardenWorkflowInput[] = [
  { id: "w1", label: "Build", runStatus: "running", nodeCount: 3 },
];

function distance(a: { x: number; y: number }, b: { x: number; y: number }) {
  return Math.hypot(a.x - b.x, a.y - b.y);
}

describe("computeGardenLayout", () => {
  it("emits a position for every agent and workflow", () => {
    const result = computeGardenLayout({
      projection: projectionOf(nodes),
      teams,
      workflows,
      scene: createScene(),
    });
    expect([...result.positions.keys()].sort()).toEqual([
      "agent:a1",
      "agent:a2",
      "agent:b1",
      "workflow:w1",
    ]);
  });

  it("places teammates closer than agents from another team", () => {
    // The property the phyllotaxis spiral could not provide: distance means
    // something.
    const result = computeGardenLayout({
      projection: projectionOf(nodes),
      teams,
      workflows: [],
      scene: createScene(),
    });
    expect(
      distance(result.positions.get("agent:a1")!, result.positions.get("agent:a2")!),
    ).toBeLessThan(
      distance(result.positions.get("agent:a1")!, result.positions.get("agent:b1")!),
    );
  });

  it("reports where each unit sits so a drag can become a district-relative pin", () => {
    const result = computeGardenLayout({
      projection: projectionOf(nodes),
      teams,
      workflows,
      scene: createScene(),
    });
    expect(result.placement.get("agent:a1")?.districtId).toBe("team:hw");
    expect(result.placement.get("agent:b1")?.districtId).toBe("team:web");
    // A blueprint has no durable binding until a run assigns roles.
    expect(result.placement.get("workflow:w1")?.districtId).toBe(COMMONS_DISTRICT_ID);
  });

  it("honours a pin exactly", () => {
    const first = computeGardenLayout({
      projection: projectionOf(nodes),
      teams,
      workflows,
      scene: createScene(),
    });
    const origin = first.placement.get("agent:a1")!.districtOrigin;
    const pinned = pinEntity(first.scene, "agent:a1", "team:hw", { x: 12, y: 34 }, origin);

    const result = computeGardenLayout({
      projection: projectionOf(nodes),
      teams,
      workflows,
      scene: pinned,
    });
    expect(result.positions.get("agent:a1")).toEqual({ x: 12, y: 34 });
  });

  it("returns a scene carrying district cells and settled positions", () => {
    const result = computeGardenLayout({
      projection: projectionOf(nodes),
      teams,
      workflows,
      scene: createScene(),
    });
    expect(Object.keys(result.scene.districts.cells).sort()).toEqual([
      COMMONS_DISTRICT_ID,
      "team:hw",
      "team:web",
    ]);
    expect(result.scene.positions["agent:a1"]).toBeDefined();
  });

  it("handles an empty roster", () => {
    const result = computeGardenLayout({
      projection: projectionOf([]),
      teams: [],
      workflows: [],
      scene: createScene(),
    });
    expect(result.positions.size).toBe(0);
  });
});

describe("gardenLayoutSignature", () => {
  it("ignores status, colour, and selection", () => {
    // These are display channels. If they entered the signature, every telemetry
    // tick would rerun the pipeline and nudge the whole map.
    const base = gardenLayoutSignature(projectionOf(nodes), teams, workflows);
    const repainted = gardenLayoutSignature(
      projectionOf(
        nodes.map((entry) => ({
          ...entry,
          status: "Processing",
          color: "var(--x)",
          selected: true,
        })),
      ),
      teams,
      workflows,
    );
    expect(repainted).toBe(base);
  });

  it("ignores continuous edge recency but reacts to a state change", () => {
    // Recency is recomputed against the wall clock; letting it through would
    // make the map breathe as conversations age.
    const withEdge = (recency: number, state: string) =>
      gardenLayoutSignature(
        projectionOf(nodes, [
          { id: "a1--a2", source: "a1", target: "a2", origin: "manual", state, recency },
        ] as unknown as AgentGraphProjection["commEdges"]),
        teams,
        workflows,
      );
    expect(withEdge(0.9, "recent")).toBe(withEdge(0.4, "recent"));
    expect(withEdge(0.9, "recent")).not.toBe(withEdge(0.9, "ongoing"));
  });

  it("reacts to roster, folder, and team membership changes", () => {
    const base = gardenLayoutSignature(projectionOf(nodes), teams, workflows);
    expect(
      gardenLayoutSignature(projectionOf(nodes.slice(0, 2)), teams, workflows),
    ).not.toBe(base);
    expect(
      gardenLayoutSignature(
        projectionOf([node("a1", "Alpha", "D:\Dev\Moved"), nodes[1], nodes[2]]),
        teams,
        workflows,
      ),
    ).not.toBe(base);
    expect(
      gardenLayoutSignature(projectionOf(nodes), [
        { id: "hw", name: "Hardware", agentIds: ["a1"] },
        { id: "web", name: "Web", agentIds: ["b1"] },
      ] as AgentTeam[], workflows),
    ).not.toBe(base);
  });

  it("is stable under input reordering", () => {
    expect(gardenLayoutSignature(projectionOf([...nodes].reverse()), [...teams].reverse(), workflows)).toBe(
      gardenLayoutSignature(projectionOf(nodes), teams, workflows),
    );
  });
});

describe("display attachment", () => {
  it("attaches live label, status, and colour to computed positions", () => {
    const layout = computeGardenLayout({
      projection: projectionOf(nodes),
      teams,
      workflows,
      scene: createScene(),
    });
    const units = buildAgentUnits(
      projectionOf(nodes.map((entry) => ({ ...entry, status: "Processing" }))),
      layout.positions,
      layout.crowns,
    );
    const alpha = units.find((unit) => unit.ref.id === "a1")!;
    expect(alpha.label).toBe("Alpha");
    expect(alpha.status).toBe("Processing");
    expect(alpha.position).toEqual(layout.positions.get("agent:a1"));
    expect(alpha.crown).toEqual([]);
  });

  it("attaches workflow run status without touching position", () => {
    const layout = computeGardenLayout({
      projection: projectionOf(nodes),
      teams,
      workflows,
      scene: createScene(),
    });
    const units = buildWorkflowUnits(workflows, layout.positions);
    expect(units[0]).toMatchObject({ label: "Build", runStatus: "running", nodeCount: 3 });
    expect(units[0].position).toEqual(layout.positions.get("workflow:w1"));
  });
});


describe("skills as agent attributes", () => {
  const kicad = {
    entryRef: "skills/kicad",
    label: "KiCad Review",
    tags: ["hardware"],
    deployments: [{ targetType: "agent", targetId: "a1", linked: true }],
  };

  it("places no unit for a skill", () => {
    // A skill deployed to several agents cannot be near all of them, so it has
    // no defensible position at all. It renders on its carriers instead.
    const result = computeGardenLayout({
      projection: projectionOf(nodes),
      teams,
      workflows: [],
      skills: [kicad],
      scene: createScene(),
    });
    expect([...result.positions.keys()].sort()).toEqual(["agent:a1", "agent:a2", "agent:b1"]);
  });

  it("hangs the skill on the agent it is deployed to, and on nobody else", () => {
    const result = computeGardenLayout({
      projection: projectionOf(nodes),
      teams,
      workflows: [],
      skills: [kicad],
      scene: createScene(),
    });
    expect(result.crowns.get("a1")?.map((glyph) => glyph.entryRef)).toEqual(["skills/kicad"]);
    expect(result.crowns.has("a2")).toBe(false);
  });

  it("reaches every agent of a class, which a skill unit could never do", () => {
    // The decisive case for instancing: a class deployment has no single agent
    // to sit beside, so the unit model had nowhere to put it.
    const mixed = [
      node("a1", "Alpha", "D:\Dev\Hardware", "Architect"),
      node("a2", "Beta", "D:\Dev\Hardware", "Architect"),
      node("b1", "Gamma", "D:\Dev\Web", "Coder"),
    ];
    const result = computeGardenLayout({
      projection: projectionOf(mixed),
      teams,
      workflows: [],
      skills: [
        { ...kicad, deployments: [{ targetType: "class", targetId: "Architect", linked: true }] },
      ],
      scene: createScene(),
    });
    expect(result.crowns.get("a1")?.[0]).toMatchObject({ provenance: "class" });
    expect(result.crowns.get("a2")?.[0]).toMatchObject({ provenance: "class" });
    expect(result.crowns.has("b1")).toBe(false);
  });

  it("pulls two agents sharing a rare skill closer than an agent without it", () => {
    // Skills leave the unit set but stay in the metric, and this is why: the
    // shared `skill:` token is rare, so IDF makes it a strong attractor. The
    // three agents are otherwise identical — same team, same folder, same class.
    const peers = [
      node("a1", "Alpha", "D:\Dev\Hardware"),
      node("a2", "Beta", "D:\Dev\Hardware"),
      node("a3", "Delta", "D:\Dev\Hardware"),
    ];
    const oneTeam: AgentTeam[] = [
      { id: "hw", name: "Hardware", agentIds: ["a1", "a2", "a3"] },
    ] as AgentTeam[];
    const result = computeGardenLayout({
      projection: projectionOf(peers),
      teams: oneTeam,
      workflows: [],
      skills: [
        {
          ...kicad,
          deployments: [
            { targetType: "agent", targetId: "a1", linked: true },
            { targetType: "agent", targetId: "a2", linked: true },
          ],
        },
      ],
      scene: createScene(),
    });
    const [alpha, beta, delta] = ["a1", "a2", "a3"].map(
      (id) => result.positions.get(`agent:${id}`)!,
    );
    expect(distance(alpha, beta)).toBeLessThan(distance(alpha, delta));
  });

  it("orders a crown by IDF, so a universal skill sinks below a rare one", () => {
    // Otherwise a skill deployed everywhere renders on every agent and swamps
    // the crown with the one thing that distinguishes nobody.
    const result = computeGardenLayout({
      projection: projectionOf(nodes),
      teams,
      workflows: [],
      skills: [
        { ...kicad, entryRef: "skills/everywhere", label: "Everywhere",
          deployments: [{ targetType: "user", targetId: "global", linked: true }] },
        kicad,
      ],
      scene: createScene(),
    });
    expect(result.crowns.get("a1")?.map((glyph) => glyph.entryRef)).toEqual([
      "skills/kicad",
      "skills/everywhere",
    ]);
    // Carried by every agent, so it is the only thing b1 has.
    expect(result.crowns.get("b1")?.map((glyph) => glyph.entryRef)).toEqual([
      "skills/everywhere",
    ]);
  });

  it("attaches the crown to the agent unit", () => {
    const layout = computeGardenLayout({
      projection: projectionOf(nodes),
      teams,
      workflows: [],
      skills: [kicad],
      scene: createScene(),
    });
    const units = buildAgentUnits(projectionOf(nodes), layout.positions, layout.crowns);
    expect(units.find((unit) => unit.ref.id === "a1")!.crown).toHaveLength(1);
    expect(units.find((unit) => unit.ref.id === "a2")!.crown).toEqual([]);
  });

  it("relayouts on a redeployment but not on a rename or a copy fallback", () => {
    // A copy and a junction are the same tie for distance purposes and differ
    // only in how the glyph is stroked.
    const base = gardenLayoutSignature(projectionOf(nodes), teams, workflows, [kicad]);
    expect(
      gardenLayoutSignature(projectionOf(nodes), teams, workflows, [
        { ...kicad, label: "Renamed", tags: ["other"] },
      ]),
    ).toBe(base);
    expect(
      gardenLayoutSignature(projectionOf(nodes), teams, workflows, [
        { ...kicad, deployments: [{ targetType: "agent", targetId: "a1", linked: false }] },
      ]),
    ).toBe(base);
    expect(
      gardenLayoutSignature(projectionOf(nodes), teams, workflows, [
        { ...kicad, deployments: [{ targetType: "agent", targetId: "b1", linked: true }] },
      ]),
    ).not.toBe(base);
  });
});
