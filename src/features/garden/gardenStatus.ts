import type { ChangeReviewBaseline } from "../../types";
import type { GardenWorkflowRunStatus } from "./garden.types";
import type { TerrainChangeKind } from "./terrainPaint";
import { formatAgentStatusLabel, getAgentStatusColorToken, getAgentStatusIndicatorClass, normalizeAgentStatus } from "../../utils/statusUtils";
import { formatRunStatus, workflowRunStatusColor } from "../workflows/run/statusLabels";

const GARDEN_AGENT_LEGEND_STATUSES = [
  "Idle",
  "Processing",
  "Action Required",
  "Headless",
  "Restoring",
  "Off",
  "Error",
] as const;

/** Visible labels keep Garden's color-and-motion canvas readable without inference. */
export const GARDEN_AGENT_STATUS_LEGEND = GARDEN_AGENT_LEGEND_STATUSES.map((status) => ({
  label: formatAgentStatusLabel(status),
  indicatorClass: getAgentStatusIndicatorClass(status),
}));

/** Agents animate only while actively processing (cyan), per the idle-CPU rule. */
export function isActiveAgentStatus(status: string): boolean {
  const normalized = normalizeAgentStatus(status);
  return normalized === "Processing..." || normalized === "Headless" || normalized === "Restoring";
}

/** Garden uses the shared agent palette rather than a canvas-only status map. */
export function gardenAgentStatusColor(status: string): string {
  return getAgentStatusColorToken(status);
}

export function gardenAgentStatusLabel(status: string): string {
  return formatAgentStatusLabel(status);
}

/** Workflows animate only while a run is live or waiting on a human. */
export function isActiveWorkflowStatus(status: GardenWorkflowRunStatus): boolean {
  return status === "running" || status === "awaiting_approval";
}

/** Mirrors the agent statusToColor palette so the two perspectives read alike. */
export function workflowStatusColor(status: GardenWorkflowRunStatus): string {
  return status === "none" ? "var(--color-wardian-text-muted)" : workflowRunStatusColor(status);
}

export function gardenWorkflowStatusLabel(status: GardenWorkflowRunStatus): string {
  return status === "none" ? "No runs yet" : formatRunStatus(status);
}

/**
 * Change legend for the ground plane.
 *
 * The same hues and wording the Changes pane uses, because the two surfaces
 * describe one change set. `mixed` has no counterpart there — a sidebar row is
 * one path and cannot be mixed — and it is included because a folder holding an
 * addition and a deletion is not "modified".
 */
export const GARDEN_CHANGE_LEGEND: ReadonlyArray<{
  kind: TerrainChangeKind;
  label: string;
  colorVar: string;
}> = [
  { kind: "added", label: "Added", colorVar: "var(--color-wardian-success)" },
  { kind: "modified", label: "Modified", colorVar: "var(--color-wardian-warning)" },
  { kind: "deleted", label: "Deleted", colorVar: "var(--color-wardian-error)" },
  { kind: "renamed", label: "Renamed", colorVar: "var(--color-wardian-processing)" },
  { kind: "mixed", label: "Mixed", colorVar: "var(--color-wardian-warning)" },
];

/**
 * How the ground's baseline reads in words.
 *
 * Stated on the map rather than assumed, because the terrain uses a
 * workspace-level baseline while the Changes pane may be showing an
 * agent-scoped one. The two are allowed to differ; they are not allowed to
 * differ silently.
 */
export function gardenChangeBaselineLabel(baseline: ChangeReviewBaseline): string {
  switch (baseline) {
    case "branch_point":
      return "Changed since this branch";
    case "head":
      return "Changed since the last commit";
    default:
      return "Changed since the last commit";
  }
}

/**
 * A skill has no runtime status. What it has instead is reach — how many agents
 * on the map carry it — which is the answer the map can give that a list
 * cannot, now that a skill is drawn on its carriers rather than in one place.
 */
export function gardenSkillReachLabel(carrierCount: number): string {
  if (carrierCount <= 0) return "On no agents here";
  return carrierCount === 1 ? "On 1 agent" : `On ${carrierCount} agents`;
}
