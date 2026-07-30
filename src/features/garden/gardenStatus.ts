import type { GardenWorkflowRunStatus } from "./garden.types";
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
 * Library assets have no runtime status — their meaningful state is how widely
 * they are deployed, which is also what places them on the map. "Not deployed"
 * is the interesting case: it marks an asset nothing currently uses.
 */
export function gardenLibraryDeploymentLabel(deploymentCount: number): string {
  if (deploymentCount <= 0) return "Not deployed";
  return deploymentCount === 1 ? "Deployed to 1 target" : `Deployed to ${deploymentCount} targets`;
}
