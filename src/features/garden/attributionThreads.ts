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
 * Paths that have a more specific path in the same set, and so should not be
 * threaded.
 *
 * `buildTerrainPaint` rolls every change onto its path *and every ancestor*, and
 * pools `agentIds` and `evidence` up the chain — that is what lets a folder say
 * its subtree changed. It also means an agent that wrote one file appears on
 * every folder above it, so without this the map drew a thread to the file, to
 * `src/features`, to `src`, and to the repository root: four lines making one
 * claim.
 *
 * Worse, the churn cap ranked them backwards. A folder's churn is the sum of its
 * subtree, so ancestors always outrank the file they contain, and `MAX_THREADS`
 * filled with the least specific cells while the actual writes were cut. The
 * lines that survived ended at the geometric centre of a large folder rect,
 * where nothing is drawn — a label sits at its top-left — which reads exactly
 * like a line going nowhere.
 *
 * A folder still keeps its thread when it is the deepest *rendered* cell, which
 * is the truthful answer at that level of detail: the write is somewhere in
 * here, and here is as precise as the map currently is.
 */
function supersededByDescendant(paths: Iterable<string>): Set<string> {
  const present = new Set(paths);
  const superseded = new Set<string>();
  for (const path of present) {
    let cursor = path;
    for (;;) {
      const cut = cursor.lastIndexOf("/");
      // `cut <= 0` stops at a POSIX root (`/a`) and, together with the drive
      // prefix losing its slash on the next pass, at a Windows one (`d:/a`).
      if (cut <= 0) break;
      cursor = cursor.slice(0, cut);
      if (present.has(cursor)) superseded.add(cursor);
    }
  }
  return superseded;
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

  // One supersession set rather than one per agent, because a selection is
  // either a single agent or a single cell: the agent case leaves exactly one
  // claimant on every candidate, and the cell case leaves exactly one candidate.
  // Neither can put two agents at different depths of the same tree, so keying
  // by agent would be machinery guarding a state that cannot arise.
  const claimed: Array<{ cell: TerrainCell; churn: number; agentIds: readonly string[] }> = [];
  for (const cell of cells) {
    const cellPaint = paint.get(cell.path);
    if (!cellPaint || cellPaint.evidence !== "attributed") continue;
    if (selectedPath && cell.path !== selectedPath) continue;

    const agentIds = selectedAgentId
      ? cellPaint.agentIds.includes(selectedAgentId)
        ? [selectedAgentId]
        : []
      : cellPaint.agentIds;
    if (agentIds.length === 0) continue;

    claimed.push({ cell, churn: cellPaint.churn, agentIds });
  }

  const superseded = supersededByDescendant(claimed.map((entry) => entry.cell.path));

  const candidates: Array<{ thread: Thread; churn: number }> = [];
  for (const entry of claimed) {
    if (superseded.has(entry.cell.path)) continue;
    for (const agentId of entry.agentIds) {
      const from = positions.get(agentId);
      if (!from) continue;
      candidates.push({
        thread: { key: threadKey(agentId, entry.cell.path), from, to: centre(entry.cell) },
        churn: entry.churn,
      });
    }
  }

  candidates.sort(
    (left, right) => right.churn - left.churn || left.thread.key.localeCompare(right.thread.key),
  );
  return candidates.slice(0, max).map((candidate) => candidate.thread);
}

