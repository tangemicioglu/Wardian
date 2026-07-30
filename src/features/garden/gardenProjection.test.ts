import { describe, expect, it } from "vitest";
import type { AgentConfig } from "../../types";
import type { AgentTeam } from "../../layout/watchlist/types";
import type { AgentGraphProjection } from "../graph/graphProjection";
import {
  buildAgentUnits,
  buildLibraryUnits,
  buildWorkflowUnits,
  computeGardenLayout,
  gardenLayoutSignature,
  type GardenWorkflowInput,
} from "./gardenProjection";
import { createScene, pinEntity } from "./gardenScene";
import { COMMONS_DISTRICT_ID } from "./districts";

function agent(id: string, folder: string): AgentConfig {
  return {
    session_id: id,
    session_name: id,
    agent_class: "Coder",
    folder,
    is_off: false,
  } as AgentConfig;
}

function node(id: string, label: string, folder: string) {
  return {
    id,
    label,
    status: "Idle",
    color: "var(--color-wardian-success)",
    x: 0,
    y: 0,
    size: 9,
    agent: agent(id, folder),
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
    );
    const alpha = units.find((unit) => unit.ref.id === "a1")!;
    expect(alpha.label).toBe("Alpha");
    expect(alpha.status).toBe("Processing");
    expect(alpha.position).toEqual(layout.positions.get("agent:a1"));
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

describe("library units", () => {
  const skill = {
    entryRef: "skills/kicad",
    kind: "skill" as const,
    label: "KiCad Review",
    tags: ["hardware"],
    deployments: [{ targetType: "agent", targetId: "a1", linked: true }],
  };

  it("places a deployed skill in its target agent's district", () => {
    // Deployment is a canonical record, so this placement is defensible rather
    // than inferred.
    const result = computeGardenLayout({
      projection: projectionOf(nodes),
      teams,
      workflows,
      library: [skill],
      scene: createScene(),
    });
    expect(result.placement.get("skill:skills/kicad")?.districtId).toBe("team:hw");
  });

  it("puts an undeployed asset in the commons rather than guessing", () => {
    const result = computeGardenLayout({
      projection: projectionOf(nodes),
      teams,
      workflows,
      library: [{ ...skill, deployments: [] }],
      scene: createScene(),
    });
    expect(result.placement.get("skill:skills/kicad")?.districtId).toBe(COMMONS_DISTRICT_ID);
  });

  it("lands a skill nearer its deployment target than an unrelated agent", () => {
    const result = computeGardenLayout({
      projection: projectionOf(nodes),
      teams,
      workflows: [],
      library: [skill],
      scene: createScene(),
    });
    const position = result.positions.get("skill:skills/kicad")!;
    expect(distance(position, result.positions.get("agent:a1")!)).toBeLessThan(
      distance(position, result.positions.get("agent:b1")!),
    );
  });

  it("reports deployment count and copied deployments on the unit", () => {
    const layout = computeGardenLayout({
      projection: projectionOf(nodes),
      teams,
      workflows,
      library: [
        {
          ...skill,
          deployments: [
            { targetType: "agent", targetId: "a1", linked: true },
            { targetType: "class", targetId: "Architect", linked: false },
          ],
        },
      ],
      scene: createScene(),
    });
    const units = buildLibraryUnits(
      [
        {
          ...skill,
          deployments: [
            { targetType: "agent", targetId: "a1", linked: true },
            { targetType: "class", targetId: "Architect", linked: false },
          ],
        },
      ],
      layout.positions,
    );
    expect(units[0]).toMatchObject({
      label: "KiCad Review",
      deploymentCount: 2,
      hasCopiedDeployment: true,
    });
    expect(units[0].ref).toEqual({ kind: "skill", id: "skills/kicad" });
  });

  it("keeps a renamed asset from provoking a relayout, but a redeployment does", () => {
    // Labels are display; deployment targets move the asset between districts.
    const base = gardenLayoutSignature(projectionOf(nodes), teams, workflows, [skill]);
    expect(
      gardenLayoutSignature(projectionOf(nodes), teams, workflows, [
        { ...skill, label: "Renamed", tags: ["other"] },
      ]),
    ).toBe(base);
    expect(
      gardenLayoutSignature(projectionOf(nodes), teams, workflows, [
        { ...skill, deployments: [{ targetType: "agent", targetId: "b1", linked: true }] },
      ]),
    ).not.toBe(base);
  });
});
