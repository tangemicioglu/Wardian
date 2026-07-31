/**
 * Where a workflow is actually deployed.
 *
 * A blueprint describes what a workflow *needs* — `role:evolver` says it wants
 * whichever agent fills the evolver role. It does not say which agent that is,
 * and for most workflows the blueprint never will: the binding is made when the
 * workflow is deployed, and it lives in the schedule record rather than in the
 * document.
 *
 * That is the missing half of a workflow's identity on the map. Reading only the
 * blueprint, the Evolver's three workflows share nothing with the Evolver but a
 * word, so they sat in the commons while the agent they run on sat elsewhere.
 * The schedule says plainly that all three run as agent
 * `6bc97063-…`, which is exactly the kind of canonical record districting is
 * supposed to be built on — the same standing as a skill's deployment or an
 * artifact's origin.
 *
 * Only `target_type: "agent"` assignments count. A `temporary_provider` names a
 * throwaway agent that exists for the length of one run and belongs nowhere.
 */

export interface ScheduleAssignment {
  target_type?: string;
  agent_id?: string;
  provider?: string;
}

export interface WorkflowScheduleRecord {
  blueprint_id?: string;
  assignments?: Record<string, ScheduleAssignment> | null;
  /** Legacy shape: role -> agent id or provider name. */
  bindings?: Record<string, string> | null;
}

/**
 * Agent ids each blueprint is deployed onto, keyed by blueprint id.
 *
 * Several schedules can deploy one blueprint — the same scan running morning and
 * afternoon — so ids are pooled and de-duplicated. A blueprint deployed onto two
 * different agents keeps both: `resolveEntityDistrict` counts the votes and the
 * most-referenced district wins, which is the honest answer for a workflow that
 * genuinely spans two places.
 */
export function deploymentsByBlueprint(
  schedules: readonly WorkflowScheduleRecord[],
  knownAgentIds?: ReadonlySet<string>,
): Map<string, string[]> {
  const byBlueprint = new Map<string, Set<string>>();

  for (const schedule of schedules) {
    const blueprintId = schedule.blueprint_id;
    if (!blueprintId) continue;

    const agentIds = new Set<string>();
    for (const assignment of Object.values(schedule.assignments ?? {})) {
      if (assignment?.target_type !== "agent") continue;
      const agentId = assignment.agent_id?.trim();
      if (agentId) agentIds.add(agentId);
    }

    // `bindings` predates `assignments` and stores a bare string per role, which
    // is an agent id for an agent target and a provider name otherwise. Without
    // the structured form the two are indistinguishable, so a binding is only
    // trusted when it names an agent that exists.
    if (agentIds.size === 0 && knownAgentIds) {
      for (const value of Object.values(schedule.bindings ?? {})) {
        const candidate = value?.trim();
        if (candidate && knownAgentIds.has(candidate)) agentIds.add(candidate);
      }
    }

    if (agentIds.size === 0) continue;
    const existing = byBlueprint.get(blueprintId);
    if (existing) for (const id of agentIds) existing.add(id);
    else byBlueprint.set(blueprintId, agentIds);
  }

  // Sorted so the facet set, and therefore the layout, does not depend on the
  // order schedules happen to be stored in.
  return new Map(
    [...byBlueprint].map(([blueprintId, ids]) => [blueprintId, [...ids].sort()]),
  );
}
