import { describe, expect, it } from "vitest";
import { deploymentsByBlueprint } from "./workflowDeployments";

const EVOLVER = "6bc97063-1de9-4245-a90a-d7f4064fa5e0";

describe("deploymentsByBlueprint", () => {
  it("reads the agent a schedule deploys a workflow onto", () => {
    // The case this exists for: the Evolver's blueprints name `role:evolver` and
    // nothing else, so read alone they have no tie to any agent. The schedule is
    // where the role is actually filled.
    const map = deploymentsByBlueprint([
      {
        blueprint_id: "evolver-daily-skill-scan",
        assignments: {
          evolver: { target_type: "agent", agent_id: EVOLVER },
        },
      },
    ]);
    expect(map.get("evolver-daily-skill-scan")).toEqual([EVOLVER]);
  });

  it("ignores a throwaway agent spun up for one run", () => {
    // A `temporary_provider` exists for the length of a run and belongs nowhere,
    // so it cannot place a workflow.
    const map = deploymentsByBlueprint([
      {
        blueprint_id: "evolver-weekly-skillopt-batch",
        assignments: {
          reviewer: { target_type: "temporary_provider", provider: "codex" },
          evolver: { target_type: "agent", agent_id: EVOLVER },
        },
      },
    ]);
    expect(map.get("evolver-weekly-skillopt-batch")).toEqual([EVOLVER]);
  });

  it("pools the agents when several schedules deploy one blueprint", () => {
    const map = deploymentsByBlueprint([
      { blueprint_id: "scan", assignments: { a: { target_type: "agent", agent_id: "a2" } } },
      { blueprint_id: "scan", assignments: { a: { target_type: "agent", agent_id: "a1" } } },
      { blueprint_id: "scan", assignments: { a: { target_type: "agent", agent_id: "a1" } } },
    ]);
    // Sorted and de-duplicated, so the facet set does not depend on the order
    // schedules happen to be stored in.
    expect(map.get("scan")).toEqual(["a1", "a2"]);
  });

  it("omits a blueprint whose schedules bind nothing", () => {
    const map = deploymentsByBlueprint([
      { blueprint_id: "manual", assignments: {} },
      { blueprint_id: "provider-only", assignments: { r: { target_type: "temporary_provider", provider: "codex" } } },
    ]);
    expect(map.size).toBe(0);
  });

  it("trusts a legacy binding only when it names an agent that exists", () => {
    // `bindings` stores a bare string per role: an agent id for an agent target
    // and a provider name otherwise. The two are indistinguishable without the
    // roster, and treating `"codex"` as an agent id would tie the workflow to
    // nothing.
    const schedules = [
      { blueprint_id: "legacy", bindings: { evolver: EVOLVER, reviewer: "codex" } },
    ];
    expect(deploymentsByBlueprint(schedules, new Set([EVOLVER])).get("legacy")).toEqual([EVOLVER]);
    expect(deploymentsByBlueprint(schedules).size).toBe(0);
  });

  it("prefers the structured assignment over the legacy binding", () => {
    const map = deploymentsByBlueprint(
      [
        {
          blueprint_id: "both",
          assignments: { r: { target_type: "agent", agent_id: "a1" } },
          bindings: { r: "a2" },
        },
      ],
      new Set(["a1", "a2"]),
    );
    expect(map.get("both")).toEqual(["a1"]);
  });

  it("tolerates records with no blueprint or no bindings at all", () => {
    expect(deploymentsByBlueprint([{}, { blueprint_id: "x" }]).size).toBe(0);
  });
});
