import type { RunStatusKind } from "../workflows/run/runTypes";
import type { GardenSkillGlyph } from "./skillGlyphs";

/**
 * Kinds the Garden can address.
 *
 * A subset of `EntityRef`'s vocabulary. Only `agent` and `workflow` are
 * *placed*: `skill` is addressable so a glyph can be selected and deep-linked,
 * but a skill has no position of its own — it renders on the agents that carry
 * it. See `skillGlyphs.ts` for why.
 */
export type GardenEntityKind = "agent" | "workflow" | "skill";

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
  /**
   * Skills this agent carries, most distinctive first.
   *
   * Decoration, not geometry: the crown is attached to the agent's position and
   * never enters the layout, which is what lets it expand and contract with
   * zoom without any risk of moving a unit.
   */
  crown: GardenSkillGlyph[];
}

export type GardenWorkflowRunStatus = RunStatusKind | "none";

export interface GardenWorkflowUnit {
  ref: GardenEntityRef; // kind === "workflow"
  label: string;
  runStatus: GardenWorkflowRunStatus;
  nodeCount: number;
  position: GardenPosition;
}

export function unitKey(ref: GardenEntityRef): string {
  return `${ref.kind}:${ref.id}`;
}
