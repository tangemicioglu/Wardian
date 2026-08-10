import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import type {
  ChangeReviewBaseline,
  ChangeReviewLoadResponse,
  ChangeReviewPrefs,
} from "../../types";
import { normalizeEntityPath } from "./entityRef";
import { buildTerrainPaint, type RootChangeSet, type TerrainPaint } from "./terrainPaint";

interface ExplorerChangedEvent {
  root_path: string;
  changed_paths: string[];
}

/** Change-review calls allowed in flight at once. */
export const MAX_CHANGE_CONCURRENCY = 4;

/**
 * Baselines the map can honestly render.
 *
 * `last_effective_turn`, `conversation_start`, and `unreviewed` all resolve
 * against one agent's snapshots and watermark. That is exactly right for the
 * Changes pane, which shows one selected agent — and meaningless for a map
 * showing every agent at once, where picking a representative would make the
 * whole colouring depend on an arbitrary choice.
 *
 * `head` and `branch_point` are computed from the repository and name no agent,
 * so the map adopts the pane's preference when it is one of these and falls
 * back to `head` when it is not. The legend states which baseline is in force,
 * so the two surfaces can differ but can never differ *silently*.
 */
const MAP_BASELINES: readonly ChangeReviewBaseline[] = ["head", "branch_point"];

export const DEFAULT_TERRAIN_BASELINE: ChangeReviewBaseline = "head";

export function terrainBaselineFor(preference: ChangeReviewBaseline): ChangeReviewBaseline {
  return MAP_BASELINES.includes(preference) ? preference : DEFAULT_TERRAIN_BASELINE;
}

export interface TerrainChangesOptions {
  enabled: boolean;
  /** Normalized workspace roots whose ground is currently on screen. */
  roots: readonly string[];
}

export interface TerrainChangesResult {
  /** Normalized absolute path -> paint, including every changed path's ancestors. */
  paint: ReadonlyMap<string, TerrainPaint>;
  baseline: ChangeReviewBaseline;
  /** Roots that reported no git repository, so the map can stay quiet about them. */
  withoutGit: ReadonlySet<string>;
}

const EMPTY_PAINT: ReadonlyMap<string, TerrainPaint> = new Map();
const EMPTY_ROOTS: ReadonlySet<string> = new Set();

/**
 * Fetch the change set for each visible workspace root and turn it into paint.
 *
 * One call per root, not one per agent: `load_change_review` attributes across
 * every conversation in the workspace, so the requesting agent affects only the
 * watermark and the snapshot baseline — neither of which a map-level baseline
 * uses. A 37-district install therefore costs a handful of `git status`
 * invocations rather than one per agent.
 *
 * Nothing polls. Recomputation is driven by `explorer-changed` for agent writes
 * and `git-changed` for the operator's own staging and commits, which is the
 * same pair the Changes pane subscribes to and for the same reasons:
 * `git_watch` observes only `.git/index` and `.git/HEAD`, so it can never see
 * the writes this paint exists to show.
 */
/**
 * A held summary, tagged with the baseline that produced it.
 *
 * Recorded rather than invalidated by a separate effect: an effect that clears
 * state on a baseline change has to run *before* the one that decides what is
 * missing, and relying on declaration order for that is a bug waiting for
 * someone to reorder two hooks. A tag makes staleness a property of the datum.
 */
interface HeldChangeSet extends RootChangeSet {
  baseline: ChangeReviewBaseline;
}

export function useTerrainChanges(options: TerrainChangesOptions): TerrainChangesResult {
  const { enabled, roots } = options;
  const [baseline, setBaseline] = useState<ChangeReviewBaseline>(DEFAULT_TERRAIN_BASELINE);
  const [baselineLoaded, setBaselineLoaded] = useState(false);
  const [changes, setChanges] = useState<ReadonlyMap<string, HeldChangeSet>>(
    () => new Map<string, HeldChangeSet>(),
  );
  const [withoutGit, setWithoutGit] = useState<ReadonlySet<string>>(() => new Set<string>());
  const inFlight = useRef(new Set<string>());

  const rootKey = useMemo(() => [...roots].sort().join(" "), [roots]);

  // The pane's habitual baseline, narrowed to what a map can render. Fetching
  // waits on this: starting with the default and correcting afterwards would
  // spend a `git status` per root on a question nobody asked.
  useEffect(() => {
    if (!enabled) return;
    let active = true;
    void Promise.resolve(invoke<ChangeReviewPrefs>("load_change_review_prefs"))
      .then((prefs) => {
        if (!active) return;
        if (prefs) setBaseline(terrainBaselineFor(prefs.baseline));
        setBaselineLoaded(true);
      })
      .catch(() => {
        if (active) setBaselineLoaded(true);
      });
    return () => {
      active = false;
    };
  }, [enabled]);

  const load = useCallback(
    async (targets: readonly string[]) => {
      const queue = targets.filter((root) => !inFlight.current.has(root));
      if (queue.length === 0) return;
      for (const root of queue) inFlight.current.add(root);

      // A fixed pool rather than `Promise.all`: each call is a `git status` on a
      // real repository, and firing thirty-seven at once is a disk-bound stall
      // that delays the ones the user is actually looking at.
      let cursor = 0;
      const worker = async () => {
        for (;;) {
          const index = cursor++;
          if (index >= queue.length) return;
          const root = queue[index];
          try {
            const response = await invoke<ChangeReviewLoadResponse>("load_change_review", {
              request: { cwd: root, baseline, agent_id: null },
            });
            if (!response) continue;
            setWithoutGit((current) => {
              const next = new Set(current);
              if (response.git_available) next.delete(root);
              else next.add(root);
              return next;
            });
            setChanges((current) => {
              const next = new Map(current);
              next.set(root, {
                root,
                baseline,
                entries: response.summary.files,
                toTurnIndex: response.summary.to_turn_index,
              });
              return next;
            });
          } catch {
            // A root that cannot be reviewed renders as unpainted ground. The
            // change-review spec is explicit that a failure here degrades the
            // view rather than becoming the view.
            setChanges((current) => {
              if (!current.has(root)) return current;
              const next = new Map(current);
              next.delete(root);
              return next;
            });
          } finally {
            inFlight.current.delete(root);
          }
        }
      };

      await Promise.all(
        Array.from({ length: Math.min(MAX_CHANGE_CONCURRENCY, queue.length) }, worker),
      );
    },
    [baseline],
  );

  // Load whatever is on screen and not held under the current baseline. Cached
  // results survive the root leaving the viewport, so panning back is free; a
  // baseline change makes every held set stale and reloads them.
  useEffect(() => {
    if (!enabled || !baselineLoaded || roots.length === 0) return;
    const missing = roots.filter((root) => changes.get(root)?.baseline !== baseline);
    if (missing.length === 0) return;
    void load(missing);
    // `changes` is read to find what is missing but must not retrigger this
    // effect on every arrival, or a slow root would restart the pass repeatedly.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, baselineLoaded, baseline, rootKey, load]);

  useEffect(() => {
    if (!enabled || roots.length === 0) return;
    const watched = new Set(roots);
    let active = true;

    const refresh = (rootPath: string) => {
      const root = normalizeEntityPath(rootPath);
      if (!active || !root || !watched.has(root)) return;
      void load([root]);
    };

    const unlistenExplorer = listen<ExplorerChangedEvent>("explorer-changed", (event) =>
      refresh(event.payload.root_path),
    );
    const unlistenGit = listen<string>("git-changed", (event) => refresh(event.payload));

    return () => {
      active = false;
      void Promise.resolve(unlistenExplorer.then((unlisten) => unlisten?.())).catch(
        () => undefined,
      );
      void Promise.resolve(unlistenGit.then((unlisten) => unlisten?.())).catch(() => undefined);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, rootKey, load]);

  const paint = useMemo(() => {
    if (!enabled || changes.size === 0) return EMPTY_PAINT;
    // Only sets answering the current question. A set held under a superseded
    // baseline describes a comparison the legend is no longer claiming.
    const current = [...changes.values()].filter((held) => held.baseline === baseline);
    if (current.length === 0) return EMPTY_PAINT;
    return buildTerrainPaint(current);
  }, [enabled, changes, baseline]);

  return {
    paint,
    baseline,
    withoutGit: enabled ? withoutGit : EMPTY_ROOTS,
  };
}
