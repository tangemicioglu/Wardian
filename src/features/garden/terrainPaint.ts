/**
 * Change paint for Garden terrain.
 *
 * Kept rigorously separate from `terrain.ts`, and the separation is the point.
 * The stability contract in `docs/specs/2026-07-30-garden-metric-map.md` is
 * enforced by a type boundary — `LayoutInput` accepts no telemetry, so geometry
 * cannot depend on it — and churn is the same class of signal as status. A map
 * whose landmarks moved when someone saved a file could not be navigated, and
 * its stored positions would stop meaning anything.
 *
 * So this module consumes cells and produces colour. It cannot produce a rect,
 * and `buildTerrain` cannot see a change set.
 *
 * ## Why the rollup walks paths rather than cells
 *
 * A folder cell must show that something happened inside it whether or not its
 * subtree is drawn — otherwise the map rewards expansion, and reading it means
 * opening everything. The rollup therefore climbs the ancestor chain of every
 * *changed path*, which is bounded by the size of the change set, rather than
 * descending from drawn cells, which would find nothing beyond the frontier.
 */

import type {
  ChangeReviewChangeKind,
  ChangeReviewEvidence,
  ChangeReviewFileEntry,
} from "../../types";
import { normalizeEntityPath } from "./entityRef";

/**
 * The aggregate kind of a cell.
 *
 * `mixed` is a real answer rather than a fallback: a folder holding an addition
 * and a deletion is not "modified", and picking the most frequent kind would
 * let one more file flip a folder's colour. A precedence order would be worse
 * still — it would assert that deletions matter more than additions, which is a
 * claim about the work, not about the data.
 */
export type TerrainChangeKind = ChangeReviewChangeKind | "mixed";

export interface TerrainPaint {
  kind: TerrainChangeKind;
  /** 0..1, log-scaled and normalized across every root on the map. */
  churn: number;
  /** 0..1, decayed by how many turns ago the newest change landed. */
  recency: number;
  /** `attributed` when any changed path beneath the cell maps to an agent. */
  evidence: ChangeReviewEvidence;
  /** True only when every changed path beneath the cell has been reviewed. */
  reviewed: boolean;
  /** Union of the agents that wrote beneath this cell. */
  agentIds: readonly string[];
  /** Changed paths rolled up into this cell. */
  count: number;
}

export interface RootChangeSet {
  /** Normalized absolute workspace root. */
  root: string;
  entries: readonly ChangeReviewFileEntry[];
  /** Newest turn the summary covers, for the recency decay. */
  toTurnIndex: number | null;
}

/**
 * Turns over which a change fades to half intensity.
 *
 * Expressed in turns rather than wall-clock time because turns are what the
 * change set actually carries, and because an agent that ran overnight and one
 * that ran a minute ago have produced the same amount of work. Wall clock would
 * make the map fade while nothing happened.
 */
export const RECENCY_HALF_LIFE_TURNS = 8;

/**
 * Recency for a path whose turn is unknown.
 *
 * `inferred` entries — writes performed through a shell — carry no turn
 * indices, and the change-review spec is explicit that they are a lower bound
 * and never a filter. Painting them at zero would hide exactly the writes that
 * `inferred` exists to surface; painting them at full intensity would claim a
 * freshness the data does not support. Mid is the honest reading.
 */
const UNKNOWN_RECENCY = 0.55;

interface Accumulator {
  kinds: Set<TerrainChangeKind>;
  churn: number;
  newestTurn: number | null;
  attributed: boolean;
  allReviewed: boolean;
  agentIds: Set<string>;
  count: number;
}

/**
 * Roll a set of per-root change summaries onto every path that can carry paint.
 *
 * The result is keyed by normalized absolute path and contains every changed
 * path *and every ancestor of one* up to its root, so a cell at any depth can
 * look itself up in one probe.
 *
 * A deleted path keeps its own entry even though no cell will ever match it —
 * it is not on disk. Its ancestors carry the signal, which is why a deletion
 * shows up on the parent folder rather than as a hole in the treemap that the
 * next listing would silently close.
 */
export function buildTerrainPaint(
  changes: readonly RootChangeSet[],
): Map<string, TerrainPaint> {
  const accumulators = new Map<string, Accumulator>();

  for (const change of changes) {
    const root = normalizeEntityPath(change.root);
    if (!root) continue;

    for (const entry of change.entries) {
      const absolute = joinWorkspacePath(root, entry.path);
      if (!absolute) continue;

      const churn = (entry.insertions ?? 0) + (entry.deletions ?? 0);
      const newestTurn =
        entry.turn_indices.length > 0 ? Math.max(...entry.turn_indices) : null;
      const distance =
        newestTurn !== null && change.toTurnIndex !== null
          ? Math.max(0, change.toTurnIndex - newestTurn)
          : null;

      for (const path of ancestorChain(absolute, root)) {
        const accumulator = accumulators.get(path) ?? emptyAccumulator();
        accumulator.kinds.add(entry.change_kind);
        accumulator.churn += churn;
        if (distance !== null) {
          accumulator.newestTurn =
            accumulator.newestTurn === null
              ? distance
              : Math.min(accumulator.newestTurn, distance);
        }
        if (entry.evidence === "attributed") accumulator.attributed = true;
        if (!entry.reviewed) accumulator.allReviewed = false;
        for (const agentId of entry.agent_ids) accumulator.agentIds.add(agentId);
        accumulator.count += 1;
        accumulators.set(path, accumulator);
      }
    }
  }

  // Normalized across every root rather than within each one. Per-root
  // normalization would give a repository with three changed lines the same
  // saturation as one with three thousand, which is precisely the comparison a
  // map showing several repositories exists to make.
  let maxChurn = 0;
  for (const accumulator of accumulators.values()) {
    maxChurn = Math.max(maxChurn, accumulator.churn);
  }
  const denominator = Math.log1p(maxChurn);

  const paint = new Map<string, TerrainPaint>();
  for (const [path, accumulator] of accumulators) {
    paint.set(path, {
      kind: accumulator.kinds.size === 1 ? [...accumulator.kinds][0] : "mixed",
      churn: denominator > 0 ? Math.log1p(accumulator.churn) / denominator : 0,
      recency:
        accumulator.newestTurn !== null
          ? 2 ** (-accumulator.newestTurn / RECENCY_HALF_LIFE_TURNS)
          : UNKNOWN_RECENCY,
      evidence: accumulator.attributed ? "attributed" : "inferred",
      reviewed: accumulator.allReviewed,
      agentIds: [...accumulator.agentIds].sort(),
      count: accumulator.count,
    });
  }
  return paint;
}

function emptyAccumulator(): Accumulator {
  return {
    kinds: new Set(),
    churn: 0,
    newestTurn: null,
    attributed: false,
    allReviewed: true,
    agentIds: new Set(),
    count: 0,
  };
}

/**
 * Absolute path for a change-review entry.
 *
 * `ChangeReviewFileEntry.path` is workspace-relative in git's own spelling, but
 * the type does not forbid an absolute path and one root's entry must never be
 * joined onto another's. An entry that already looks absolute is taken as it
 * stands, matching `pathForWorkspace` in the Changes pane.
 */
export function joinWorkspacePath(root: string, path: string): string | null {
  const trimmed = path.trim();
  if (!trimmed) return null;
  const absolute =
    /^[A-Za-z]:[\\/]/.test(trimmed) || trimmed.startsWith("/") || trimmed.startsWith("\\\\")
      ? trimmed
      : `${root.replace(/[\\/]+$/g, "")}/${trimmed.replace(/^[\\/]+/g, "")}`;
  return normalizeEntityPath(absolute);
}

/** The path itself and every ancestor up to and including `root`. */
export function ancestorChain(path: string, root: string): string[] {
  const chain: string[] = [];
  let current = path;
  // A path outside its root cannot be walked to it, so it contributes only
  // itself rather than climbing to the filesystem root and painting everything.
  if (current !== root && !current.startsWith(`${root}/`)) return [current];
  while (current.length >= root.length) {
    chain.push(current);
    if (current === root) break;
    const separator = current.lastIndexOf("/");
    if (separator <= 0) break;
    current = current.slice(0, separator);
  }
  return chain;
}
