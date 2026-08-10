import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { normalizeEntityPath } from "./entityRef";
import {
  buildTerrain,
  type TerrainCell,
  type TerrainChild,
  type TerrainDistrict,
  type TerrainListing,
} from "./terrain";
import {
  MAX_TERRAIN_CELLS,
  frontierRequests,
  intersectsViewport,
  minSubdivideArea,
  staleListings,
  type TerrainViewport,
} from "./terrainFrontier";
import type { FileNode } from "../explorer/FileTree";

interface ExplorerChangedEvent {
  root_path: string;
  changed_paths: string[];
}

/** Delay between the viewport settling and the listings it implies. */
export const EXPANSION_DEBOUNCE_MS = 200;

/**
 * Issue a command whose result nobody waits for, and swallow every failure.
 *
 * Used only on the watch and unwatch paths. An unwatch runs during unmount, and
 * a throw there tears down the commit rather than the effect — so the call is
 * wrapped rather than assumed to return a rejectable promise. Failing to
 * unwatch costs a stale registration; failing to unmount costs the view.
 */
function fireAndForget(promise: unknown): void {
  void Promise.resolve(promise).catch(() => undefined);
}

/**
 * Zoom quantum for the subdivision threshold.
 *
 * `minSubdivideArea` is a continuous function of scale, so feeding raw zoom in
 * would rebuild the whole treemap on every wheel tick and let a cell hovering
 * at the boundary flicker in and out on a one-pixel change. Quantizing to
 * sqrt(2) steps means detail changes when the zoom changes meaningfully, and
 * the rebuild happens once per step rather than once per frame.
 */
const SCALE_STEPS_PER_OCTAVE = 2;

/**
 * Separator for the root-set dependency key.
 *
 * NUL, matching the pair keys in `metric.ts`. A printable delimiter would be
 * wrong rather than merely ugly: a workspace path may contain a space or a
 * comma, so two different root sets could produce one key and a genuine change
 * of roots would not re-register the watchers.
 */
export const ROOT_KEY_SEPARATOR = "\u0000";

export function quantizeScale(scale: number): number {
  if (!Number.isFinite(scale) || scale <= 0) return 1;
  const octave = Math.log2(scale) * SCALE_STEPS_PER_OCTAVE;
  return 2 ** (Math.round(octave) / SCALE_STEPS_PER_OCTAVE);
}

export interface GardenTerrainOptions {
  enabled: boolean;
  /** districtId -> roots and extent, from `computeGardenLayout`. */
  districts: ReadonlyMap<string, TerrainDistrict>;
  /** Null until the canvas has measured itself and applied a transform. */
  viewport: TerrainViewport | null;
}

export interface GardenTerrainResult {
  cells: TerrainCell[];
  /** Directories with a listing in flight, for a loading affordance. */
  pending: ReadonlySet<string>;
  /**
   * Workspace roots whose ground is currently on screen.
   *
   * Published so change review is fetched for the repositories the operator is
   * actually looking at. On a 37-district install, zooming into one project
   * should not cost a `git status` on the other thirty-six.
   */
  visibleRoots: string[];
}

/**
 * Ingest and lay out the ground beneath the Garden's districts.
 *
 * Ingestion is lazy by construction: a directory is listed only when its cell
 * is large enough on screen to be worth subdividing, which is the same boundary
 * `facets.ts` uses to admit files to the IDF corpus. There is no recursive
 * enumeration here, and nothing polls — invalidation comes from
 * `explorer-changed`, the 150 ms-debounced working-tree watcher that already
 * excludes `.git`, `node_modules`, `target`, `dist`, and the rest.
 *
 * `ExplorerPanel`'s three-second `git_status` interval is deliberately not
 * imitated. It exists to paint badges on one tree; at map scale it would be one
 * timer per root.
 */
export function useGardenTerrain(options: GardenTerrainOptions): GardenTerrainResult {
  const { enabled, districts, viewport } = options;
  const [listings, setListings] = useState<ReadonlyMap<string, TerrainListing>>(
    () => new Map<string, TerrainListing>(),
  );
  const [pending, setPending] = useState<ReadonlySet<string>>(() => new Set<string>());
  const inFlight = useRef(new Set<string>());
  // Directories that could not be read. Retried only after an explicit
  // invalidation, so an unreadable path costs one call rather than one per
  // frontier evaluation forever.
  const failed = useRef(new Set<string>());

  // The live listing map, for the watcher. Reading it from a ref rather than
  // depending on it keeps a single subscription across every ingestion, instead
  // of tearing the watchers down and re-registering them each time a folder
  // arrives — which loses whatever the filesystem did in the gap.
  const listingsRef = useRef(listings);
  listingsRef.current = listings;

  const roots = useMemo(() => {
    const unique = new Set<string>();
    for (const district of districts.values()) {
      for (const root of district.roots) unique.add(root);
    }
    return [...unique].sort();
  }, [districts]);
  const rootKey = roots.join(ROOT_KEY_SEPARATOR);

  // Drop listings belonging to roots that have left the roster.
  //
  // The frontier budget is a count of cached directories (`MAX_FRONTIER_DIRS`),
  // and nothing used to remove entries — only unreadable refreshes were deleted.
  // So the listings of a district that was removed, renamed, or re-districted
  // kept their share of the allowance forever, and once the cache filled with
  // them `frontierRequests` returned nothing and a *newly added* root could
  // never expand past its own root cell.
  //
  // Keyed on `rootKey` so this runs when the root set changes rather than on
  // every ingestion, and it deliberately keeps everything under a surviving
  // root: those listings are still correct and re-fetching them would be the
  // eviction-and-refetch cycle that made the ground blink.
  useEffect(() => {
    if (!enabled) return;
    setListings((current) => {
      if (current.size === 0) return current;
      const next = new Map<string, TerrainListing>();
      for (const [path, listing] of current) {
        if (roots.some((root) => path === root || path.startsWith(`${root}/`))) {
          next.set(path, listing);
        }
      }
      return next.size === current.size ? current : next;
    });
    // `rootKey` is the content of `roots`; the array identity changes on every
    // layout pass and would make this run constantly.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, rootKey]);

  const requestListings = useCallback(async (paths: readonly string[]) => {
    const wanted = paths.filter(
      (path) => !inFlight.current.has(path) && !failed.current.has(path),
    );
    if (wanted.length === 0) return;
    for (const path of wanted) inFlight.current.add(path);
    setPending(new Set(inFlight.current));

    const results = await Promise.all(
      wanted.map(async (path): Promise<TerrainListing | null> => {
        try {
          const nodes = await invoke<FileNode[]>("get_directory_tree", { path });
          return { path, children: toChildren(nodes ?? []) };
        } catch {
          failed.current.add(path);
          return null;
        }
      }),
    );

    for (const path of wanted) inFlight.current.delete(path);
    setPending(new Set(inFlight.current));

    const fetched = results.filter((listing): listing is TerrainListing => listing !== null);
    const unreadable = wanted.filter((path) => failed.current.has(path));
    if (fetched.length === 0 && unreadable.length === 0) return;
    setListings((current) => {
      const next = new Map(current);
      for (const listing of fetched) next.set(listing.path, listing);
      // A refresh that could not be read is the one case where dropping a
      // listing is right: the directory is gone or unreadable, and keeping its
      // children on screen would be the map asserting what it just failed to
      // confirm. Its parent's refresh removes the cell itself.
      for (const path of unreadable) next.delete(path);
      return next;
    });
  }, []);

  // Watch every root and refresh the listings a change actually invalidates.
  //
  // Refresh, not eviction: see `staleListings`. Scoped by the event's own
  // `changed_paths`, which accumulate across the 150 ms debounce window rather
  // than being sampled from it, so the set is complete for that window.
  useEffect(() => {
    if (!enabled || roots.length === 0) return;
    let disposed = false;
    const watched: string[] = [];

    const unlistenPromise = listen<ExplorerChangedEvent>("explorer-changed", (event) => {
      const changedRoot = normalizeEntityPath(event.payload.root_path);
      if (!changedRoot || !roots.includes(changedRoot)) return;
      const changed = event.payload.changed_paths
        .map((path) => normalizeEntityPath(path))
        .filter((path): path is string => Boolean(path));
      // Retry only what this event touched. Clearing every failure would make
      // one unreadable folder cost a call on every write anywhere in the repo.
      for (const path of changed) failed.current.delete(path);
      const stale = staleListings(listingsRef.current, changedRoot, changed);
      if (stale.length > 0) void requestListings(stale);
    });

    void (async () => {
      for (const root of roots) {
        if (disposed) return;
        try {
          await invoke("explorer_watch", { rootPath: root });
          if (disposed) {
            fireAndForget(invoke("explorer_unwatch", { rootPath: root }));
            return;
          }
          watched.push(root);
        } catch {
          // A root that cannot be watched still renders; it just will not
          // refresh itself. Failing the whole terrain over one of several roots
          // would be a worse trade.
        }
      }
    })();

    return () => {
      disposed = true;
      for (const root of watched) {
        fireAndForget(invoke("explorer_unwatch", { rootPath: root }));
      }
      fireAndForget(unlistenPromise.then((unlisten) => unlisten?.()));
    };
    // `rootKey` is the content of `roots`; depending on the array identity would
    // re-register every watcher on each layout pass.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, rootKey]);

  const quantizedScale = viewport ? quantizeScale(viewport.scale) : 0;

  const cells = useMemo(() => {
    if (!enabled || !quantizedScale) return [];
    return buildTerrain({
      districts,
      listings,
      minSubdivideArea: minSubdivideArea(quantizedScale),
      maxCells: MAX_TERRAIN_CELLS,
    });
  }, [enabled, districts, listings, quantizedScale]);

  // Expansion is debounced so a wheel gesture sweeping four zoom levels issues
  // listings for the level it lands on, not for each level it passed through.
  useEffect(() => {
    if (!enabled || !viewport || cells.length === 0) return;
    const timer = window.setTimeout(() => {
      const requests = frontierRequests(cells, viewport, listings, {
        inFlight: inFlight.current.size,
      });
      const fresh = requests.filter((path) => !failed.current.has(path));
      if (fresh.length > 0) void requestListings(fresh);
    }, EXPANSION_DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [enabled, viewport, cells, listings, requestListings]);

  const visibleRoots = useMemo(() => {
    if (!viewport) return [];
    const visible = cells
      .filter((cell) => cell.depth === 0 && intersectsViewport(cell.rect, viewport.world))
      .map((cell) => cell.path);
    return [...new Set(visible)].sort();
  }, [cells, viewport]);

  return { cells, pending, visibleRoots };
}

/**
 * Adapt `get_directory_tree` output.
 *
 * Paths are normalized here so terrain keys, district roots, and the change
 * set all compare in one scheme — the map treats several key schemes as one
 * keyspace, and a folder arriving under two spellings would render twice.
 * The backend already sorts directories first then by name; re-sorting keeps
 * the treemap deterministic even if that ever changes.
 */
function toChildren(nodes: readonly FileNode[]): TerrainChild[] {
  const children: TerrainChild[] = [];
  for (const node of nodes) {
    const path = normalizeEntityPath(node.path);
    if (!path) continue;
    children.push({
      name: node.name,
      path,
      isDir: node.is_dir,
      extension: node.extension ?? null,
    });
  }
  children.sort((left, right) => {
    if (left.isDir !== right.isDir) return left.isDir ? -1 : 1;
    return left.path < right.path ? -1 : left.path > right.path ? 1 : 0;
  });
  return children;
}
