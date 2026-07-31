import type { RunStatusKind } from "../workflows/run/runTypes";

/**
 * Kinds the Garden renders today.
 *
 * A subset of `EntityRef`'s vocabulary — this is what has a renderer and a
 * district rule, not what the metric can place. Library entries were the first
 * non-agent kinds to admit because their deployment records already give them a
 * defensible home: a skill belongs where it is deployed, which is a canonical
 * fact rather than a guess.
 */
export type GardenEntityKind = "agent" | "workflow" | "skill" | "prompt" | "class";

/** Library-backed kinds, which share one renderer. */
export type GardenLibraryKind = Extract<GardenEntityKind, "skill" | "prompt" | "class">;

export interface GardenEntityRef {
  kind: GardenEntityKind;
  id: string;
}

export interface GardenPosition {
  x: number;
  y: number;
}

export interface GardenAgentUnit {
  ref: GardenEntityRef; // kind === "agent"
  label: string;
  status: string;
  color: string; // may be a CSS var() expression; resolve before Konva fill
  position: GardenPosition;
}

export type GardenWorkflowRunStatus = RunStatusKind | "none";

export interface GardenWorkflowUnit {
  ref: GardenEntityRef; // kind === "workflow"
  label: string;
  runStatus: GardenWorkflowRunStatus;
  nodeCount: number;
  position: GardenPosition;
}

export interface GardenLibraryUnit {
  ref: GardenEntityRef; // kind is one of GardenLibraryKind
  /**
   * The library's own `<section>/<rel_path>` identity, verbatim.
   *
   * Carried separately from `ref.id` because `EntityRef` lowercases library
   * refs for case-insensitive matching, while the Library's own lookup is
   * case-sensitive — reconstructing `classes/Architect` from a lowercased id
   * would fail to select anything.
   */
  entryRef: string;
  label: string;
  /**
   * How many places this asset is deployed to. Zero is meaningful: an
   * undeployed skill is inert, and the map should say so rather than drawing it
   * like a live one.
   */
  deploymentCount: number;
  /** True when at least one deployment is a copy rather than a live junction. */
  hasCopiedDeployment: boolean;
  position: GardenPosition;
}

export function unitKey(ref: GardenEntityRef): string {
  return `${ref.kind}:${ref.id}`;
}
