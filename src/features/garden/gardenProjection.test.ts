import { describe, expect, it } from "vitest";
import type { AgentConfig } from "../../types";
import type { AgentTeam } from "../../layout/watchlist/types";
import type { AgentGraphProjection } from "../graph/graphProjection";
import { buildGardenUnits, type GardenWorkflowInput } from "./gardenProjection";
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

describe("buildGardenUnits", () => {
  it("emits one unit per agent and workflow", () => {
    const result = buildGardenUnits({
      projection: projectionOf(nodes),
      teams,
      workflows,
      scene: createScene(),
    });
    expect(result.agentUnits.map((unit) => unit.ref.id)).toEqual(["a1", "a2", "b1"]);
    expect(result.workflowUnits.map((unit) => unit.ref.id)).toEqual(["w1"]);
  });

  it("carries label, status, and colour through from the projection", () => {
    const result = buildGardenUnits({
      projection: projectionOf(nodes),
      teams,
      workflows,
      scene: createScene(),
    });
    const alpha = result.agentUnits.find((unit) => unit.ref.id === "a1")!;
    expect(alpha.label).toBe("Alpha");
    expect(alpha.status).toBe("Idle");
    expect(alpha.color).toBe("var(--color-wardian-success)");
  });

  it("places teammates closer than agents from another team", () => {
    // The property the phyllotaxis spiral could not provide: distance means
    // something.
    const result = buildGardenUnits({
      projection: projectionOf(nodes),
      teams,
      workflows: [],
      scene: createScene(),
    });
    const byId = new Map(result.agentUnits.map((unit) => [unit.ref.id, unit.position]));
    expect(distance(byId.get("a1")!, byId.get("a2")!)).toBeLessThan(
      distance(byId.get("a1")!, byId.get("b1")!),
    );
  });

  it("keeps geometry independent of status, so a status change cannot move a unit", () => {
    // The invariant the whole design rests on: telemetry is a display channel.
    const idle = buildGardenUnits({
      projection: projectionOf(nodes),
      teams,
      workflows,
      scene: createScene(),
    });
    const busy = buildGardenUnits({
      projection: projectionOf(
        nodes.map((entry) => ({ ...entry, status: "Processing", color: "var(--x)" })),
      ),
      teams,
      workflows,
      scene: createScene(),
    });
    expect(busy.agentUnits.map((unit) => unit.position)).toEqual(
      idle.agentUnits.map((unit) => unit.position),
    );
  });

  it("reports where each unit sits so a drag can become a district-relative pin", () => {
    const result = buildGardenUnits({
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
    const first = buildGardenUnits({
      projection: projectionOf(nodes),
      teams,
      workflows,
      scene: createScene(),
    });
    const origin = first.placement.get("agent:a1")!.districtOrigin;
    const pinned = pinEntity(first.scene, "agent:a1", "team:hw", { x: 12, y: 34 }, origin);

    const result = buildGardenUnits({
      projection: projectionOf(nodes),
      teams,
      workflows,
      scene: pinned,
    });
    expect(result.agentUnits.find((unit) => unit.ref.id === "a1")!.position).toEqual({
      x: 12,
      y: 34,
    });
  });

  it("returns a scene carrying district cells and settled positions", () => {
    const result = buildGardenUnits({
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
    const result = buildGardenUnits({
      projection: projectionOf([]),
      teams: [],
      workflows: [],
      scene: createScene(),
    });
    expect(result.agentUnits).toEqual([]);
    expect(result.workflowUnits).toEqual([]);
  });
});
