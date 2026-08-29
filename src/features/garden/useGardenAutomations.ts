import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Blueprint } from "../automations/builder/blueprintTypes";
import type { RunSummary, RunSummaryListResult } from "../automations/run/runTypes";
import type { GardenAutomationInput } from "./gardenProjection";
import { automationContextOf, type AutomationContext } from "./automationContext";
import type { BlueprintListResult, BlueprintRef } from "../automations/automationTypes";
import {
  deploymentsByBlueprint,
  type AutomationScheduleRecord,
} from "./automationDeployments";

interface ParsedBlueprint {
  id: string;
  name: string;
  nodeCount: number;
  context: AutomationContext;
}

type GardenInvoke = (command: string, args?: Record<string, unknown>) => Promise<unknown>;

/** Failure values that keep the page shape `automation_list_*` declares. */
const EMPTY_BLUEPRINT_PAGE: BlueprintListResult = { blueprints: [], truncated: false, next_offset: null };
const EMPTY_RUN_PAGE: RunSummaryListResult = { runs: [], truncated: false, next_offset: null };

export interface GardenAutomationInputsResult {
  automations: GardenAutomationInput[];
  truncated: boolean;
  nextOffset: number | null;
}

export interface GardenAutomationHookResult extends GardenAutomationInputsResult {
  loadMore: () => Promise<void>;
}

let cachedBlueprintKey: string | null = null;
let cachedBlueprints: ParsedBlueprint[] = [];

/**
 * Pure: attach each blueprint's most-recent run status (by updated_at) and the
 * agents its schedules deploy it onto.
 */
export function mergeAutomationRunStatus(
  blueprints: ParsedBlueprint[],
  runs: RunSummary[],
  deployments: ReadonlyMap<string, string[]> = new Map(),
): GardenAutomationInput[] {
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

export async function loadGardenAutomationInputs(
  invoker: GardenInvoke = invoke as GardenInvoke,
  blueprintOffset = 0,
): Promise<GardenAutomationInputsResult> {
  const blueprintResult = (await (
    blueprintOffset > 0
      ? invoker("automation_list_blueprints", { offset: blueprintOffset })
      : invoker("automation_list_blueprints")
  ).catch(() => EMPTY_BLUEPRINT_PAGE)) as BlueprintListResult;
  const refs = blueprintResult.blueprints;
  const truncated = blueprintResult.truncated;
  const nextOffset = blueprintResult.next_offset;
  const nextBlueprintKey = blueprintRefsKey(refs);
  let blueprints = cachedBlueprintKey === nextBlueprintKey ? cachedBlueprints : null;

  if (!blueprints) {
    const parsedRaw = await Promise.all(
      refs.map(async (ref) => {
        const result = await invoker("automation_parse", { path: ref.path }).catch(() => null) as { blueprint?: Blueprint } | null;
        if (!result?.blueprint) return null;
        return {
          id: result.blueprint.id,
          name: result.blueprint.name,
          nodeCount: result.blueprint.nodes.length,
          // Read once, at parse time, and cached with the blueprint: this is
          // what gives an automation somewhere to be other than the commons.
          context: automationContextOf(result.blueprint, ref.path),
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
  const [runResult, schedules] = await Promise.all([
    invoker("automation_list_runs").catch(() => EMPTY_RUN_PAGE) as Promise<RunSummaryListResult>,
    invoker("schedule_list").catch(() => []) as Promise<AutomationScheduleRecord[]>,
  ]);
  return {
    automations: mergeAutomationRunStatus(
      blueprints,
      runResult.runs,
      deploymentsByBlueprint(schedules),
    ),
    truncated,
    nextOffset,
  };
}

export function resetGardenAutomationCacheForTests() {
  cachedBlueprintKey = null;
  cachedBlueprints = [];
}

function blueprintRefsKey(refs: BlueprintRef[]) {
  return JSON.stringify(refs.map((ref) => [ref.id, ref.path]));
}

/** Loads the blueprint catalog (list + parse, mirroring AutomationsView) and merges run status. */
export function useGardenAutomations(enabled = true): GardenAutomationHookResult {
  const [result, setResult] = useState<GardenAutomationInputsResult>({ automations: [], truncated: false, nextOffset: null });

  const load = useCallback(async () => {
    setResult(await loadGardenAutomationInputs());
  }, []);

  const loadMore = useCallback(async () => {
    if (result.nextOffset === null) return;
    const page = await loadGardenAutomationInputs(invoke as GardenInvoke, result.nextOffset);
    setResult((current) => ({
      automations: [
        ...current.automations,
        ...page.automations.filter((candidate) => !current.automations.some((existing) => existing.id === candidate.id)),
      ],
      truncated: page.truncated,
      nextOffset: page.nextOffset,
    }));
  }, [result.nextOffset]);

  useEffect(() => {
    if (!enabled) return;
    void load();
  }, [enabled, load]);

  return { ...result, loadMore };
}
