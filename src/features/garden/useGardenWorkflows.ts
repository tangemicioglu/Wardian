import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Blueprint } from "../workflows/builder/blueprintTypes";
import type { RunSummary } from "../workflows/run/runTypes";
import type { GardenWorkflowInput } from "./gardenProjection";
import { workflowContextOf, type WorkflowContext } from "./workflowContext";
import {
  deploymentsByBlueprint,
  type WorkflowScheduleRecord,
} from "./workflowDeployments";

interface BlueprintRef {
  id: string;
  path: string;
}

interface ParsedBlueprint {
  id: string;
  name: string;
  nodeCount: number;
  context: WorkflowContext;
}

type GardenInvoke = (command: string, args?: Record<string, unknown>) => Promise<unknown>;

let cachedBlueprintKey: string | null = null;
let cachedBlueprints: ParsedBlueprint[] = [];

/**
 * Pure: attach each blueprint's most-recent run status (by updated_at) and the
 * agents its schedules deploy it onto.
 */
export function mergeWorkflowRunStatus(
  blueprints: ParsedBlueprint[],
  runs: RunSummary[],
  deployments: ReadonlyMap<string, string[]> = new Map(),
): GardenWorkflowInput[] {
  const latest = new Map<string, RunSummary>();
  for (const run of runs) {
    const existing = latest.get(run.blueprint_id);
    if (!existing || (run.updated_at ?? "") > (existing.updated_at ?? "")) {
      latest.set(run.blueprint_id, run);
    }
  }
  return blueprints.map((bp) => ({
    id: bp.id,
    label: bp.name,
    runStatus: latest.get(bp.id)?.status ?? "none",
    nodeCount: bp.nodeCount,
    // A blueprint's own `agent_ref` bindings and the schedules that deploy it are
    // the same kind of evidence — both name a concrete agent — so they pool.
    agentIds: [...new Set([...bp.context.agentIds, ...(deployments.get(bp.id) ?? [])])].sort(),
    roleNames: bp.context.roleNames,
    classNames: bp.context.classNames,
    workspacePaths: bp.context.workspacePaths,
    libraryFolder: bp.context.libraryFolder,
  }));
}

export async function loadGardenWorkflowInputs(invoker: GardenInvoke = invoke as GardenInvoke): Promise<GardenWorkflowInput[]> {
  // `invoke` can resolve to null (not just reject), so coalesce to [] before mapping.
  const refs = ((await invoker("workflow_list_blueprints").catch(() => [])) ?? []) as BlueprintRef[];
  const nextBlueprintKey = blueprintRefsKey(refs);
  let blueprints = cachedBlueprintKey === nextBlueprintKey ? cachedBlueprints : null;

  if (!blueprints) {
    const parsedRaw = await Promise.all(
      refs.map(async (ref) => {
        const result = await invoker("workflow_parse", { path: ref.path }).catch(() => null) as { blueprint?: Blueprint } | null;
        if (!result?.blueprint) return null;
        return {
          id: result.blueprint.id,
          name: result.blueprint.name,
          nodeCount: result.blueprint.nodes.length,
          // Read once, at parse time, and cached with the blueprint: this is
          // what gives a workflow somewhere to be other than the commons.
          context: workflowContextOf(result.blueprint, ref.path),
        } satisfies ParsedBlueprint;
      }),
    );
    blueprints = parsedRaw.filter((bp): bp is ParsedBlueprint => bp !== null);
    cachedBlueprintKey = nextBlueprintKey;
    cachedBlueprints = blueprints;
  }

  // Runs and schedules are both small, unparsed lists and independent of each
  // other, so they are fetched together rather than in sequence. Neither is
  // cached with the blueprints: run status changes constantly, and a schedule
  // can be rebound without the blueprint file changing at all.
  const [runs, schedules] = await Promise.all([
    invoker("workflow_list_runs").catch(() => []) as Promise<RunSummary[]>,
    invoker("schedule_list").catch(() => []) as Promise<WorkflowScheduleRecord[]>,
  ]);
  return mergeWorkflowRunStatus(
    blueprints,
    runs ?? [],
    deploymentsByBlueprint(schedules ?? []),
  );
}

export function resetGardenWorkflowCacheForTests() {
  cachedBlueprintKey = null;
  cachedBlueprints = [];
}

function blueprintRefsKey(refs: BlueprintRef[]) {
  return JSON.stringify(refs.map((ref) => [ref.id, ref.path]));
}

/** Loads the blueprint catalog (list + parse, mirroring WorkflowsView) and merges run status. */
export function useGardenWorkflows(enabled = true): GardenWorkflowInput[] {
  const [workflows, setWorkflows] = useState<GardenWorkflowInput[]>([]);

  const load = useCallback(async () => {
    setWorkflows(await loadGardenWorkflowInputs());
  }, []);

  useEffect(() => {
    if (!enabled) return;
    void load();
  }, [enabled, load]);

  return workflows;
}
