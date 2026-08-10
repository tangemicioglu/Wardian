/**
 * Which agent-to-ground ties the map draws.
 *
 * Pure, and separate from the Konva component for the same reason `terrain.ts`
 * is separate from `TerrainLayer.tsx`: the selection rule is the part worth
 * testing, and importing react-konva to check it would drag a native canvas
 * into the test run.
 */

import type { GardenPosition } from "./garden.types";
import type { TerrainCell } from "./terrain";
import type { TerrainPaint } from "./terrainPaint";

/**
 * Threads drawn at once.
 *
 * A busy turn touches hundreds of files, and a hundred lines converging on one
 * orb is a starburst that hides the agent it is about. The cap is by churn, so
 * what survives is the work rather than the first paths alphabetically.
 */
export const MAX_THREADS = 24;

export interface Thread {
  key: string;
  from: GardenPosition;
  to: GardenPosition;
}

/**
 * Composite key for one agent-to-path tie.
 *
 * NUL-separated, matching the pair keys in `metric.ts`. A visible separator
 * would be wrong rather than merely ugly: paths may contain spaces, commas, and
 * colons, so any printable delimiter admits two different pairs that collapse
 * to one key — and React would then drop one of the two lines.
 */
export function threadKey(agentId: string, path: string): string {
  return `${agentId}\0${path}`;
}

function centre(cell: TerrainCell): GardenPosition {
  return { x: cell.rect.x + cell.rect.width / 2, y: cell.rect.y + cell.rect.height / 2 };
}

/**
 * Which agent-to-ground ties to draw.
 *
 * Exported for test, and pure so the selection rule can be checked without a
 * canvas. Two directions, one rule: an agent selection draws its writes, a
 * ground selection draws the agents that wrote it.
 *
 * Only `attributed` paint qualifies. An `inferred` entry is a write the change
 * set saw but no turn record claimed — usually a shell command — and drawing a
 * line to an agent that never claimed it would invent the attribution the
 * evidence discriminant exists to withhold.
 */
export function threadsFor(options: {
  cells: readonly TerrainCell[];
  paint: ReadonlyMap<string, TerrainPaint>;
  positions: ReadonlyMap<string, GardenPosition>;
  selectedAgentId?: string | null;
  selectedPath?: string | null;
  max?: number;
}): Thread[] {
  const { cells, paint, positions, selectedAgentId, selectedPath } = options;
  const max = options.max ?? MAX_THREADS;
  if (!selectedAgentId && !selectedPath) return [];

  const candidates: Array<{ thread: Thread; churn: number }> = [];
  for (const cell of cells) {
    const cellPaint = paint.get(cell.path);
    if (!cellPaint || cellPaint.evidence !== "attributed") continue;
    if (selectedPath && cell.path !== selectedPath) continue;

    const agentIds = selectedAgentId
      ? cellPaint.agentIds.includes(selectedAgentId)
        ? [selectedAgentId]
        : []
      : cellPaint.agentIds;

    for (const agentId of agentIds) {
      const from = positions.get(agentId);
      if (!from) continue;
      candidates.push({
        thread: { key: threadKey(agentId, cell.path), from, to: centre(cell) },
        churn: cellPaint.churn,
      });
    }
  }

  candidates.sort(
    (left, right) => right.churn - left.churn || left.thread.key.localeCompare(right.thread.key),
  );
  return candidates.slice(0, max).map((candidate) => candidate.thread);
}

