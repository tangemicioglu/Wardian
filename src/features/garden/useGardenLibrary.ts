import { useEffect, useMemo } from "react";
import type { LibraryEntry, LibraryIndex, LibraryIndexFolder } from "../../types";
import { isLibraryEntry } from "../../types";
import { useLibraryStore } from "../../store/useLibraryStore";
import type { GardenLibraryKind } from "./garden.types";

export interface GardenLibraryDeployment {
  targetType: string;
  targetId: string;
  /** False when the skill was copied rather than junctioned; edits do not sync. */
  linked: boolean;
}

export interface GardenLibraryInput {
  /** `<section>/<rel_path>`, the library's own stable identity. */
  entryRef: string;
  kind: GardenLibraryKind;
  label: string;
  tags: string[];
  deployments: GardenLibraryDeployment[];
}

/**
 * Library sections the Garden places.
 *
 * `workflows` is excluded on purpose: blueprints already enter through
 * `useGardenWorkflows`, keyed by `Blueprint.id`. Admitting them here as well
 * would reintroduce the dual-identity bug `entityRef.ts` exists to prevent —
 * the same workflow as two units, at two positions, with two facet vectors.
 *
 * `mcps` is stubbed in the backend and never has entries.
 */
const PLACED_SECTIONS: Record<string, GardenLibraryKind> = {
  skills: "skill",
  prompts: "prompt",
  classes: "class",
};

/** Flatten the library index into Garden inputs. Pure, so it is directly testable. */
export function gardenLibraryInputs(index: LibraryIndex | null): GardenLibraryInput[] {
  if (!index) return [];

  const inputs: GardenLibraryInput[] = [];
  for (const [sectionId, kind] of Object.entries(PLACED_SECTIONS)) {
    const section = index.sections?.[sectionId as keyof typeof index.sections];
    if (!section || section.stubbed) continue;
    for (const entry of flattenEntries(section.tree)) {
      // An entry that failed to parse has no trustworthy description or tags;
      // placing it would assert a relationship the file cannot back.
      if (entry.error) continue;
      inputs.push({
        entryRef: entry.entry_ref,
        kind,
        label: entry.name,
        tags: entry.tags ?? [],
        deployments: (index.deployments?.[entry.entry_ref] ?? []).map((target) => ({
          targetType: target.target_type,
          targetId: target.target_id,
          linked: target.linked,
        })),
      });
    }
  }
  // Sorted so the layout's district and facet passes see a stable order.
  return inputs.sort((left, right) => left.entryRef.localeCompare(right.entryRef));
}

function flattenEntries(folder: LibraryIndexFolder): LibraryEntry[] {
  const entries: LibraryEntry[] = [];
  const walk = (node: LibraryIndexFolder) => {
    for (const child of node.children) {
      if (isLibraryEntry(child)) entries.push(child);
      else walk(child);
    }
  };
  walk(folder);
  return entries;
}

/**
 * Library assets for the Garden.
 *
 * Reads the shared library store rather than invoking directly: the Library view
 * already owns fetching and refreshes on the `library-changed` event, so the
 * Garden gets updates for free and cannot disagree with it about what exists.
 */
export function useGardenLibrary(enabled = true): GardenLibraryInput[] {
  const index = useLibraryStore((state) => state.index);
  const fetchIndex = useLibraryStore((state) => state.fetchIndex);

  useEffect(() => {
    if (!enabled || index) return;
    void fetchIndex();
  }, [enabled, index, fetchIndex]);

  return useMemo(() => (enabled ? gardenLibraryInputs(index) : []), [enabled, index]);
}
